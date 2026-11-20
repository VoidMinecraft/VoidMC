//! VMC-ATP-022: an application registers an `ItemBehavior` for one item
//! through `VoidServer::add_plugin` (here `TestServer::builder().plugin`) and
//! public API only. The behaviour takes over that item's right-click and break
//! hooks; every other item keeps its default interaction.
//!
//! Existing coverage: `atp_006_shared_blocks` covers default placement against
//! every face, the hand whose item is placed, and an empty hand or a non-block
//! item placing nothing; the unit tests of `void/src/plugins/item_use.rs` cover
//! the break hooks in a bare app.

use std::sync::{Arc, Mutex};

use bevy_ecs::prelude::Entity;
use voidmc::components::ClientId;
use voidmc::world::{
    ChunkData, ChunkIndex, ChunkPos as ServerChunkPos, DimensionId, set_block_state,
};
use voidmc::{
    BlockBreakContext, BlockFace, BlockPosition, Hand, Inventory, ItemBehavior,
    ItemBehaviorRegistry, ItemId, ItemStack as ServerItemStack, ItemUseContext, UseResult,
};
use voidmc_e2e::block::BlockState;
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, Vec3};
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_interact::InteractionHand,
    s_player_action::{Action, ServerboundPlayerAction},
    s_set_carried_item::ServerboundSetCarriedItem,
    s_set_creative_mode_slot::ServerboundSetCreativeModeSlot,
    s_use_item::ServerboundUseItem,
    s_use_item_on::{BlockHit, ServerboundUseItemOn},
};
use voidmc_e2e::registry::builtin::{BlockKind, ItemKind};
use voidmc_e2e::{Bot, TestServer};

use crate::{feet, ground};

const MAIN_HAND_SLOT: u16 = 36;
const OFF_HAND_SLOT: u16 = 45;

/// The example server's `WandBehavior` (`void-example/src/main.rs`), which the
/// example binary cannot export: a stick right-clicked on a block places
/// glowstone against the clicked face and messages the player.
struct WandBehavior;

impl ItemBehavior for WandBehavior {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        ctx.reply("zap! placed glowstone");
        ctx.place_block(block_state("minecraft:glowstone"));
        UseResult::Handled
    }
}

/// What one hook call of [`Recorder`] saw.
#[derive(Clone, Debug, PartialEq)]
enum Call {
    UseOnBlock {
        player: Entity,
        client_id: u32,
        hand: Hand,
        held: ServerItemStack,
        position: BlockPosition,
        face: BlockFace,
        cursor: (f32, f32, f32),
        inside_block: bool,
    },
    Use {
        player: Entity,
        client_id: u32,
        hand: Hand,
        held: ServerItemStack,
    },
}

type Calls = Arc<Mutex<Vec<Call>>>;

/// Records every right-click hook call and answers it in chat.
struct Recorder {
    calls: Calls,
    on_block: UseResult,
}

impl ItemBehavior for Recorder {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        let target = ctx.block.expect("a use on a block carries its target");
        self.calls.lock().unwrap().push(Call::UseOnBlock {
            player: ctx.player,
            client_id: ctx.client_id,
            hand: ctx.hand,
            held: ctx.held.clone(),
            position: target.position,
            face: target.face,
            cursor: target.cursor,
            inside_block: target.inside_block,
        });
        ctx.reply("recorder: on block");
        self.on_block
    }

    fn on_use(&self, ctx: &mut ItemUseContext) -> UseResult {
        assert!(ctx.block.is_none(), "a use in the air has no target block");
        self.calls.lock().unwrap().push(Call::Use {
            player: ctx.player,
            client_id: ctx.client_id,
            hand: ctx.hand,
            held: ctx.held.clone(),
        });
        ctx.reply("recorder: in air");
        UseResult::Pass
    }
}

/// Answers a use on a block with a fixed chat line and result.
struct Answer(&'static str, UseResult);

impl ItemBehavior for Answer {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        ctx.reply(self.0);
        self.1
    }
}

fn block_state(item: &str) -> i32 {
    ItemId::from_name(item)
        .and_then(|item| item.default_block_state())
        .unwrap_or_else(|| panic!("{item} places no block"))
}

fn stack(item: &str, count: u8) -> ServerItemStack {
    ServerItemStack::of(item, count).unwrap()
}

fn kind(state: Option<BlockState>) -> Option<BlockKind> {
    state.map(BlockKind::from)
}

fn to_server(pos: BlockPos) -> BlockPosition {
    BlockPosition {
        x: pos.x,
        y: pos.y as i16,
        z: pos.z,
    }
}

async fn server_block(server: &TestServer, pos: BlockPos) -> Option<i32> {
    server
        .with_world(move |world| {
            let chunk = ServerChunkPos::new(pos.x.div_euclid(16), pos.z.div_euclid(16));
            let entity = *world
                .resource::<ChunkIndex>()
                .0
                .get(&(DimensionId::Overworld, chunk))?;
            world.get::<ChunkData>(entity)?.get_block(
                pos.x.rem_euclid(16) as u8,
                pos.y,
                pos.z.rem_euclid(16) as u8,
            )
        })
        .await
}

async fn server_slot(server: &TestServer, player: Entity, slot: usize) -> ServerItemStack {
    server
        .with_world(move |world| world.get::<Inventory>(player).unwrap().get(slot).clone())
        .await
}

async fn client_id(server: &TestServer, player: Entity) -> u32 {
    server
        .with_world(move |world| world.get::<ClientId>(player).unwrap().0)
        .await
}

/// A solid block two blocks east of `bot`, with air above it.
fn support(bot: &Bot) -> BlockPos {
    let below = ground(bot);
    bot.view(|view| view.ground_below(BlockPos::new(below.x + 2, below.y + 8, below.z)))
        .expect("the bot holds the chunk next to it")
}

async fn hold(bot: &Bot, slot: u16, item: Option<(ItemKind, i32)>) {
    bot.send(ServerboundSetCreativeModeSlot {
        slot_num: slot,
        item_stack: match item {
            Some((kind, count)) => ItemStack::Present(ItemStackData::new(kind, count)),
            None => ItemStack::Empty,
        },
    })
    .await;
}

fn hit(clicked: BlockPos, face: Direction) -> BlockHit {
    let normal = face.normal();
    BlockHit {
        block_pos: clicked,
        direction: face,
        location: Vec3::new(
            f64::from(clicked.x) + 0.5 + f64::from(normal.x) * 0.5,
            f64::from(clicked.y) + 0.5 + f64::from(normal.y) * 0.5,
            f64::from(clicked.z) + 0.5 + f64::from(normal.z) * 0.5,
        ),
        inside: false,
        world_border: false,
    }
}

async fn use_on(bot: &Bot, hand: InteractionHand, block_hit: BlockHit, seq: u32) {
    bot.send(ServerboundUseItemOn {
        hand,
        block_hit,
        seq,
    })
    .await;
}

async fn use_in_air(bot: &Bot, hand: InteractionHand, seq: u32) {
    bot.send(ServerboundUseItem {
        hand,
        seq,
        y_rot: 0.0,
        x_rot: 0.0,
    })
    .await;
}

/// What a vanilla client sends for one right-click on a block when it does
/// not expect the main-hand item to do anything: a use on the block and a use
/// in the air with the main hand, then a use on the block with the off hand.
async fn vanilla_click(bot: &Bot, block_hit: BlockHit, first_seq: u32) {
    use_on(bot, InteractionHand::MainHand, block_hit.clone(), first_seq).await;
    use_in_air(bot, InteractionHand::MainHand, first_seq + 1).await;
    use_on(bot, InteractionHand::OffHand, block_hit, first_seq + 2).await;
}

async fn start_breaking(bot: &Bot, pos: BlockPos, seq: u32) {
    bot.send(ServerboundPlayerAction {
        action: Action::StartDestroyBlock,
        pos,
        direction: Direction::Up,
        seq,
    })
    .await;
}

async fn expect_ack(bot: &Bot, seq: u32) {
    bot.expect(
        &format!("the acknowledgement of sequence {seq}"),
        |packet| match packet {
            ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == seq => Some(()),
            _ => None,
        },
    )
    .await;
}

async fn expect_chat(bot: &Bot, text: &str) {
    bot.expect(&format!("the chat line {text:?}"), |packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => {
            chat.content.to_string().contains(text).then_some(())
        }
        _ => None,
    })
    .await;
}

/// The queued system chat lines.
fn chats(bot: &Bot) -> Vec<String> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => Some(chat.content.to_string()),
        _ => None,
    })
}

/// The queued block updates at `pos`.
fn updates_at(bot: &Bot, pos: BlockPos) -> Vec<BlockState> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::BlockUpdate(update) if update.pos == pos => Some(update.block_state),
        _ => None,
    })
}

#[tokio::test]
async fn the_example_wand_places_glowstone_once_per_click_for_everyone() {
    let server = TestServer::builder()
        .plugin(|app| {
            app.world_mut()
                .resource_mut::<ItemBehaviorRegistry>()
                .register_for("minecraft:stick", WandBehavior);
        })
        .start()
        .await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let entity = server.player_entity(alice.uuid()).await;

    let clicked = support(&alice);
    let target = clicked.up(1);
    let clicked_state = server_block(&server, clicked).await;
    assert_eq!(server_block(&server, target).await, Some(0), "air to fill");

    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::Stick, 1))).await;
    alice.sync().await;
    alice.clear();
    bob.clear();
    vanilla_click(&alice, hit(clicked, Direction::Up), 1).await;
    for seq in 1..=3 {
        expect_ack(&alice, seq).await;
    }
    expect_chat(&alice, "zap! placed glowstone").await;
    bob.wait_until("the wand's glowstone", |view| {
        kind(view.block_at(target)) == Some(BlockKind::Glowstone)
    })
    .await;
    alice.sync().await;
    bob.sync().await;

    assert_eq!(
        server_block(&server, target).await,
        Some(block_state("minecraft:glowstone")),
        "the server stores the wand's block"
    );
    assert_eq!(server_block(&server, clicked).await, clicked_state);
    assert_eq!(kind(alice.block_at(target)), Some(BlockKind::Glowstone));
    assert!(
        chats(&alice).iter().all(|line| !line.contains("zap!")),
        "one click answers once, not once per packet: {:?}",
        chats(&alice)
    );
    assert!(
        chats(&bob).iter().all(|line| !line.contains("zap!")),
        "the reply goes to the clicking player only"
    );
    assert_eq!(
        server_slot(&server, entity, 36).await,
        stack("minecraft:stick", 1),
        "the wand is not consumed"
    );

    let carol = server.join("Carol").await;
    assert_eq!(
        kind(carol.block_at(target)),
        Some(BlockKind::Glowstone),
        "a late joiner gets the stored change"
    );

    for bot in [&alice, &bob, &carol] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_behavior_placing_a_block_overwrites_what_the_default_placement_would_not() {
    let server = TestServer::builder()
        .plugin(|app| {
            app.world_mut()
                .resource_mut::<ItemBehaviorRegistry>()
                .register_for("minecraft:stick", WandBehavior);
        })
        .start()
        .await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // A stone on the block the wand clicks, and the block Alice stands in:
    // the default placement refuses both, `place_block` forces them.
    let clicked = support(&alice);
    let occupied = clicked.up(1);
    let below_alice = ground(&alice);
    assert_eq!(
        below_alice.up(1),
        feet(&alice),
        "Alice stands on the ground"
    );
    let stone = block_state("minecraft:stone");
    server
        .with_world(move |world| {
            set_block_state(
                world,
                DimensionId::Overworld,
                to_server(occupied),
                stone,
                None,
            );
        })
        .await;
    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::Stick, 1))).await;
    alice.sync().await;

    use_on(
        &alice,
        InteractionHand::MainHand,
        hit(clicked, Direction::Up),
        1,
    )
    .await;
    use_on(
        &alice,
        InteractionHand::MainHand,
        hit(below_alice, Direction::Up),
        2,
    )
    .await;
    expect_ack(&alice, 1).await;
    expect_ack(&alice, 2).await;
    let glowstone = block_state("minecraft:glowstone");
    for target in [occupied, feet(&alice)] {
        bob.wait_until("the wand's glowstone", |view| {
            kind(view.block_at(target)) == Some(BlockKind::Glowstone)
        })
        .await;
        assert_eq!(server_block(&server, target).await, Some(glowstone));
    }

    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn the_behavior_sees_the_player_the_used_hand_and_the_clicked_face() {
    let calls = Calls::default();
    let recorded = calls.clone();
    let server = TestServer::builder()
        .plugin(move |app| {
            app.world_mut()
                .resource_mut::<ItemBehaviorRegistry>()
                .register_for(
                    "minecraft:stick",
                    Recorder {
                        calls: recorded,
                        on_block: UseResult::Handled,
                    },
                );
        })
        .start()
        .await;
    let alice = server.join("Alice").await;
    let player = server.player_entity(alice.uuid()).await;
    let client_id = client_id(&server, player).await;

    // Five sticks in the second hotbar slot, selected.
    hold(&alice, MAIN_HAND_SLOT + 1, Some((ItemKind::Stick, 5))).await;
    alice.send(ServerboundSetCarriedItem { slot: 1 }).await;
    alice.sync().await;
    alice.clear();

    let clicked = support(&alice);
    let block_hit = BlockHit {
        block_pos: clicked,
        direction: Direction::North,
        location: Vec3::new(
            f64::from(clicked.x) + 0.25,
            f64::from(clicked.y) + 0.75,
            f64::from(clicked.z),
        ),
        inside: false,
        world_border: false,
    };
    vanilla_click(&alice, block_hit.clone(), 1).await;
    for seq in 1..=3 {
        expect_ack(&alice, seq).await;
    }
    alice.sync().await;

    let sticks = stack("minecraft:stick", 5);
    assert_eq!(
        std::mem::take(&mut *calls.lock().unwrap()),
        [
            Call::UseOnBlock {
                player,
                client_id,
                hand: Hand::MainHand,
                held: sticks.clone(),
                position: to_server(clicked),
                face: BlockFace::North,
                cursor: (0.25, 0.75, 0.0),
                inside_block: false,
            },
            Call::Use {
                player,
                client_id,
                hand: Hand::MainHand,
                held: sticks,
            },
        ],
        "one hook call per packet that uses the stick; the empty off hand runs nothing"
    );
    let lines = chats(&alice);
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("recorder: on block"))
            .count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("recorder: in air"))
            .count(),
        1
    );
    alice.clear();

    // The stick moves to the off hand; the selected slot is empty.
    hold(&alice, MAIN_HAND_SLOT + 1, None).await;
    hold(&alice, OFF_HAND_SLOT, Some((ItemKind::Stick, 2))).await;
    use_on(&alice, InteractionHand::MainHand, block_hit.clone(), 4).await;
    use_on(&alice, InteractionHand::OffHand, block_hit, 5).await;
    expect_ack(&alice, 4).await;
    expect_ack(&alice, 5).await;
    alice.sync().await;
    assert_eq!(
        std::mem::take(&mut *calls.lock().unwrap()),
        [Call::UseOnBlock {
            player,
            client_id,
            hand: Hand::OffHand,
            held: stack("minecraft:stick", 2),
            position: to_server(clicked),
            face: BlockFace::North,
            cursor: (0.25, 0.75, 0.0),
            inside_block: false,
        }],
        "only the hand that holds the stick runs its behaviour"
    );

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn pass_falls_back_to_the_default_placement_and_handled_replaces_it() {
    let server = TestServer::builder()
        .plugin(|app| {
            let mut registry = app.world_mut().resource_mut::<ItemBehaviorRegistry>();
            registry.register_for(
                "minecraft:oak_planks",
                Answer("planks: pass", UseResult::Pass),
            );
            registry.register_for(
                "minecraft:cobblestone",
                Answer("cobblestone: handled", UseResult::Handled),
            );
        })
        .start()
        .await;
    let alice = server.join("Alice").await;
    let clicked = support(&alice);
    let planks = clicked.up(1);
    let above = clicked.up(2);
    assert_eq!(server_block(&server, planks).await, Some(0));
    assert_eq!(server_block(&server, above).await, Some(0));

    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::OakPlanks, 1))).await;
    use_on(
        &alice,
        InteractionHand::MainHand,
        hit(clicked, Direction::Up),
        1,
    )
    .await;
    expect_ack(&alice, 1).await;
    expect_chat(&alice, "planks: pass").await;
    alice
        .wait_until("the default placement of the planks", |view| {
            kind(view.block_at(planks)) == Some(BlockKind::OakPlanks)
        })
        .await;
    assert_eq!(
        server_block(&server, planks).await,
        Some(block_state("minecraft:oak_planks"))
    );

    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::Cobblestone, 1))).await;
    alice.sync().await;
    alice.clear();
    use_on(
        &alice,
        InteractionHand::MainHand,
        hit(planks, Direction::Up),
        2,
    )
    .await;
    expect_ack(&alice, 2).await;
    expect_chat(&alice, "cobblestone: handled").await;
    alice.sync().await;
    assert_eq!(
        server_block(&server, above).await,
        Some(0),
        "a handled use does not also place the block"
    );
    assert!(updates_at(&alice, above).is_empty());

    alice.assert_healthy();
    server.stop().await;
}

/// Uses one item from the stack in the hand per right-click in the air.
struct Consume;

impl ItemBehavior for Consume {
    fn on_use(&self, ctx: &mut ItemUseContext) -> UseResult {
        ctx.consume(1);
        ctx.reply(&format!("consumed, {} left", ctx.held.count - 1));
        UseResult::Handled
    }
}

#[tokio::test]
async fn a_use_in_the_air_can_consume_the_held_item() {
    let server = TestServer::builder()
        .plugin(|app| {
            app.world_mut()
                .resource_mut::<ItemBehaviorRegistry>()
                .register_for("minecraft:stick", Consume);
        })
        .start()
        .await;
    let alice = server.join("Alice").await;
    let entity = server.player_entity(alice.uuid()).await;

    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::Stick, 2))).await;
    use_in_air(&alice, InteractionHand::MainHand, 1).await;
    expect_ack(&alice, 1).await;
    expect_chat(&alice, "consumed, 1 left").await;
    alice
        .wait_until("one stick left", |view| {
            view.inventory[usize::from(MAIN_HAND_SLOT)]
                == ItemStack::Present(ItemStackData::new(ItemKind::Stick, 1))
        })
        .await;

    use_in_air(&alice, InteractionHand::MainHand, 2).await;
    expect_ack(&alice, 2).await;
    expect_chat(&alice, "consumed, 0 left").await;
    alice
        .wait_until("no stick left", |view| {
            view.inventory[usize::from(MAIN_HAND_SLOT)] == ItemStack::Empty
        })
        .await;
    assert!(server_slot(&server, entity, 36).await.is_empty());

    // The hand is empty now: nothing runs, the use is still acknowledged.
    alice.clear();
    use_in_air(&alice, InteractionHand::MainHand, 3).await;
    expect_ack(&alice, 3).await;
    alice.sync().await;
    assert!(chats(&alice).is_empty());

    alice.assert_healthy();
    server.stop().await;
}

/// Keeps every block a player starts breaking with it.
struct Keep;

impl ItemBehavior for Keep {
    fn before_break_block(&self, ctx: &mut BlockBreakContext) -> UseResult {
        ctx.reply("keep: kept");
        UseResult::Handled
    }
}

/// Lets the block break, then reports it.
struct Report;

impl ItemBehavior for Report {
    fn on_break_block(&self, ctx: &mut BlockBreakContext) -> UseResult {
        let still_there = ctx.with_world(|world| {
            let chunk =
                ServerChunkPos::new(ctx.position.x.div_euclid(16), ctx.position.z.div_euclid(16));
            let entity = world.resource::<ChunkIndex>().0[&(DimensionId::Overworld, chunk)];
            world.get::<ChunkData>(entity).unwrap().get_block(
                ctx.position.x.rem_euclid(16) as u8,
                i32::from(ctx.position.y),
                ctx.position.z.rem_euclid(16) as u8,
            )
        });
        ctx.reply(&format!(
            "report: broke {} with {} now {:?}",
            ctx.broken_state,
            ctx.held.item.name().unwrap_or("?"),
            still_there
        ));
        UseResult::Pass
    }
}

#[tokio::test]
async fn break_hooks_can_keep_a_block_or_follow_its_removal() {
    let server = TestServer::builder()
        .plugin(|app| {
            let mut registry = app.world_mut().resource_mut::<ItemBehaviorRegistry>();
            registry.register_for("minecraft:stick", Keep);
            registry.register_for("minecraft:blaze_rod", Report);
        })
        .start()
        .await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let target = support(&alice);
    let original = server_block(&server, target).await.unwrap();
    assert_ne!(original, 0);

    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::Stick, 1))).await;
    alice.sync().await;
    alice.clear();
    bob.clear();
    start_breaking(&alice, target, 1).await;
    expect_ack(&alice, 1).await;
    expect_chat(&alice, "keep: kept").await;
    alice.sync().await;
    bob.sync().await;
    assert_eq!(server_block(&server, target).await, Some(original));
    assert_eq!(
        updates_at(&alice, target)
            .into_iter()
            .map(|state| u32::from(state) as i32)
            .collect::<Vec<_>>(),
        [original],
        "the breaker's predicted break is undone"
    );
    assert!(
        updates_at(&bob, target).is_empty(),
        "nothing changed for others"
    );

    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::BlazeRod, 1))).await;
    start_breaking(&alice, target, 2).await;
    expect_ack(&alice, 2).await;
    expect_chat(
        &alice,
        &format!("report: broke {original} with minecraft:blaze_rod now Some(0)"),
    )
    .await;
    bob.wait_until("the broken block", |view| {
        view.block_at(target).is_some_and(|state| state.is_air())
    })
    .await;
    assert_eq!(server_block(&server, target).await, Some(0));

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

/// The world, chat and hook calls that a stone, a diamond and a stick used on
/// blocks produce, on a server with or without a behaviour for the stick.
async fn default_interactions(stick_behavior: Option<Calls>) -> (Vec<Option<i32>>, Vec<String>) {
    let server = TestServer::builder()
        .plugin(move |app| {
            if let Some(calls) = stick_behavior {
                app.world_mut()
                    .resource_mut::<ItemBehaviorRegistry>()
                    .register_for(
                        "minecraft:stick",
                        Recorder {
                            calls,
                            on_block: UseResult::Handled,
                        },
                    );
            }
        })
        .start()
        .await;
    let alice = server.join("Alice").await;
    let clicked = support(&alice);
    let targets = [clicked.up(1), clicked.up(2)];
    for target in targets {
        assert_eq!(server_block(&server, target).await, Some(0));
    }
    alice.sync().await;
    alice.clear();

    for (seq, (item, clicked, face)) in [
        (ItemKind::Stone, clicked, Direction::Up),
        (ItemKind::Diamond, targets[0], Direction::Up),
        (ItemKind::Stick, targets[0], Direction::Up),
    ]
    .into_iter()
    .enumerate()
    {
        hold(&alice, MAIN_HAND_SLOT, Some((item, 1))).await;
        let seq = seq as u32 + 1;
        use_on(&alice, InteractionHand::MainHand, hit(clicked, face), seq).await;
        expect_ack(&alice, seq).await;
    }
    alice.sync().await;

    let mut blocks = Vec::new();
    for target in targets {
        let stored = server_block(&server, target).await;
        assert_eq!(
            alice.block_at(target).map(|state| u32::from(state) as i32),
            stored,
            "the client and the server agree at {target:?}"
        );
        blocks.push(stored);
    }
    let lines = chats(&alice);
    alice.assert_healthy();
    server.stop().await;
    (blocks, lines)
}

#[tokio::test]
async fn unrelated_items_behave_as_without_the_registration() {
    let (without, without_chat) = default_interactions(None).await;
    let calls = Calls::default();
    let (with, with_chat) = default_interactions(Some(calls.clone())).await;

    assert_eq!(
        without,
        [Some(block_state("minecraft:stone")), Some(0)],
        "stone is placed; a diamond and an unregistered stick do nothing"
    );
    assert!(without_chat.is_empty(), "{without_chat:?}");
    assert_eq!(with, without, "the registration changes no other item");
    assert_eq!(with_chat.len(), 1, "only the stick answers: {with_chat:?}");
    assert!(with_chat[0].contains("recorder: on block"));
    let calls = calls.lock().unwrap();
    assert!(
        matches!(
            calls.as_slice(),
            [Call::UseOnBlock { held, face: BlockFace::Top, .. }]
                if *held == stack("minecraft:stick", 1)
        ),
        "the behaviour runs for the stick only: {calls:?}"
    );
}

#[tokio::test]
async fn a_second_registration_replaces_the_first_and_an_unknown_item_is_reported() {
    let server = TestServer::builder()
        .plugin(|app| {
            let mut registry = app.world_mut().resource_mut::<ItemBehaviorRegistry>();
            registry.register_for("minecraft:stick", Answer("first", UseResult::Handled));
            registry.register_for("minecraft:stick", Answer("second", UseResult::Handled));
            registry.register_for(
                "minecraft:not_an_item",
                Answer("unknown", UseResult::Handled),
            );
        })
        .start()
        .await;
    let warning = server
        .logs()
        .into_iter()
        .find(|record| record.message.contains("minecraft:not_an_item"))
        .expect("the unknown item is reported");
    assert_eq!(warning.level, tracing::Level::WARN);
    assert!(warning.target.starts_with("voidmc"));

    let alice = server.join("Alice").await;
    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::Stick, 1))).await;
    alice.sync().await;
    alice.clear();
    use_on(
        &alice,
        InteractionHand::MainHand,
        hit(support(&alice), Direction::Up),
        1,
    )
    .await;
    expect_ack(&alice, 1).await;
    expect_chat(&alice, "second").await;
    alice.sync().await;
    assert!(
        chats(&alice).is_empty(),
        "the replaced behaviour does not run"
    );

    alice.assert_healthy();
    server.stop().await;
}

/// Tries world changes that cannot happen and reports what they returned.
struct Unreachable;

impl ItemBehavior for Unreachable {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        let clicked = ctx.block.unwrap().position;
        let glowstone = block_state("minecraft:glowstone");
        let above = ctx.set_block(BlockPosition { y: 2000, ..clicked }, glowstone);
        let below = ctx.set_block(
            BlockPosition {
                y: -2000,
                ..clicked
            },
            glowstone,
        );
        let unloaded = ctx.set_block(
            BlockPosition {
                x: clicked.x + 100_000,
                ..clicked
            },
            glowstone,
        );
        ctx.reply(&format!(
            "unreachable: above {above}, below {below}, unloaded {unloaded}"
        ));
        UseResult::Handled
    }

    fn on_use(&self, ctx: &mut ItemUseContext) -> UseResult {
        let placed = ctx.place_block(block_state("minecraft:glowstone"));
        ctx.reply(&format!("unreachable: in air {placed}"));
        UseResult::Handled
    }
}

#[tokio::test]
async fn impossible_changes_and_bursts_of_uses_leave_the_queue_working() {
    let server = TestServer::builder()
        .plugin(|app| {
            let mut registry = app.world_mut().resource_mut::<ItemBehaviorRegistry>();
            registry.register_for("minecraft:blaze_rod", Unreachable);
            registry.register_for("minecraft:stick", WandBehavior);
        })
        .start()
        .await;
    let alice = server.join("Alice").await;
    let clicked = support(&alice);

    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::BlazeRod, 1))).await;
    use_on(
        &alice,
        InteractionHand::MainHand,
        hit(clicked, Direction::Up),
        1,
    )
    .await;
    use_in_air(&alice, InteractionHand::MainHand, 2).await;
    expect_ack(&alice, 1).await;
    expect_ack(&alice, 2).await;
    expect_chat(
        &alice,
        "unreachable: above false, below false, unloaded false",
    )
    .await;
    expect_chat(&alice, "unreachable: in air false").await;

    // Three clicks in a burst, each on the block the previous one placed.
    hold(&alice, MAIN_HAND_SLOT, Some((ItemKind::Stick, 1))).await;
    for seq in 3..=5 {
        let below = clicked.up(seq as i32 - 3);
        use_on(
            &alice,
            InteractionHand::MainHand,
            hit(below, Direction::Up),
            seq,
        )
        .await;
    }
    for seq in 3..=5 {
        expect_ack(&alice, seq).await;
        expect_chat(&alice, "zap! placed glowstone").await;
    }
    alice.sync().await;
    for height in 1..=3 {
        assert_eq!(
            server_block(&server, clicked.up(height)).await,
            Some(block_state("minecraft:glowstone"))
        );
        assert_eq!(
            kind(alice.block_at(clicked.up(height))),
            Some(BlockKind::Glowstone)
        );
    }
    assert!(chats(&alice).iter().all(|line| !line.contains("zap!")));

    alice.assert_healthy();
    server.stop().await;
}
