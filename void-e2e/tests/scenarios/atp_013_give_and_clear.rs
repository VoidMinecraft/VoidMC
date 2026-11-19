//! VMC-ATP-013: `/give` and `/clear` update the server-authoritative player
//! inventory, respect per-item stack limits, and keep the client's view of
//! the player window (window 0) equal to the server's slot for slot.

use bevy_ecs::entity::Entity;
use voidmc::{Inventory, ItemStack as ServerStack};
use voidmc_e2e::inventory::operations::ClickType;
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::protocol::packets::game::s_container_click::{
    HashedStack, ServerboundContainerClick,
};
use voidmc_e2e::registry::builtin::{EntityKind, ItemKind};
use voidmc_e2e::{Bot, TestServer};

const PLAYER_WINDOW: i32 = 0;
const SLOTS: usize = Inventory::SIZE;

/// A slot index of window 0, the item and its count.
type Update = (u16, ItemKind, i32);

/// An inventory packet for window 0, as the client decoded it.
#[derive(Debug, PartialEq)]
enum WindowUpdate {
    Slot(u16, ItemStack),
    Content(Vec<ItemStack>, ItemStack),
    Cursor(ItemStack),
}

/// What one command produced for the bot that ran it.
struct Outcome {
    chat: Vec<String>,
    inventory: Vec<WindowUpdate>,
    state_ids: Vec<u32>,
    item_entities: usize,
}

impl Outcome {
    fn slots(&self) -> Vec<(u16, ItemStack)> {
        self.inventory
            .iter()
            .map(|sync| match sync {
                WindowUpdate::Slot(slot, item) => (*slot, item.clone()),
                other => panic!("expected only per-slot updates, got {other:?}"),
            })
            .collect()
    }
}

fn stack(kind: ItemKind, count: i32) -> ItemStack {
    ItemStack::Present(ItemStackData::new(kind, count))
}

fn inventory_packet(packet: &ClientboundGamePacket) -> Option<(WindowUpdate, Option<u32>)> {
    match packet {
        ClientboundGamePacket::ContainerSetSlot(p) if p.container_id == PLAYER_WINDOW => Some((
            WindowUpdate::Slot(p.slot, p.item_stack.clone()),
            Some(p.state_id),
        )),
        ClientboundGamePacket::ContainerSetContent(p) if p.container_id == PLAYER_WINDOW => Some((
            WindowUpdate::Content(p.items.clone(), p.carried_item.clone()),
            Some(p.state_id),
        )),
        ClientboundGamePacket::SetCursorItem(p) => {
            Some((WindowUpdate::Cursor(p.contents.clone()), None))
        }
        _ => None,
    }
}

/// Runs `command` and returns everything the server sent back for it.
async fn run(bot: &Bot, command: &str) -> Outcome {
    bot.sync().await;
    bot.clear();
    bot.command(command).await;
    bot.sync().await;
    let chat = bot.queued(|packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => Some(chat.content.to_string()),
        _ => None,
    });
    let (inventory, state_ids): (Vec<WindowUpdate>, Vec<Option<u32>>) =
        bot.queued(inventory_packet).into_iter().unzip();
    let item_entities = bot
        .queued(|packet| match packet {
            ClientboundGamePacket::AddEntity(add) if add.entity_type == EntityKind::Item => {
                Some(())
            }
            _ => None,
        })
        .len();
    Outcome {
        chat,
        inventory,
        state_ids: state_ids.into_iter().flatten().collect(),
        item_entities,
    }
}

async fn server_inventory(server: &TestServer, entity: Entity) -> (Vec<ServerStack>, ServerStack) {
    server
        .with_world(move |world| {
            let inventory = world
                .get::<Inventory>(entity)
                .expect("the player has an inventory");
            (
                (0..SLOTS).map(|i| inventory.get(i).clone()).collect(),
                inventory.cursor().clone(),
            )
        })
        .await
}

/// The server's slots must be exactly `expected` (every other slot and the
/// cursor empty), with default components.
async fn assert_server_holds(
    server: &TestServer,
    entity: Entity,
    expected: &[(usize, ItemKind, u8)],
) {
    let (slots, cursor) = server_inventory(server, entity).await;
    let mut want = vec![ServerStack::EMPTY; SLOTS];
    for &(slot, kind, count) in expected {
        want[slot] = ServerStack::of(kind.to_str(), count).expect("a known item");
    }
    for (index, (have, want)) in slots.iter().zip(&want).enumerate() {
        assert!(
            (have.is_empty() && want.is_empty()) || have == want,
            "server slot {index}: have {have:?}, want {want:?}"
        );
    }
    assert!(cursor.is_empty(), "server cursor holds {cursor:?}");
}

fn as_pair(stack: &ServerStack) -> Option<(&'static str, i32)> {
    (!stack.is_empty()).then(|| {
        assert!(stack.components.is_empty(), "{stack:?} has components");
        (
            stack.item.name().expect("a known item"),
            i32::from(stack.count),
        )
    })
}

fn client_pair(stack: &ItemStack) -> Option<(&'static str, i32)> {
    stack.as_present().map(|data| {
        assert!(
            data.component_patch.iter().next().is_none(),
            "{data:?} carries a component patch"
        );
        (data.kind.to_str(), data.count)
    })
}

/// The client's window 0 and cursor equal the server's `Inventory`.
async fn assert_converged(bot: &Bot, server: &TestServer, entity: Entity) {
    bot.sync().await;
    let (slots, cursor) = server_inventory(server, entity).await;
    let (view, view_cursor) = bot.view(|view| (view.inventory.clone(), view.cursor.clone()));
    assert_eq!(
        view.len(),
        SLOTS,
        "{} sees a window of {}",
        bot.name(),
        view.len()
    );
    for (index, (server_slot, client_slot)) in slots.iter().zip(&view).enumerate() {
        assert_eq!(
            as_pair(server_slot),
            client_pair(client_slot),
            "{}: slot {index} differs between server and client",
            bot.name()
        );
    }
    assert_eq!(
        as_pair(&cursor),
        client_pair(&view_cursor),
        "cursor differs"
    );
}

/// A click that changes nothing, carrying the client's current state id: a
/// stale id is answered with a full resync, a current one with nothing.
async fn assert_state_id_current(bot: &Bot, empty_slot: i16) {
    let state_id = bot.view(|view| view.inventory_state_id);
    bot.clear();
    bot.send(ServerboundContainerClick {
        container_id: PLAYER_WINDOW,
        state_id,
        slot_num: empty_slot,
        button_num: 0,
        click_type: ClickType::Pickup,
        changed_slots: Default::default(),
        carried_item: HashedStack(None),
    })
    .await;
    bot.sync().await;
    let resyncs = bot.queued(|packet| match packet {
        ClientboundGamePacket::ContainerSetContent(_) => Some(()),
        _ => None,
    });
    assert!(
        resyncs.is_empty(),
        "the server considered state id {state_id} stale and resynced the window"
    );
}

fn set_slots(
    server: &TestServer,
    entity: Entity,
    slots: Vec<(usize, ItemKind, u8)>,
    cursor: Option<(ItemKind, u8)>,
) -> impl Future<Output = ()> {
    server.with_world(move |world| {
        let mut inventory = world.get_mut::<Inventory>(entity).unwrap();
        for (slot, kind, count) in slots {
            inventory.set(slot, ServerStack::of(kind.to_str(), count).unwrap());
        }
        if let Some((kind, count)) = cursor {
            inventory.set_cursor(ServerStack::of(kind.to_str(), count).unwrap());
        }
    })
}

/// The ATP steps as written: give stone, give pearls, clear, inspecting the
/// server inventory and the client's view after each.
#[tokio::test]
async fn give_stone_and_pearls_then_clear() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let alice_entity = server.player_entity(alice.uuid()).await;
    let bob_entity = server.player_entity(bob.uuid()).await;

    let given = run(&bob, "give minecraft:diamond 3").await;
    assert_eq!(given.slots(), [(36, stack(ItemKind::Diamond, 3))]);
    bob.clear();

    let login_state_id = alice.view(|view| view.inventory_state_id);
    let out = run(&alice, "give minecraft:stone 20").await;
    assert_eq!(out.chat, ["Gave 20 x minecraft:stone"]);
    assert_eq!(
        out.slots(),
        [(36, stack(ItemKind::Stone, 20))],
        "one slot update for the first hotbar slot"
    );
    assert!(out.state_ids[0] > login_state_id);
    assert_eq!(alice.view(|view| view.inventory_state_id), out.state_ids[0]);
    assert_server_holds(&server, alice_entity, &[(36, ItemKind::Stone, 20)]).await;
    assert_converged(&alice, &server, alice_entity).await;
    assert_state_id_current(&alice, 9).await;

    let out = run(&alice, "give minecraft:ender_pearl 20").await;
    assert_eq!(out.chat, ["Gave 20 x minecraft:ender_pearl"]);
    assert_eq!(
        out.slots(),
        [
            (37, stack(ItemKind::EnderPearl, 16)),
            (38, stack(ItemKind::EnderPearl, 4)),
        ],
        "pearls stack to 16"
    );
    assert!(out.state_ids.windows(2).all(|w| w[0] < w[1]));
    assert_server_holds(
        &server,
        alice_entity,
        &[
            (36, ItemKind::Stone, 20),
            (37, ItemKind::EnderPearl, 16),
            (38, ItemKind::EnderPearl, 4),
        ],
    )
    .await;
    assert_converged(&alice, &server, alice_entity).await;
    assert_state_id_current(&alice, 9).await;

    let out = run(&alice, "clear").await;
    assert_eq!(out.chat, ["Inventory cleared"]);
    assert_eq!(
        out.inventory,
        [WindowUpdate::Content(
            vec![ItemStack::Empty; SLOTS],
            ItemStack::Empty
        )]
    );
    assert_server_holds(&server, alice_entity, &[]).await;
    assert_converged(&alice, &server, alice_entity).await;
    assert_state_id_current(&alice, 36).await;

    bob.sync().await;
    assert!(
        bob.queued(inventory_packet).is_empty(),
        "Bob's window changed"
    );
    assert_server_holds(&server, bob_entity, &[(36, ItemKind::Diamond, 3)]).await;
    assert_converged(&bob, &server, bob_entity).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

/// Matching stacks fill first, then empty slots hotbar-first; no stack ever
/// exceeds its item's limit and totals stay exact.
#[tokio::test]
async fn give_respects_stack_limits_and_fill_order() {
    use ItemKind::{DiamondSword, Dirt, EnderPearl, Stone};
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let entity = server.player_entity(alice.uuid()).await;
    let mut held: Vec<(usize, ItemKind, u8)> = Vec::new();

    let steps: [(&str, &str, &[Update]); 4] = [
        (
            "give minecraft:ender_pearl 20",
            "Gave 20 x minecraft:ender_pearl",
            &[(36, EnderPearl, 16), (37, EnderPearl, 4)],
        ),
        (
            "give minecraft:ender_pearl 12",
            "Gave 12 x minecraft:ender_pearl",
            &[(37, EnderPearl, 16)],
        ),
        (
            "give minecraft:diamond_sword 3",
            "Gave 3 x minecraft:diamond_sword",
            &[
                (38, DiamondSword, 1),
                (39, DiamondSword, 1),
                (40, DiamondSword, 1),
            ],
        ),
        (
            "give minecraft:stone 64",
            "Gave 64 x minecraft:stone",
            &[(41, Stone, 64)],
        ),
    ];
    for (command, reply, updates) in steps {
        let out = run(&alice, command).await;
        assert_eq!(out.chat, [reply], "{command}");
        let expected: Vec<_> = updates
            .iter()
            .map(|&(slot, kind, count)| (slot, stack(kind, count)))
            .collect();
        assert_eq!(out.slots(), expected, "{command}");
        for &(slot, kind, count) in updates {
            held.retain(|(s, ..)| *s != usize::from(slot));
            held.push((usize::from(slot), kind, count as u8));
        }
        assert_server_holds(&server, entity, &held).await;
        assert_converged(&alice, &server, entity).await;
    }

    // Dirt in the way at 42, a partial stone stack after the empty 43.
    set_slots(&server, entity, vec![(42, Dirt, 1), (44, Stone, 60)], None).await;
    held.extend([(42, Dirt, 1), (44, Stone, 60)]);
    assert_converged(&alice, &server, entity).await;

    let out = run(&alice, "give minecraft:stone 10").await;
    assert_eq!(out.chat, ["Gave 10 x minecraft:stone"]);
    assert_eq!(
        out.slots(),
        [(43, stack(Stone, 6)), (44, stack(Stone, 64))],
        "tops up the partial stack before using the earlier empty slot"
    );
    held.extend([(43, Stone, 6)]);
    held.retain(|(s, ..)| *s != 44);
    held.push((44, Stone, 64));
    assert_server_holds(&server, entity, &held).await;
    assert_converged(&alice, &server, entity).await;

    // The hotbar is full: the rest goes to the first main-inventory slot.
    let out = run(&alice, "give stone 64").await;
    assert_eq!(out.chat, ["Gave 64 x minecraft:stone"], "bare ids resolve");
    assert_eq!(out.slots(), [(9, stack(Stone, 6)), (43, stack(Stone, 64))]);
    held.retain(|(s, ..)| *s != 43);
    held.extend([(43, Stone, 64), (9, Stone, 6)]);
    assert_server_holds(&server, entity, &held).await;
    assert_converged(&alice, &server, entity).await;

    alice.assert_healthy();
    server.stop().await;
}

/// Giving more than fits gives what fits, says how much did not, and never
/// touches armor, crafting or offhand slots, nor drops or duplicates items.
#[tokio::test]
async fn give_beyond_capacity_keeps_counts_exact() {
    use ItemKind::{Dirt, Stone};
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let entity = server.player_entity(alice.uuid()).await;

    let mut held: Vec<(usize, ItemKind, u8)> = (Inventory::MAIN_START..Inventory::OFFHAND)
        .map(|slot| (slot, Dirt, 64))
        .collect();
    held.retain(|(slot, ..)| *slot != 20);
    held.push((20, Stone, 60));
    set_slots(&server, entity, held.clone(), None).await;
    assert_converged(&alice, &server, entity).await;

    let out = run(&alice, "give minecraft:stone 10").await;
    assert_eq!(out.chat, ["Gave 4 x minecraft:stone (6 didn't fit)"]);
    assert_eq!(out.slots(), [(20, stack(Stone, 64))]);
    assert_eq!(out.item_entities, 0, "nothing is dropped");
    held.retain(|(slot, ..)| *slot != 20);
    held.push((20, Stone, 64));
    assert_server_holds(&server, entity, &held).await;
    assert_converged(&alice, &server, entity).await;

    for (command, reply) in [
        (
            "give minecraft:stone 5",
            "Gave 0 x minecraft:stone (5 didn't fit)",
        ),
        (
            "give minecraft:diamond_sword 1",
            "Gave 0 x minecraft:diamond_sword (1 didn't fit)",
        ),
    ] {
        let out = run(&alice, command).await;
        assert_eq!(out.chat, [reply], "{command}");
        assert_eq!(out.inventory, [], "{command} changed the window");
        assert_eq!(out.item_entities, 0, "{command} dropped an item");
        assert_server_holds(&server, entity, &held).await;
        assert_converged(&alice, &server, entity).await;
    }

    alice.assert_healthy();
    server.stop().await;
}

/// Bad counts, bad ids and missing or extra arguments are answered with an
/// error and leave the inventory and the client's view as they were.
#[tokio::test]
async fn give_rejects_bad_input_without_touching_inventory() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let entity = server.player_entity(alice.uuid()).await;

    let out = run(&alice, "give minecraft:stone 5").await;
    assert_eq!(out.chat, ["Gave 5 x minecraft:stone"]);
    let held = [(36, ItemKind::Stone, 5)];

    for (command, error) in [
        ("give minecraft:stone 0", "below minimum 1"),
        ("give minecraft:stone -3", "below minimum 1"),
        ("give minecraft:stone 65", "above maximum 64"),
        ("give minecraft:stone 2147483648", "not a valid integer"),
        (
            "give minecraft:stone 99999999999999999999",
            "not a valid integer",
        ),
        ("give minecraft:stone abc", "not a valid integer"),
        ("give minecraft:stone 2.5", "not a valid integer"),
        ("give minecraft:not_an_item 1", "not a known item"),
        ("give minecraft: 1", "not a known item"),
        ("give :stone", "not a known item"),
        ("give Minecraft:Stone", "not a known item"),
        (
            "give minecraft:stone[max_stack_size=99] 1",
            "not a known item",
        ),
        ("give minecraft:stone 5 extra", "Too many arguments"),
        ("give", "Missing required argument <item>"),
        ("give minecraft:air", "Cannot give minecraft:air"),
        ("give air 5", "Cannot give minecraft:air"),
    ] {
        let out = run(&alice, command).await;
        assert!(
            out.chat.first().is_some_and(|line| line.contains(error)),
            "/{command} replied {:?}, expected an error containing {error:?}",
            out.chat
        );
        assert!(
            out.chat.iter().all(|line| !line.starts_with("Gave")),
            "/{command} claimed success: {:?}",
            out.chat
        );
        assert_eq!(out.inventory, [], "/{command} changed the window");
        assert_server_holds(&server, entity, &held).await;
        assert_converged(&alice, &server, entity).await;
    }

    alice.assert_healthy();
    server.stop().await;
}

/// `/clear` empties every slot of window 0 (crafting, armor, main, hotbar,
/// offhand) and the carried item, in one full-content packet; a second
/// player is untouched; clearing an empty inventory is harmless.
#[tokio::test]
async fn clear_empties_every_slot_and_the_cursor() {
    use ItemKind::{
        DiamondBoots, DiamondChestplate, DiamondHelmet, DiamondLeggings, DiamondSword, Dirt,
        EnderPearl, OakPlanks, Shield, Stone,
    };
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let alice_entity = server.player_entity(alice.uuid()).await;
    let bob_entity = server.player_entity(bob.uuid()).await;

    run(&bob, "give minecraft:diamond 3").await;
    bob.clear();

    let setup = vec![
        (1, OakPlanks, 1),
        (2, OakPlanks, 1),
        (3, OakPlanks, 1),
        (4, OakPlanks, 1),
        (5, DiamondHelmet, 1),
        (6, DiamondChestplate, 1),
        (7, DiamondLeggings, 1),
        (8, DiamondBoots, 1),
        (9, Dirt, 64),
        (35, Dirt, 12),
        (36, DiamondSword, 1),
        (44, Stone, 3),
        (45, Shield, 1),
    ];
    set_slots(&server, alice_entity, setup, Some((EnderPearl, 2))).await;
    alice.sync().await;
    assert_eq!(
        alice.view(|view| view.cursor.clone()),
        stack(EnderPearl, 2),
        "the client holds the carried pearls"
    );
    assert_converged(&alice, &server, alice_entity).await;

    let out = run(&alice, "clear").await;
    assert_eq!(out.chat, ["Inventory cleared"]);
    assert_eq!(
        out.inventory,
        [WindowUpdate::Content(
            vec![ItemStack::Empty; SLOTS],
            ItemStack::Empty
        )]
    );
    assert_eq!(alice.view(|view| view.inventory_state_id), out.state_ids[0]);
    assert_server_holds(&server, alice_entity, &[]).await;
    assert_converged(&alice, &server, alice_entity).await;
    assert_state_id_current(&alice, 36).await;

    let out = run(&alice, "clear").await;
    assert_eq!(out.chat, ["Inventory cleared"], "nothing to clear");
    assert_server_holds(&server, alice_entity, &[]).await;
    assert_converged(&alice, &server, alice_entity).await;

    bob.sync().await;
    assert!(
        bob.queued(inventory_packet).is_empty(),
        "Bob's window changed"
    );
    assert_server_holds(&server, bob_entity, &[(36, ItemKind::Diamond, 3)]).await;
    assert_converged(&bob, &server, bob_entity).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

/// After a reconnect the client's window again matches the server's.
#[tokio::test]
async fn rejoin_shows_the_server_inventory() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let old = server.player_entity(alice.uuid()).await;
    run(&alice, "give minecraft:stone 5").await;
    alice.disconnect().await;
    server
        .wait_for("Alice's old entity to despawn", move |world| {
            world.get_entity(old).is_err().then_some(())
        })
        .await;

    let alice = server.join("Alice").await;
    let entity = server.player_entity(alice.uuid()).await;
    assert_converged(&alice, &server, entity).await;
    let out = run(&alice, "give minecraft:stone 5").await;
    assert_eq!(out.chat, ["Gave 5 x minecraft:stone"]);
    assert_converged(&alice, &server, entity).await;

    alice.assert_healthy();
    server.stop().await;
}
