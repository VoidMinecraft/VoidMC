//! VMC-ATP-014: every click mode on the player inventory (window 0).
//!
//! Each click is sent by the bot the way a vanilla client sends it
//! ([`Bot::click`]): predicted with `voidmc_e2e::clicks`, an independent
//! transcription of vanilla's `AbstractContainerMenu.doClick` and
//! `InventoryMenu.quickMoveStack` for window 0, shown in the bot's view at
//! once, and sent with the hashed contents it now expects in every slot it
//! changed, and its cursor. The test runs the same rules on its own
//! [`InventoryMenu`] as the state the server must reach. After each action
//! it checks that
//!
//! - the server's `Inventory` (46 slots and cursor) is what vanilla computes,
//! - the bot's view (its own prediction, then the server's corrections) ends
//!   up equal to the server,
//! - every state id the server sends is the client's last one plus one,
//! - each item's total over the slots, the cursor and the stacks dropped into
//!   the world never changes except through `/give` and `/clear`, and no
//!   stack exceeds its max size.
//!
//! Armor slots only take an item whose `equippable` slot is theirs, one at a
//! time, and a shift-click sends armor and shields to their empty slot, as in
//! vanilla. The rules read each item's equipment slot and max stack size from
//! Azalea's default components, a source independent from VoidMC's data.
//! Another player is shown what the clicks put into the hands and armor
//! slots.

use std::collections::BTreeMap;

use bevy_ecs::prelude::Entity;
use voidmc::components::ItemEntity;
use voidmc::events::EntityDespawnEvent;
use voidmc::{Inventory, ItemId, ItemStack as ServerItemStack};
use voidmc_e2e::clicks::{
    InventoryMenu, OFFHAND, OUTSIDE, RESULT, SIZE, Stack, from_client, hashed, max_stack, stack,
};
use voidmc_e2e::inventory::ItemStack;
use voidmc_e2e::inventory::components::EquipmentSlot;
use voidmc_e2e::inventory::operations::ClickType;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::protocol::packets::game::s_container_click::ServerboundContainerClick;
use voidmc_e2e::protocol::packets::game::s_container_close::ServerboundContainerClose;
use voidmc_e2e::protocol::packets::game::s_set_creative_mode_slot::ServerboundSetCreativeModeSlot;
use voidmc_e2e::registry::Registry;
use voidmc_e2e::registry::builtin::ItemKind;
use voidmc_e2e::{Bot, TestServer};

use crate::wait_for_equipment;

fn from_server(item: &ServerItemStack) -> Stack {
    if item.is_empty() {
        return None;
    }
    let kind = ItemKind::from_u32(item.item.0 as u32).expect("a known item");
    Some((kind, i32::from(item.count)))
}

fn to_server(stack: Stack) -> ServerItemStack {
    match stack {
        None => ServerItemStack::EMPTY,
        Some((kind, count)) => ServerItemStack::new(ItemId(kind.to_u32() as i32), count as u8),
    }
}

/// An inventory packet as the client applies it. The bot's view applies it;
/// the contents are kept for the failure messages.
#[derive(Debug)]
#[allow(dead_code)]
enum Update {
    Slot(u32, usize, Stack),
    Content(u32, Vec<Stack>, Stack),
    Cursor(Stack),
}

fn inventory_update(packet: &ClientboundGamePacket) -> Option<Update> {
    match packet {
        ClientboundGamePacket::ContainerSetSlot(p) if p.container_id == 0 => Some(Update::Slot(
            p.state_id,
            usize::from(p.slot),
            from_client(&p.item_stack),
        )),
        ClientboundGamePacket::ContainerSetContent(p) if p.container_id == 0 => {
            Some(Update::Content(
                p.state_id,
                p.items.iter().map(from_client).collect(),
                from_client(&p.carried_item),
            ))
        }
        ClientboundGamePacket::SetCursorItem(p) => Some(Update::Cursor(from_client(&p.contents))),
        _ => None,
    }
}

/// What the server holds after an action, plus the stacks it dropped.
struct ServerState {
    slots: Vec<Stack>,
    cursor: Stack,
    dropped: Vec<(ItemKind, i32)>,
}

/// One bot on a server, the state the server must reach (`model`) and the
/// expected item totals. What the client shows is the bot's view.
struct Session<'a> {
    server: &'a TestServer,
    bot: Bot,
    entity: Entity,
    model: InventoryMenu,
    state_id: u32,
    totals: BTreeMap<ItemKind, i32>,
    dropped: BTreeMap<ItemKind, i32>,
    /// Inventory packets received during the last `settle`.
    updates: Vec<Update>,
}

impl<'a> Session<'a> {
    async fn join(server: &'a TestServer, creative: bool) -> Self {
        let bot = server.join("Alice").await;
        let entity = server.player_entity(bot.uuid()).await;
        Self {
            server,
            bot,
            entity,
            model: InventoryMenu::new(creative),
            state_id: 0,
            totals: BTreeMap::new(),
            dropped: BTreeMap::new(),
            updates: Vec::new(),
        }
    }

    /// Replaces the whole inventory server-side and lets the client catch up.
    async fn setup(&mut self, slots: &[(usize, ItemKind, i32)], cursor: Stack) {
        let entity = self.entity;
        let stacks: Vec<(usize, ServerItemStack)> = slots
            .iter()
            .map(|&(slot, kind, count)| (slot, to_server(stack(kind, count))))
            .collect();
        let carried = to_server(cursor);
        self.server
            .with_world(move |world| {
                let mut inventory = world.get_mut::<Inventory>(entity).unwrap();
                inventory.clear();
                for (slot, stack) in stacks {
                    inventory.set(slot, stack);
                }
                inventory.set_cursor(carried);
            })
            .await;
        self.bot.sync().await;
        let (inventory, cursor) = self.client();
        self.state_id = self.bot.view(|view| view.inventory_state_id);
        self.bot.clear();
        self.model.slots = inventory.clone();
        self.model.carried = cursor;
        self.model.quickcraft_status = 0;
        self.model.quickcraft_slots.clear();
        self.model.drops.clear();
        self.totals.clear();
        self.dropped.clear();
        for (kind, count) in inventory.iter().chain([&cursor]).flatten() {
            *self.totals.entry(*kind).or_default() += count;
        }
        self.check().await;
    }

    /// What the bot's view shows: the 46 slots and the cursor.
    fn client(&self) -> (Vec<Stack>, Stack) {
        self.bot.view(|view| {
            (
                view.inventory.iter().map(from_client).collect(),
                from_client(&view.cursor),
            )
        })
    }

    /// Clicks the way a vanilla client does ([`Bot::click`]), without
    /// waiting, and applies the click to the expected server state.
    async fn send(&mut self, slot: i16, button: u8, mode: ClickType) {
        self.send_with(slot, button, mode, self.state_id, |_, _| {})
            .await;
    }

    /// Clicks with the state id `state_id` and a prediction that `forge`
    /// rewrites after vanilla's rules made it; the bot's view shows the forged
    /// prediction. Returns the prediction the rules made, before `forge`.
    async fn send_with(
        &mut self,
        slot: i16,
        button: u8,
        mode: ClickType,
        state_id: u32,
        forge: impl FnOnce(&mut Vec<(u16, Stack)>, &mut Stack),
    ) -> (Vec<(u16, Stack)>, Stack) {
        self.model.click(slot, button, mode);
        let mut predicted = None;
        self.bot
            .click_with(slot, button, mode, |click| {
                predicted = Some((click.changed.clone(), click.carried));
                click.state_id = state_id;
                forge(&mut click.changed, &mut click.carried);
            })
            .await;
        predicted.expect("the click was predicted")
    }

    /// One click, then every check. The previous action was settled, so the
    /// bot predicted from what the server holds: its prediction is what the
    /// rules compute on the expected server state.
    async fn click(&mut self, slot: i16, button: u8, mode: ClickType) {
        let before = self.model.clone();
        let (changed, carried) = self
            .send_with(slot, button, mode, self.state_id, |_, _| {})
            .await;
        let expected: Vec<(u16, Stack)> = (0..SIZE)
            .filter(|&i| before.slots[i] != self.model.slots[i])
            .map(|i| (i as u16, self.model.slots[i]))
            .collect();
        assert_eq!(
            (changed, carried),
            (expected, self.model.carried),
            "the bot predicted another result than the rules on the server's state"
        );
        self.settle().await;
        assert!(
            self.updates.is_empty(),
            "a correct prediction was answered with {:?}",
            self.updates
        );
    }

    /// Waits for the server to handle everything sent, takes its inventory
    /// packets (the bot's view has applied them), and checks every invariant.
    async fn settle(&mut self) {
        self.bot.sync().await;
        self.updates.clear();
        while !self.bot.queued(inventory_update).is_empty() {
            let update = self
                .bot
                .expect("an inventory update", inventory_update)
                .await;
            match &update {
                Update::Slot(state_id, ..) | Update::Content(state_id, ..) => {
                    self.advance_state_id(*state_id);
                }
                Update::Cursor(_) => {}
            }
            self.updates.push(update);
        }
        self.check().await;
    }

    fn advance_state_id(&mut self, state_id: u32) {
        assert_eq!(
            state_id,
            (self.state_id + 1) & 32767,
            "state ids must advance one by one"
        );
        self.state_id = state_id;
    }

    async fn server_state(&self) -> ServerState {
        let entity = self.entity;
        self.server
            .with_world(move |world| {
                let inventory = world.get::<Inventory>(entity).unwrap();
                let slots = (0..SIZE).map(|i| from_server(inventory.get(i))).collect();
                let cursor = from_server(inventory.cursor());
                let mut items = world.query::<(Entity, &ItemEntity)>();
                let drops: Vec<(Entity, Stack)> = items
                    .iter(world)
                    .map(|(entity, item)| (entity, from_server(&item.stack)))
                    .collect();
                for (entity, _) in &drops {
                    world.trigger(EntityDespawnEvent { entity: *entity });
                }
                world.flush();
                ServerState {
                    slots,
                    cursor,
                    dropped: drops.into_iter().filter_map(|(_, stack)| stack).collect(),
                }
            })
            .await
    }

    async fn check(&mut self) {
        let state = self.server_state().await;
        let mut expected_drops = std::mem::take(&mut self.model.drops);
        let mut dropped = state.dropped.clone();
        expected_drops.sort();
        dropped.sort();
        assert_eq!(dropped, expected_drops, "items dropped into the world");
        for (kind, count) in &state.dropped {
            *self.dropped.entry(*kind).or_default() += count;
        }

        let (client, client_cursor) = self.client();
        for (i, shown) in client.iter().enumerate() {
            assert_eq!(
                state.slots[i], self.model.slots[i],
                "server slot {i} vs vanilla"
            );
            assert_eq!(*shown, state.slots[i], "client slot {i} vs server");
        }
        assert_eq!(state.cursor, self.model.carried, "server cursor vs vanilla");
        assert_eq!(client_cursor, state.cursor, "client cursor vs server");

        let mut held: BTreeMap<ItemKind, i32> = BTreeMap::new();
        for (kind, count) in state.slots.iter().chain([&state.cursor]).flatten() {
            assert!(
                *count <= max_stack(*kind),
                "a stack of {count} {kind:?} exceeds its max size"
            );
            *held.entry(*kind).or_default() += count;
        }
        for (kind, count) in &self.dropped {
            *held.entry(*kind).or_default() += count;
        }
        held.retain(|_, count| *count != 0);
        let mut totals = self.totals.clone();
        totals.retain(|_, count| *count != 0);
        assert_eq!(held, totals, "items were created or destroyed");
    }

    /// The server state, after resynchronizing the model to it (for actions
    /// whose exact result depends on timing, like an interleaved command).
    async fn adopt_server_state(&mut self) {
        let state = self.server_state().await;
        self.model.slots = state.slots;
        self.model.carried = state.cursor;
        self.model.quickcraft_status = 0;
        self.model.quickcraft_slots.clear();
        for (kind, count) in state.dropped {
            *self.dropped.entry(kind).or_default() += count;
        }
    }

    fn finish(self) {
        self.bot.assert_healthy();
    }

    fn slot(&self, index: usize) -> Stack {
        self.model.slots[index]
    }

    fn cursor(&self) -> Stack {
        self.model.carried
    }
}

async fn survival() -> TestServer {
    TestServer::builder()
        .operators()
        .config(|config| config.game_mode(0))
        .start()
        .await
}

async fn creative() -> TestServer {
    TestServer::builder()
        .config(|config| config.game_mode(1))
        .start()
        .await
}

use ClickType::{Clone as CloneClick, Pickup, PickupAll, QuickCraft, QuickMove, Swap, Throw};
use EquipmentSlot::{Chest, Feet, Head, Mainhand, Offhand};
use ItemKind::{
    CarvedPumpkin, Diamond, DiamondBoots, DiamondChestplate, DiamondHelmet, DiamondSword, Dirt,
    EnderPearl, Shield, Stone,
};

#[tokio::test]
async fn left_and_right_clicks_pick_up_place_split_merge_and_drop() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    s.setup(
        &[(36, Diamond, 20), (37, EnderPearl, 7), (9, Diamond, 50)],
        None,
    )
    .await;

    s.click(36, 0, Pickup).await;
    assert_eq!((s.slot(36), s.cursor()), (None, Some((Diamond, 20))));
    s.click(12, 0, Pickup).await;
    assert_eq!((s.slot(12), s.cursor()), (Some((Diamond, 20)), None));
    s.click(12, 1, Pickup).await;
    assert_eq!(
        (s.slot(12), s.cursor()),
        (Some((Diamond, 10)), Some((Diamond, 10)))
    );
    s.click(13, 1, Pickup).await;
    s.click(13, 1, Pickup).await;
    assert_eq!(
        (s.slot(13), s.cursor()),
        (Some((Diamond, 2)), Some((Diamond, 8)))
    );
    s.click(9, 0, Pickup).await;
    assert_eq!((s.slot(9), s.cursor()), (Some((Diamond, 58)), None));
    s.click(12, 1, Pickup).await;
    s.click(9, 0, Pickup).await;
    s.click(13, 0, Pickup).await;
    s.click(9, 0, Pickup).await;
    assert_eq!(
        (s.slot(9), s.cursor()),
        (Some((Diamond, 64)), Some((Diamond, 1)))
    );

    // A full stack takes nothing; a different item swaps with the cursor.
    s.click(12, 0, Pickup).await;
    s.click(12, 0, Pickup).await;
    assert_eq!((s.slot(12), s.cursor()), (None, Some((Diamond, 6))));
    s.click(9, 1, Pickup).await;
    s.click(9, 0, Pickup).await;
    assert_eq!(
        (s.slot(9), s.cursor()),
        (Some((Diamond, 64)), Some((Diamond, 6)))
    );
    s.click(37, 0, Pickup).await;
    assert_eq!(
        (s.slot(37), s.cursor()),
        (Some((Diamond, 6)), Some((EnderPearl, 7)))
    );
    s.click(37, 1, Pickup).await;
    assert_eq!(
        (s.slot(37), s.cursor()),
        (Some((EnderPearl, 7)), Some((Diamond, 6)))
    );

    // Pearls stop at 16, the remainder stays on the cursor.
    s.setup(&[(9, EnderPearl, 10), (10, EnderPearl, 12)], None)
        .await;
    s.click(10, 0, Pickup).await;
    s.click(9, 0, Pickup).await;
    assert_eq!(
        (s.slot(9), s.cursor()),
        (Some((EnderPearl, 16)), Some((EnderPearl, 6)))
    );

    // Outside the window: right drops one, left the rest.
    s.click(OUTSIDE, 1, Pickup).await;
    assert_eq!(s.cursor(), Some((EnderPearl, 5)));
    s.click(OUTSIDE, 0, Pickup).await;
    assert_eq!(s.cursor(), None);
    assert_eq!(s.dropped[&EnderPearl], 6);

    // Empty cursor on an empty slot or outside: nothing, whatever the button.
    s.click(20, 0, Pickup).await;
    s.click(20, 1, Pickup).await;
    s.click(OUTSIDE, 0, Pickup).await;
    s.click(OUTSIDE, 1, Pickup).await;
    assert_eq!((s.slot(20), s.cursor()), (None, None));

    // The crafting output takes nothing, and only gives a whole stack.
    s.setup(&[(9, Stone, 20), (RESULT, Stone, 50)], None).await;
    s.click(9, 0, Pickup).await;
    s.click(RESULT as i16, 0, Pickup).await;
    s.click(RESULT as i16, 1, Pickup).await;
    assert_eq!(
        (s.slot(RESULT), s.cursor()),
        (Some((Stone, 50)), Some((Stone, 20)))
    );
    s.click(9, 0, Pickup).await;
    s.click(9, 1, Pickup).await;
    s.click(RESULT as i16, 0, Pickup).await;
    assert_eq!((s.slot(RESULT), s.cursor()), (None, Some((Stone, 60))));
    s.click(RESULT as i16, 0, Pickup).await;
    s.click(RESULT as i16, 1, Pickup).await;
    assert_eq!((s.slot(RESULT), s.cursor()), (None, Some((Stone, 60))));

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn shift_clicks_merge_first_then_fill_and_keep_the_remainder() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    let mut slots: Vec<(usize, ItemKind, i32)> = (11..36).map(|i| (i, Stone, 64)).collect();
    slots.extend([
        (9, Dirt, 60),
        (10, Stone, 30),
        (36, Dirt, 40),
        (38, Stone, 10),
    ]);
    s.setup(&slots, None).await;

    // Hotbar to main: 4 merge, the rest stays for want of room.
    s.click(36, 0, QuickMove).await;
    assert_eq!(
        (s.slot(9), s.slot(36)),
        (Some((Dirt, 64)), Some((Dirt, 36)))
    );
    // Main to hotbar: the partial stack fills first, then the first empty slot.
    s.click(11, 0, QuickMove).await;
    assert_eq!(
        (s.slot(11), s.slot(37), s.slot(38)),
        (None, Some((Stone, 10)), Some((Stone, 64)))
    );
    // Back up: the main inventory has room again in slot 11 only.
    s.click(37, 1, QuickMove).await;
    assert_eq!(
        (s.slot(10), s.slot(11), s.slot(37)),
        (Some((Stone, 40)), None, None)
    );

    // Crafting grid, armor and off-hand slots go to the inventory; the
    // crafting output fills it from the end.
    s.setup(
        &[
            (1, Diamond, 3),
            (5, DiamondHelmet, 1),
            (OFFHAND, EnderPearl, 4),
            (RESULT, Stone, 5),
        ],
        None,
    )
    .await;
    s.click(1, 0, QuickMove).await;
    s.click(5, 0, QuickMove).await;
    s.click(OFFHAND as i16, 0, QuickMove).await;
    s.click(RESULT as i16, 0, QuickMove).await;
    assert_eq!(
        (s.slot(9), s.slot(10), s.slot(11), s.slot(44)),
        (
            Some((Diamond, 3)),
            Some((DiamondHelmet, 1)),
            Some((EnderPearl, 4)),
            Some((Stone, 5))
        )
    );
    // Nothing to move, or outside the window with an empty cursor.
    s.click(20, 0, QuickMove).await;
    s.click(OUTSIDE, 0, QuickMove).await;

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn number_keys_and_the_offhand_key_swap_with_the_hovered_slot() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    s.setup(&[(9, Diamond, 5), (39, Dirt, 7), (OFFHAND, Stone, 3)], None)
        .await;

    s.click(9, 3, Swap).await;
    assert_eq!(
        (s.slot(9), s.slot(39)),
        (Some((Dirt, 7)), Some((Diamond, 5)))
    );
    s.click(10, 3, Swap).await;
    assert_eq!((s.slot(10), s.slot(39)), (Some((Diamond, 5)), None));
    s.click(11, 4, Swap).await;
    assert_eq!((s.slot(11), s.slot(40)), (None, None));
    s.click(9, 40, Swap).await;
    assert_eq!(
        (s.slot(9), s.slot(OFFHAND)),
        (Some((Stone, 3)), Some((Dirt, 7)))
    );
    s.click(OFFHAND as i16, 40, Swap).await;
    s.click(40, 4, Swap).await;
    // Unknown keys and the crafting output change nothing.
    for button in [9, 39, 41, 255] {
        s.click(9, button, Swap).await;
    }
    s.click(RESULT as i16, 0, Swap).await;
    assert_eq!(s.slot(RESULT), None);

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn drags_spread_the_cursor_and_broken_drags_never_duplicate() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    s.setup(&[(9, Diamond, 10), (12, Diamond, 64), (13, Dirt, 1)], None)
        .await;
    s.click(9, 0, Pickup).await;

    // Left drag over 3 slots, one of them a full stack, plus a foreign item
    // (ignored) and a repeated slot: 10 / 3 = 3 each, 1 stays.
    s.click(OUTSIDE, 0, QuickCraft).await;
    for slot in [10, 11, 13, 11, 12] {
        s.click(slot, 1, QuickCraft).await;
    }
    s.click(OUTSIDE, 2, QuickCraft).await;
    assert_eq!(
        (s.slot(10), s.slot(11), s.slot(12), s.cursor()),
        (
            Some((Diamond, 3)),
            Some((Diamond, 3)),
            Some((Diamond, 64)),
            Some((Diamond, 4))
        )
    );

    // Right drag: one per slot.
    s.click(OUTSIDE, 4, QuickCraft).await;
    for slot in [20, 21, 10] {
        s.click(slot, 5, QuickCraft).await;
    }
    s.click(OUTSIDE, 6, QuickCraft).await;
    assert_eq!(
        (s.slot(20), s.slot(21), s.slot(10), s.cursor()),
        (
            Some((Diamond, 1)),
            Some((Diamond, 1)),
            Some((Diamond, 4)),
            Some((Diamond, 1))
        )
    );

    // More slots than items: the extra slots are not collected.
    s.setup(&[(9, Dirt, 2)], None).await;
    s.click(9, 0, Pickup).await;
    s.click(OUTSIDE, 0, QuickCraft).await;
    for slot in [20, 21, 22] {
        s.click(slot, 1, QuickCraft).await;
    }
    s.click(OUTSIDE, 2, QuickCraft).await;
    assert_eq!(
        (s.slot(20), s.slot(21), s.slot(22), s.cursor()),
        (Some((Dirt, 1)), Some((Dirt, 1)), None, None)
    );

    // A single slot is a plain click; a middle drag needs creative.
    s.setup(&[(9, Dirt, 9)], None).await;
    s.click(9, 0, Pickup).await;
    s.click(OUTSIDE, 4, QuickCraft).await;
    s.click(30, 5, QuickCraft).await;
    s.click(OUTSIDE, 6, QuickCraft).await;
    assert_eq!((s.slot(30), s.cursor()), (Some((Dirt, 1)), Some((Dirt, 8))));
    s.click(OUTSIDE, 8, QuickCraft).await;
    s.click(31, 9, QuickCraft).await;
    s.click(32, 9, QuickCraft).await;
    s.click(OUTSIDE, 10, QuickCraft).await;
    assert_eq!((s.slot(31), s.slot(32)), (None, None));

    // Broken sequences: add or end without a start, a second start, an
    // unknown header, and a pickup in the middle, which only cancels.
    s.click(20, 1, QuickCraft).await;
    s.click(OUTSIDE, 2, QuickCraft).await;
    s.click(OUTSIDE, 0, QuickCraft).await;
    s.click(20, 1, QuickCraft).await;
    s.click(OUTSIDE, 0, QuickCraft).await;
    s.click(21, 1, QuickCraft).await;
    s.click(OUTSIDE, 2, QuickCraft).await;
    s.click(OUTSIDE, 0, QuickCraft).await;
    s.click(22, 3, QuickCraft).await;
    s.click(OUTSIDE, 2, QuickCraft).await;
    s.click(OUTSIDE, 0, QuickCraft).await;
    s.click(23, 1, QuickCraft).await;
    s.click(24, 1, QuickCraft).await;
    s.click(25, 0, Pickup).await;
    s.click(OUTSIDE, 2, QuickCraft).await;
    assert_eq!(
        (s.slot(20), s.slot(23), s.slot(25), s.cursor()),
        (None, None, None, Some((Dirt, 8)))
    );
    // An empty cursor cancels a drag.
    s.click(26, 0, Pickup).await;
    s.click(OUTSIDE, 0, QuickCraft).await;
    s.click(27, 1, QuickCraft).await;
    s.click(OUTSIDE, 2, QuickCraft).await;
    assert_eq!(s.slot(27), None);

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn double_click_collects_partial_stacks_before_full_ones() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    s.setup(
        &[
            (9, Stone, 64),
            (10, Stone, 10),
            (11, Stone, 5),
            (40, Stone, 30),
            (20, Dirt, 3),
            (RESULT, Stone, 4),
        ],
        None,
    )
    .await;
    s.click(11, 0, Pickup).await;
    s.click(11, 0, PickupAll).await;
    assert_eq!(
        (
            s.slot(9),
            s.slot(10),
            s.slot(40),
            s.slot(RESULT),
            s.cursor()
        ),
        (
            Some((Stone, 45)),
            None,
            None,
            Some((Stone, 4)),
            Some((Stone, 64))
        )
    );

    // Only over an empty slot, and button 1 scans from the end.
    s.setup(
        &[(9, Dirt, 10), (10, Dirt, 20), (44, Dirt, 50), (12, Dirt, 2)],
        None,
    )
    .await;
    s.click(12, 0, Pickup).await;
    s.click(9, 0, PickupAll).await;
    assert_eq!(s.cursor(), Some((Dirt, 2)));
    s.click(12, 1, PickupAll).await;
    assert_eq!(
        (s.slot(9), s.slot(10), s.slot(44), s.cursor()),
        (Some((Dirt, 10)), Some((Dirt, 8)), None, Some((Dirt, 64)))
    );

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn drop_keys_throw_from_the_hovered_slot_and_clone_needs_creative() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    s.setup(&[(9, Diamond, 5), (10, Dirt, 7)], None).await;

    s.click(9, 0, Throw).await;
    assert_eq!(s.slot(9), Some((Diamond, 4)));
    s.click(9, 1, Throw).await;
    assert_eq!(s.slot(9), None);
    s.click(9, 0, Throw).await;
    s.click(OUTSIDE, 1, Throw).await;
    assert_eq!(s.dropped[&Diamond], 5);
    // Not while the cursor holds something.
    s.click(10, 1, Pickup).await;
    s.click(10, 1, Throw).await;
    assert_eq!(s.slot(10), Some((Dirt, 3)));
    // No cloning out of nothing in survival.
    s.click(10, 0, Pickup).await;
    s.click(10, 2, CloneClick).await;
    assert_eq!(s.cursor(), None);

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn creative_players_clone_and_middle_drag() {
    let server = creative().await;
    let mut s = Session::join(&server, true).await;
    s.setup(&[(9, EnderPearl, 2)], None).await;

    // Creative items come out of nothing: the totals expect them.
    *s.totals.entry(EnderPearl).or_default() += 16;
    s.click(9, 2, CloneClick).await;
    assert_eq!(s.cursor(), Some((EnderPearl, 16)));
    s.click(OUTSIDE, 8, QuickCraft).await;
    s.click(20, 9, QuickCraft).await;
    s.click(21, 9, QuickCraft).await;
    *s.totals.entry(EnderPearl).or_default() += 32;
    s.click(OUTSIDE, 10, QuickCraft).await;
    assert_eq!(
        (s.slot(20), s.slot(21), s.cursor()),
        (
            Some((EnderPearl, 16)),
            Some((EnderPearl, 16)),
            Some((EnderPearl, 16))
        )
    );

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn wrong_predictions_are_corrected_and_right_ones_are_not_echoed() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    s.setup(
        &[(9, Diamond, 10), (10, Dirt, 5), (20, DiamondHelmet, 1)],
        None,
    )
    .await;

    // `click` already asserts that a right prediction gets no packet at all.
    s.click(9, 1, Pickup).await;

    // A forged count, a forged item in an untouched slot, a forged cursor:
    // each is resent with the server's contents and a fresh state id.
    let state_id = s.state_id;
    s.send_with(30, 0, Pickup, state_id, |changed, carried| {
        changed[0].1 = Some((Diamond, 64));
        changed.push((10, Some((Diamond, 64))));
        changed.push((11, Some((Diamond, 64))));
        *carried = Some((Diamond, 64));
    })
    .await;
    s.settle().await;
    assert_eq!(s.updates.len(), 4, "{:?}", s.updates);
    assert_eq!(
        (s.slot(30), s.slot(10)),
        (Some((Diamond, 5)), Some((Dirt, 5)))
    );

    // A prediction that leaves out a slot the click changed.
    let state_id = s.state_id;
    s.send_with(9, 0, Pickup, state_id, |changed, _| changed.clear())
        .await;
    s.settle().await;
    assert_eq!(s.updates.len(), 1, "{:?}", s.updates);

    // A client that does not know where a helmet goes predicts a shift-click
    // into the hotbar; the server sends it to the empty head slot, as
    // vanilla does, and corrects the client.
    let state_id = s.state_id;
    s.send_with(20, 0, QuickMove, state_id, |changed, _| {
        *changed = vec![(20, None), (36, Some((DiamondHelmet, 1)))];
    })
    .await;
    s.settle().await;
    assert_eq!((s.slot(5), s.slot(36)), (Some((DiamondHelmet, 1)), None));
    assert_eq!(s.updates.len(), 2, "{:?}", s.updates);

    // A stale state id is answered with the whole window.
    let stale = s.state_id.wrapping_sub(3);
    s.send_with(5, 0, Pickup, stale, |_, _| {}).await;
    s.settle().await;
    assert!(
        matches!(s.updates[..], [Update::Content(..)]),
        "{:?}",
        s.updates
    );

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn invalid_and_racing_clicks_never_duplicate_items() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    s.setup(&[(9, Diamond, 10), (10, Dirt, 5)], Some((Diamond, 3)))
        .await;

    // Slots outside the window, unknown buttons and modes, a wrong window.
    for slot in [46, 100, i16::MAX, -2, -1000, i16::MIN, -1] {
        for mode in [
            Pickup, QuickMove, Swap, CloneClick, Throw, QuickCraft, PickupAll,
        ] {
            s.click(slot, 0, mode).await;
        }
    }
    for button in [2, 3, 7, 40, 99, 128, 255] {
        s.click(9, button, Pickup).await;
        s.click(10, button, QuickMove).await;
        s.click(OUTSIDE, button, Pickup).await;
    }
    s.bot
        .send(ServerboundContainerClick {
            container_id: 5,
            state_id: s.state_id,
            slot_num: 9,
            button_num: 0,
            click_type: Pickup,
            changed_slots: [(9, hashed(None))].into_iter().collect(),
            carried_item: hashed(Some((Diamond, 13))),
        })
        .await;
    s.settle().await;
    assert!(s.updates.is_empty());

    // Clicks claiming more than the server computes.
    let state_id = s.state_id;
    s.send_with(9, 0, Pickup, state_id, |changed, carried| {
        changed[0].1 = Some((Diamond, 64));
        *carried = Some((Diamond, 64));
    })
    .await;
    s.settle().await;
    let state_id = s.state_id;
    s.send_with(11, 0, Pickup, state_id, |changed, carried| {
        changed.push((12, Some((Diamond, 99))));
        *carried = Some((Stone, 1));
    })
    .await;
    s.settle().await;

    // Many clicks in a row before the server answers any of them.
    for slot in [10, 20, 21, 20, 10, 22, 23] {
        s.send(slot, 0, Pickup).await;
        s.send(slot, 1, Pickup).await;
    }
    s.send(OUTSIDE, 0, QuickCraft).await;
    s.send(30, 1, QuickCraft).await;
    s.send(30, 1, QuickCraft).await;
    s.send(31, 1, QuickCraft).await;
    s.send(OUTSIDE, 2, QuickCraft).await;
    s.settle().await;

    // Clicks racing a /give and a /clear. Their exact result depends on
    // which the server handles first, so only the invariants are checked.
    let state_id = s.state_id;
    s.send_with(10, 0, Pickup, state_id, |_, _| {}).await;
    s.bot.command("give diamond 7").await;
    s.send_with(24, 0, Pickup, state_id, |_, _| {}).await;
    s.bot.sync().await;
    *s.totals.entry(Diamond).or_default() += 7;
    s.adopt_server_state().await;
    s.settle().await;
    let state_id = s.state_id;
    s.send_with(9, 0, Pickup, state_id, |_, _| {}).await;
    s.bot.command("clear").await;
    s.send_with(25, 0, Pickup, state_id, |_, _| {}).await;
    s.bot.sync().await;
    s.totals = s.dropped.clone();
    s.adopt_server_state().await;
    s.settle().await;
    assert!(s.model.slots.iter().all(Option::is_none) && s.cursor().is_none());

    s.finish();
    server.stop().await;
}

/// A small, seeded xorshift generator: the same clicks on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }
}

#[tokio::test]
async fn a_long_seeded_click_sequence_keeps_every_invariant() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    s.setup(
        &[
            (9, Diamond, 40),
            (10, Diamond, 64),
            (14, EnderPearl, 9),
            (20, Dirt, 33),
            (27, Stone, 64),
            (36, DiamondSword, 1),
            (37, Diamond, 7),
            (40, EnderPearl, 16),
            (44, Dirt, 1),
            (OFFHAND, Stone, 12),
            (2, Dirt, 5),
            (RESULT, Diamond, 3),
        ],
        None,
    )
    .await;

    let mut rng = Rng(0x0014_c11c_5eed);
    let slot = |rng: &mut Rng| -> i16 {
        match rng.below(20) {
            0 => OUTSIDE,
            1 => rng.pick(&[-1, -2, 46, 300]),
            _ => rng.below(SIZE as u64) as i16,
        }
    };
    for _ in 0..400 {
        let batch = 1 + rng.below(3);
        for _ in 0..batch {
            match rng.below(14) {
                0..=4 => {
                    let at = slot(&mut rng);
                    s.send(at, rng.pick(&[0, 0, 1, 1, 2]), Pickup).await;
                }
                5 => {
                    let at = slot(&mut rng);
                    s.send(at, rng.pick(&[0, 1]), QuickMove).await;
                }
                6 => {
                    let at = slot(&mut rng);
                    s.send(at, rng.pick(&[0, 3, 8, 40, 9]), Swap).await;
                }
                7 => {
                    let at = slot(&mut rng);
                    s.send(at, rng.pick(&[0, 1]), Throw).await;
                }
                8 => {
                    let at = slot(&mut rng);
                    s.send(at, rng.pick(&[0, 1]), PickupAll).await;
                }
                9..=10 => {
                    let kind = rng.pick(&[0u8, 4, 8]);
                    s.send(OUTSIDE, kind, QuickCraft).await;
                    for _ in 0..rng.below(5) {
                        let at = slot(&mut rng);
                        s.send(at, kind + 1, QuickCraft).await;
                    }
                    if rng.below(5) != 0 {
                        s.send(OUTSIDE, kind + 2, QuickCraft).await;
                    }
                }
                11 => {
                    let at = slot(&mut rng);
                    s.send(at, rng.below(16) as u8, QuickCraft).await;
                }
                12 => {
                    let at = slot(&mut rng);
                    let lie = rng.below(SIZE as u64) as u16;
                    let state_id = s.state_id;
                    s.send_with(at, 0, Pickup, state_id, |changed, carried| {
                        changed.push((lie, Some((Diamond, 64))));
                        *carried = None;
                    })
                    .await;
                }
                _ => {
                    let at = slot(&mut rng);
                    let stale = s.state_id.wrapping_add(1 + rng.below(5) as u32);
                    s.send_with(at, 1, Pickup, stale, |_, _| {}).await;
                }
            }
        }
        s.settle().await;
    }

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn closing_the_inventory_returns_the_cursor_and_crafting_grid() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    s.setup(&[(9, Diamond, 60), (1, Dirt, 2), (4, Dirt, 3)], None)
        .await;
    s.click(9, 0, Pickup).await;
    s.bot
        .send(ServerboundContainerClose { container_id: 0 })
        .await;
    s.model.slots[1] = None;
    s.model.slots[4] = None;
    s.model.slots[36] = Some((Diamond, 60));
    s.model.slots[37] = Some((Dirt, 5));
    s.model.carried = None;
    s.settle().await;
    assert!(
        s.updates.iter().any(|u| matches!(u, Update::Cursor(None))),
        "{:?}",
        s.updates
    );

    // With no room left, the cursor stack is dropped, once.
    let full: Vec<(usize, ItemKind, i32)> = (9..45).map(|i| (i, Stone, 64)).collect();
    s.setup(&full, Some((Diamond, 12))).await;
    s.bot
        .send(ServerboundContainerClose { container_id: 0 })
        .await;
    s.model.carried = None;
    s.model.drops.push((Diamond, 12));
    s.settle().await;
    assert_eq!(s.dropped[&Diamond], 12);

    s.finish();
    server.stop().await;
}

#[tokio::test]
async fn creative_slot_packets_only_work_on_a_creative_server() {
    for creative_mode in [false, true] {
        let server = if creative_mode {
            creative().await
        } else {
            survival().await
        };
        let mut s = Session::join(&server, creative_mode).await;
        s.setup(&[(9, Dirt, 1)], None).await;
        // A valid stack, one over its max size, one into the crafting output.
        for (slot, kind, count) in [(9, Diamond, 64), (10, EnderPearl, 17), (0, Diamond, 1)] {
            s.bot
                .set_creative_slot(slot, ItemStack::new(kind, count))
                .await;
        }
        if creative_mode {
            s.model.slots[9] = Some((Diamond, 64));
            s.totals = BTreeMap::from([(Diamond, 64)]);
        }
        s.settle().await;
        // Slot -1 throws from the creative inventory: a stack within its max
        // size, then one over it, which is refused.
        for (kind, count) in [(Diamond, 3), (EnderPearl, 17)] {
            s.bot
                .send(ServerboundSetCreativeModeSlot {
                    slot_num: u16::MAX,
                    item_stack: ItemStack::new(kind, count),
                })
                .await;
        }
        if creative_mode {
            s.model.drops.push((Diamond, 3));
            *s.totals.entry(Diamond).or_default() += 3;
        }
        s.settle().await;
        assert!(
            s.updates.is_empty(),
            "a throw from the creative inventory was answered with {:?}",
            s.updates
        );
        s.finish();
        server.stop().await;
    }
}

/// Armor slots refuse whatever is not their own armor, whichever click puts
/// it there, and hold one item; a client that predicts otherwise is
/// corrected. The off-hand takes any item. Bob is shown what lands in
/// Alice's hands and armor slots.
#[tokio::test]
async fn armor_slots_take_only_their_own_armor_and_the_offhand_takes_anything() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    let bob = server.join("Bob").await;
    s.setup(
        &[
            (9, Dirt, 10),
            (10, DiamondHelmet, 1),
            (11, DiamondChestplate, 1),
            (37, Stone, 3),
            (38, DiamondBoots, 1),
            (39, CarvedPumpkin, 5),
        ],
        None,
    )
    .await;

    // Dirt goes into no armor slot, by either button.
    s.click(9, 0, Pickup).await;
    for slot in 5..=8 {
        s.click(slot, 0, Pickup).await;
        s.click(slot, 1, Pickup).await;
    }
    assert_eq!(
        (
            (5..=8).map(|slot| s.slot(slot)).collect::<Vec<_>>(),
            s.cursor()
        ),
        (vec![None; 4], Some((Dirt, 10)))
    );
    // A client that predicts the dirt on its head gets the slot and its
    // cursor back.
    let state_id = s.state_id;
    s.send_with(5, 0, Pickup, state_id, |changed, carried| {
        changed.push((5, Some((Dirt, 10))));
        *carried = None;
    })
    .await;
    s.settle().await;
    assert_eq!(s.updates.len(), 2, "{:?}", s.updates);
    assert_eq!((s.slot(5), s.cursor()), (None, Some((Dirt, 10))));
    // A drag skips the armor slots it crosses.
    s.click(OUTSIDE, 0, QuickCraft).await;
    for slot in [6, 20, 7, 21] {
        s.click(slot, 1, QuickCraft).await;
    }
    s.click(OUTSIDE, 2, QuickCraft).await;
    assert_eq!(
        (s.slot(6), s.slot(7), s.slot(20), s.slot(21), s.cursor()),
        (None, None, Some((Dirt, 5)), Some((Dirt, 5)), None)
    );

    // The off-hand takes anything.
    s.click(20, 0, Pickup).await;
    s.click(OFFHAND as i16, 0, Pickup).await;
    assert_eq!(s.slot(OFFHAND), Some((Dirt, 5)));
    wait_for_equipment(&bob, &s.bot, &[(Offhand, Dirt, 5)]).await;

    // A helmet goes on the head only, and a chestplate does not replace it.
    s.click(10, 0, Pickup).await;
    s.click(6, 0, Pickup).await;
    s.click(5, 0, Pickup).await;
    s.click(11, 0, Pickup).await;
    s.click(5, 0, Pickup).await;
    assert_eq!(
        (s.slot(5), s.cursor()),
        (Some((DiamondHelmet, 1)), Some((DiamondChestplate, 1)))
    );
    s.click(6, 0, Pickup).await;
    assert_eq!(
        (s.slot(6), s.cursor()),
        (Some((DiamondChestplate, 1)), None)
    );

    // Number keys: boots onto the feet, stone onto no armor slot.
    s.click(8, 2, Swap).await;
    s.click(7, 1, Swap).await;
    assert_eq!(
        (s.slot(8), s.slot(38), s.slot(7), s.slot(37)),
        (Some((DiamondBoots, 1)), None, None, Some((Stone, 3)))
    );
    wait_for_equipment(
        &bob,
        &s.bot,
        &[
            (Offhand, Dirt, 5),
            (Feet, DiamondBoots, 1),
            (Chest, DiamondChestplate, 1),
            (Head, DiamondHelmet, 1),
        ],
    )
    .await;

    // Five pumpkins swapped onto the head: one goes on, four stay in the
    // hotbar, and the helmet goes back into the inventory, into the empty
    // selected slot: Alice now holds it.
    s.click(5, 3, Swap).await;
    assert_eq!(
        (s.slot(5), s.slot(39), s.slot(36)),
        (
            Some((CarvedPumpkin, 1)),
            Some((CarvedPumpkin, 4)),
            Some((DiamondHelmet, 1))
        )
    );
    // A pumpkin stack cannot join the one on the head.
    s.click(39, 0, Pickup).await;
    s.click(5, 0, Pickup).await;
    s.click(5, 1, Pickup).await;
    s.click(39, 0, Pickup).await;
    assert_eq!(s.slot(5), Some((CarvedPumpkin, 1)));
    // Shift-clicked off the feet, the boots go into the inventory.
    s.click(8, 0, QuickMove).await;
    assert_eq!((s.slot(8), s.slot(9)), (None, Some((DiamondBoots, 1))));
    wait_for_equipment(
        &bob,
        &s.bot,
        &[
            (Mainhand, DiamondHelmet, 1),
            (Offhand, Dirt, 5),
            (Chest, DiamondChestplate, 1),
            (Head, CarvedPumpkin, 1),
        ],
    )
    .await;

    bob.assert_healthy();
    s.finish();
    server.stop().await;
}

/// A shift-click sends a helmet, boots or a pumpkin to the empty armor slot
/// they belong in and a shield to the empty off-hand; with that slot taken,
/// the item moves as any other. Bob is shown the result.
#[tokio::test]
async fn shift_clicks_send_armor_and_shields_to_their_empty_slots() {
    let server = survival().await;
    let mut s = Session::join(&server, false).await;
    let bob = server.join("Bob").await;
    s.setup(
        &[
            (9, DiamondHelmet, 1),
            (10, DiamondHelmet, 1),
            (11, DiamondBoots, 1),
            (12, CarvedPumpkin, 10),
            (36, Shield, 1),
            (37, Shield, 1),
        ],
        None,
    )
    .await;

    s.click(9, 0, QuickMove).await;
    assert_eq!((s.slot(9), s.slot(5)), (None, Some((DiamondHelmet, 1))));
    s.click(10, 0, QuickMove).await;
    assert_eq!((s.slot(10), s.slot(38)), (None, Some((DiamondHelmet, 1))));
    s.click(36, 0, QuickMove).await;
    assert_eq!((s.slot(36), s.slot(OFFHAND)), (None, Some((Shield, 1))));
    s.click(37, 0, QuickMove).await;
    assert_eq!((s.slot(37), s.slot(9)), (None, Some((Shield, 1))));
    s.click(11, 0, QuickMove).await;
    assert_eq!((s.slot(11), s.slot(8)), (None, Some((DiamondBoots, 1))));
    wait_for_equipment(
        &bob,
        &s.bot,
        &[
            (Offhand, Shield, 1),
            (Feet, DiamondBoots, 1),
            (Head, DiamondHelmet, 1),
        ],
    )
    .await;

    // Off the head, the helmet goes into the inventory; then one pumpkin of
    // ten goes on the head and the rest moves on to the hotbar.
    s.click(5, 0, QuickMove).await;
    assert_eq!((s.slot(5), s.slot(10)), (None, Some((DiamondHelmet, 1))));
    s.click(12, 0, QuickMove).await;
    assert_eq!(
        (s.slot(12), s.slot(5), s.slot(36)),
        (None, Some((CarvedPumpkin, 1)), Some((CarvedPumpkin, 9)))
    );
    // A shield already in the off-hand goes into the inventory.
    s.click(OFFHAND as i16, 0, QuickMove).await;
    assert_eq!((s.slot(OFFHAND), s.slot(11)), (None, Some((Shield, 1))));
    wait_for_equipment(
        &bob,
        &s.bot,
        &[
            (Mainhand, CarvedPumpkin, 9),
            (Feet, DiamondBoots, 1),
            (Head, CarvedPumpkin, 1),
        ],
    )
    .await;

    bob.assert_healthy();
    s.finish();
    server.stop().await;
}

/// On a creative server, the creative inventory sets any slot, armor
/// included, as vanilla allows (a block on the head); Bob is shown each
/// change to Alice's hands and armor.
#[tokio::test]
async fn others_see_equipment_set_through_the_creative_inventory() {
    let server = creative().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    for (slot, item) in [
        (36, ItemStack::new(DiamondSword, 1)),
        (45, ItemStack::new(Shield, 1)),
        (5, ItemStack::new(Dirt, 1)),
        (6, ItemStack::new(DiamondChestplate, 1)),
    ] {
        alice
            .send(ServerboundSetCreativeModeSlot {
                slot_num: slot,
                item_stack: item,
            })
            .await;
    }
    wait_for_equipment(
        &bob,
        &alice,
        &[
            (Mainhand, DiamondSword, 1),
            (Offhand, Shield, 1),
            (Chest, DiamondChestplate, 1),
            (Head, Dirt, 1),
        ],
    )
    .await;
    for (slot, item) in [
        (36, ItemStack::Empty),
        (5, ItemStack::new(DiamondHelmet, 1)),
    ] {
        alice
            .send(ServerboundSetCreativeModeSlot {
                slot_num: slot,
                item_stack: item,
            })
            .await;
    }
    wait_for_equipment(
        &bob,
        &alice,
        &[
            (Offhand, Shield, 1),
            (Chest, DiamondChestplate, 1),
            (Head, DiamondHelmet, 1),
        ],
    )
    .await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}
