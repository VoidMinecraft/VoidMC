//! VMC-ATP-014: every click mode on the player inventory (window 0).
//!
//! Each click is predicted by [`Vanilla`], an independent transcription of
//! vanilla's `AbstractContainerMenu.doClick` and `InventoryMenu.quickMoveStack`
//! for window 0, and sent the way a vanilla client sends it: the hashed
//! contents it now expects in every slot it changed, and its cursor. After
//! each action the test checks that
//!
//! - the server's `Inventory` (46 slots and cursor) is what vanilla computes,
//! - the client, emulated like a vanilla client (its own prediction, then the
//!   server's corrections), ends up equal to the server,
//! - every state id the server sends is the client's last one plus one,
//! - each item's total over the slots, the cursor and the stacks dropped into
//!   the world never changes except through `/give` and `/clear`, and no
//!   stack exceeds its max size.
//!
//! VoidMC has no equipment data for items: armor and off-hand slots hold any
//! item and shift-clicks never route to them. The model treats them as plain
//! slots too, so the scenarios use no equippable item except where that
//! difference itself is tested.

use std::collections::BTreeMap;

use bevy_ecs::prelude::Entity;
use voidmc::components::ItemEntity;
use voidmc::events::EntityDespawnEvent;
use voidmc::{Inventory, ItemId, ItemStack as ServerItemStack};
use voidmc_e2e::inventory::ItemStack;
use voidmc_e2e::inventory::operations::ClickType;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::protocol::packets::game::s_container_click::{
    HashedActualItem, HashedPatchMap, HashedStack, ServerboundContainerClick,
};
use voidmc_e2e::protocol::packets::game::s_container_close::ServerboundContainerClose;
use voidmc_e2e::protocol::packets::game::s_set_creative_mode_slot::ServerboundSetCreativeModeSlot;
use voidmc_e2e::registry::Registry;
use voidmc_e2e::registry::builtin::ItemKind;
use voidmc_e2e::{Bot, TestServer};

const SIZE: usize = 46;
const OUTSIDE: i16 = -999;
const RESULT: usize = 0;
const HOTBAR: usize = 36;
const OFFHAND: usize = 45;

type Stack = Option<(ItemKind, i32)>;

fn max_stack(kind: ItemKind) -> i32 {
    match kind {
        ItemKind::EnderPearl | ItemKind::Egg => 16,
        ItemKind::DiamondSword | ItemKind::DiamondHelmet => 1,
        _ => 64,
    }
}

fn stack(kind: ItemKind, count: i32) -> Stack {
    (count > 0).then_some((kind, count))
}

fn count(stack: Stack) -> i32 {
    stack.map_or(0, |(_, count)| count)
}

/// Vanilla's click rules for window 0, written in the shape of the vanilla
/// code. Deliberate differences, documented in VoidMC: armor and off-hand
/// slots are plain slots, and a creative middle-drag keeps the cursor stack.
#[derive(Clone, Debug)]
struct Vanilla {
    slots: Vec<Stack>,
    carried: Stack,
    quickcraft_status: i32,
    quickcraft_type: i32,
    quickcraft_slots: Vec<usize>,
    creative: bool,
    drops: Vec<(ItemKind, i32)>,
}

impl Vanilla {
    fn new(creative: bool) -> Self {
        Self {
            slots: vec![None; SIZE],
            carried: None,
            quickcraft_status: 0,
            quickcraft_type: 0,
            quickcraft_slots: Vec::new(),
            creative,
            drops: Vec::new(),
        }
    }

    fn may_place(&self, index: usize) -> bool {
        index != RESULT
    }

    fn can_item_quick_replace(&self, index: usize, carried: (ItemKind, i32)) -> bool {
        match self.slots[index] {
            None => true,
            Some((kind, count)) => kind == carried.0 && count <= max_stack(carried.0),
        }
    }

    fn reset_quickcraft(&mut self) {
        self.quickcraft_status = 0;
        self.quickcraft_slots.clear();
    }

    /// `ServerGamePacketListenerImpl.handleContainerClick` then `doClick`.
    fn click(&mut self, slot: i16, button: u8, mode: ClickType) {
        if !(slot == -1 || slot == OUTSIDE || (0..SIZE as i16).contains(&slot)) {
            return;
        }
        self.do_click(i32::from(slot), i32::from(button as i8), mode);
    }

    fn do_click(&mut self, slot: i32, button: i32, mode: ClickType) {
        if mode == ClickType::QuickCraft {
            let previous = self.quickcraft_status;
            self.quickcraft_status = button & 3;
            let out_of_order = (previous != 1 || self.quickcraft_status != 2)
                && previous != self.quickcraft_status;
            if out_of_order || self.carried.is_none() {
                self.reset_quickcraft();
            } else if self.quickcraft_status == 0 {
                self.quickcraft_type = (button >> 2) & 3;
                let valid = match self.quickcraft_type {
                    0 | 1 => true,
                    2 => self.creative,
                    _ => false,
                };
                if valid {
                    self.quickcraft_status = 1;
                    self.quickcraft_slots.clear();
                } else {
                    self.reset_quickcraft();
                }
            } else if self.quickcraft_status == 1 {
                // Vanilla throws on a slot below zero; VoidMC ignores the packet.
                let Ok(index) = usize::try_from(slot) else {
                    return;
                };
                let carried = self.carried.unwrap();
                if self.can_item_quick_replace(index, carried)
                    && self.may_place(index)
                    && (self.quickcraft_type == 2 || carried.1 > self.quickcraft_slots.len() as i32)
                    && !self.quickcraft_slots.contains(&index)
                {
                    self.quickcraft_slots.push(index);
                }
            } else if self.quickcraft_status == 2 {
                if !self.quickcraft_slots.is_empty() {
                    if self.quickcraft_slots.len() == 1 {
                        let only = self.quickcraft_slots[0] as i32;
                        let kind = self.quickcraft_type;
                        self.reset_quickcraft();
                        self.do_click(only, kind, ClickType::Pickup);
                        return;
                    }
                    let carried = self.carried.unwrap();
                    let slots = self.quickcraft_slots.len() as i32;
                    let max = max_stack(carried.0);
                    let mut remaining = carried.1;
                    for index in self.quickcraft_slots.clone() {
                        if self.can_item_quick_replace(index, carried)
                            && self.may_place(index)
                            && (self.quickcraft_type == 2 || carried.1 >= slots)
                        {
                            let existing = count(self.slots[index]);
                            let place = match self.quickcraft_type {
                                0 => carried.1 / slots,
                                1 => 1,
                                _ => max,
                            };
                            let placed = (place + existing).min(max);
                            remaining -= placed - existing;
                            self.slots[index] = stack(carried.0, placed);
                        }
                    }
                    if self.quickcraft_type != 2 {
                        self.carried = stack(carried.0, remaining);
                    }
                }
                self.reset_quickcraft();
            } else {
                self.reset_quickcraft();
            }
        } else if self.quickcraft_status != 0 {
            self.reset_quickcraft();
        } else if (mode == ClickType::Pickup || mode == ClickType::QuickMove)
            && (button == 0 || button == 1)
        {
            if slot == i32::from(OUTSIDE) {
                if let Some((kind, carried)) = self.carried {
                    let thrown = if button == 0 { carried } else { 1 };
                    self.drops.push((kind, thrown));
                    self.carried = stack(kind, carried - thrown);
                }
            } else if slot < 0 {
            } else if mode == ClickType::QuickMove {
                self.quick_move(slot as usize);
            } else {
                self.pickup(slot as usize, button);
            }
        } else if mode == ClickType::Swap && ((0..9).contains(&button) || button == 40) {
            if slot >= 0 {
                self.swap(slot as usize, button);
            }
        } else if mode == ClickType::Clone && self.creative && self.carried.is_none() && slot >= 0 {
            if let Some((kind, _)) = self.slots[slot as usize] {
                self.carried = Some((kind, max_stack(kind)));
            }
        } else if mode == ClickType::Throw && self.carried.is_none() && slot >= 0 {
            if let Some((kind, held)) = self.slots[slot as usize] {
                let thrown = if button == 0 { 1 } else { held };
                self.drops.push((kind, thrown));
                self.slots[slot as usize] = stack(kind, held - thrown);
            }
        } else if mode == ClickType::PickupAll && slot >= 0 {
            self.pickup_all(slot as usize, button);
        }
    }

    fn pickup(&mut self, index: usize, button: i32) {
        match (self.slots[index], self.carried) {
            (None, None) => {}
            (None, Some(carried)) => {
                let increment = if button == 0 { carried.1 } else { 1 };
                self.carried = self.safe_insert(index, carried, increment);
            }
            (Some((kind, held)), None) => {
                let taken = if button == 0 { held } else { (held + 1) / 2 };
                self.slots[index] = stack(kind, held - taken);
                self.carried = stack(kind, taken);
            }
            (Some(held), Some(carried)) if self.may_place(index) => {
                if held.0 == carried.0 {
                    let increment = if button == 0 { carried.1 } else { 1 };
                    self.carried = self.safe_insert(index, carried, increment);
                } else if carried.1 <= max_stack(carried.0) {
                    self.slots[index] = Some(carried);
                    self.carried = Some(held);
                }
            }
            (Some(held), Some(carried)) => {
                // The result slot (allowModification false) gives all or nothing.
                if held.0 == carried.0 && max_stack(carried.0) - carried.1 >= held.1 {
                    self.slots[index] = None;
                    self.carried = stack(carried.0, carried.1 + held.1);
                }
            }
        }
    }

    fn safe_insert(&mut self, index: usize, carried: (ItemKind, i32), increment: i32) -> Stack {
        if !self.may_place(index) {
            return Some(carried);
        }
        let existing = count(self.slots[index]);
        let moved = increment
            .min(carried.1)
            .min(max_stack(carried.0) - existing);
        if moved <= 0 {
            return Some(carried);
        }
        match self.slots[index] {
            None => self.slots[index] = stack(carried.0, moved),
            Some((kind, held)) if kind == carried.0 => {
                self.slots[index] = stack(kind, held + moved)
            }
            Some(_) => return Some(carried),
        }
        stack(carried.0, carried.1 - moved)
    }

    /// `doClick` keeps calling `quickMoveStack` while it moves something and
    /// the slot still holds the same item.
    fn quick_move(&mut self, index: usize) {
        loop {
            let Some((kind, before)) = self.slots[index] else {
                return;
            };
            let (start, end, reverse) = match index {
                RESULT => (9, 45, true),
                1..=8 => (9, 45, false),
                9..=35 => (36, 45, false),
                36..=44 => (9, 36, false),
                _ => (9, 45, false),
            };
            if !self.move_item_stack_to(index, start, end, reverse) {
                return;
            }
            match self.slots[index] {
                Some((still, after)) if still == kind && after != before => {}
                _ => return,
            }
        }
    }

    fn move_item_stack_to(&mut self, from: usize, start: usize, end: usize, reverse: bool) -> bool {
        let (kind, mut moving) = self.slots[from].unwrap();
        let max = max_stack(kind);
        let order: Vec<usize> = if reverse {
            (start..end).rev().collect()
        } else {
            (start..end).collect()
        };
        let mut moved = false;
        if max > 1 {
            for &i in &order {
                if moving == 0 {
                    break;
                }
                if let Some((target, held)) = self.slots[i]
                    && target == kind
                {
                    if held + moving <= max {
                        self.slots[i] = stack(kind, held + moving);
                        moving = 0;
                        moved = true;
                    } else if held < max {
                        moving -= max - held;
                        self.slots[i] = stack(kind, max);
                        moved = true;
                    }
                }
            }
        }
        if moving > 0 {
            for &i in &order {
                if self.slots[i].is_none() && self.may_place(i) {
                    let placed = moving.min(max);
                    self.slots[i] = stack(kind, placed);
                    moving -= placed;
                    moved = true;
                    break;
                }
            }
        }
        self.slots[from] = stack(kind, moving);
        moved
    }

    fn swap(&mut self, index: usize, button: i32) {
        let hotbar = if button == 40 {
            OFFHAND
        } else {
            HOTBAR + button as usize
        };
        let held = self.slots[hotbar];
        let item = self.slots[index];
        match (held, item) {
            (None, None) => {}
            (None, Some(_)) => {
                self.slots[hotbar] = item;
                self.slots[index] = None;
            }
            (Some(_), None) => {
                if self.may_place(index) {
                    self.slots[index] = held;
                    self.slots[hotbar] = None;
                }
            }
            (Some(_), Some(_)) => {
                if self.may_place(index) {
                    self.slots[hotbar] = item;
                    self.slots[index] = held;
                }
            }
        }
    }

    fn pickup_all(&mut self, index: usize, button: i32) {
        let Some((kind, _)) = self.carried else {
            return;
        };
        if self.slots[index].is_some() {
            return;
        }
        let max = max_stack(kind);
        for pass in 0..2 {
            let order: Vec<usize> = if button == 0 {
                (0..SIZE).collect()
            } else {
                (0..SIZE).rev().collect()
            };
            for j in order {
                if count(self.carried) >= max {
                    break;
                }
                if j == RESULT {
                    continue;
                }
                if let Some((other, held)) = self.slots[j]
                    && other == kind
                    && (pass != 0 || held != max_stack(other))
                {
                    let taken = held.min(max - count(self.carried));
                    self.slots[j] = stack(kind, held - taken);
                    self.carried = stack(kind, count(self.carried) + taken);
                }
            }
        }
    }
}

fn hashed(stack: Stack) -> HashedStack {
    HashedStack(stack.map(|(kind, count)| HashedActualItem {
        kind,
        count,
        components: HashedPatchMap {
            added_components: Vec::new(),
            removed_components: Vec::new(),
        },
    }))
}

fn from_client(item: &ItemStack) -> Stack {
    match item {
        ItemStack::Empty => None,
        ItemStack::Present(data) => stack(data.kind, data.count),
    }
}

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

/// An inventory packet as the client applies it.
#[derive(Debug)]
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

/// One bot on a server, with the vanilla model, the emulated client view and
/// the expected item totals.
struct Session<'a> {
    server: &'a TestServer,
    bot: Bot,
    entity: Entity,
    model: Vanilla,
    client: Vec<Stack>,
    client_cursor: Stack,
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
            model: Vanilla::new(creative),
            client: vec![None; SIZE],
            client_cursor: None,
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
        let (inventory, cursor, state_id) = self.bot.view(|view| {
            (
                view.inventory.iter().map(from_client).collect::<Vec<_>>(),
                from_client(&view.cursor),
                view.inventory_state_id,
            )
        });
        self.bot.clear();
        self.client = inventory;
        self.client_cursor = cursor;
        self.state_id = state_id;
        self.model.slots = self.client.clone();
        self.model.carried = cursor;
        self.model.quickcraft_status = 0;
        self.model.quickcraft_slots.clear();
        self.model.drops.clear();
        self.totals.clear();
        self.dropped.clear();
        for (kind, count) in self.client.iter().chain([&cursor]).flatten() {
            *self.totals.entry(*kind).or_default() += count;
        }
        self.check().await;
    }

    /// Sends a click the way a vanilla client does, without waiting.
    async fn send(&mut self, slot: i16, button: u8, mode: ClickType) {
        self.send_with(slot, button, mode, self.state_id, |_, _| {})
            .await;
    }

    /// Sends a click whose prediction `forge` rewrites after vanilla's rules
    /// made it; the emulated client believes the forged prediction.
    async fn send_with(
        &mut self,
        slot: i16,
        button: u8,
        mode: ClickType,
        state_id: u32,
        forge: impl FnOnce(&mut Vec<(u16, Stack)>, &mut Stack),
    ) {
        let before = self.model.clone();
        self.model.click(slot, button, mode);
        let mut changed: Vec<(u16, Stack)> = (0..SIZE)
            .filter(|&i| before.slots[i] != self.model.slots[i])
            .map(|i| (i as u16, self.model.slots[i]))
            .collect();
        let mut carried = self.model.carried;
        forge(&mut changed, &mut carried);
        for &(slot, predicted) in &changed {
            self.client[usize::from(slot)] = predicted;
        }
        self.client_cursor = carried;
        self.bot
            .send(ServerboundContainerClick {
                container_id: 0,
                state_id,
                slot_num: slot,
                button_num: button,
                click_type: mode,
                changed_slots: changed
                    .into_iter()
                    .map(|(slot, item)| (slot, hashed(item)))
                    .collect(),
                carried_item: hashed(carried),
            })
            .await;
    }

    /// One click, then every check.
    async fn click(&mut self, slot: i16, button: u8, mode: ClickType) {
        self.send(slot, button, mode).await;
        self.settle().await;
        assert!(
            self.updates.is_empty(),
            "a correct prediction was answered with {:?}",
            self.updates
        );
    }

    /// Waits for the server to handle everything sent, applies its inventory
    /// packets to the emulated client, and checks every invariant.
    async fn settle(&mut self) {
        self.bot.sync().await;
        self.updates.clear();
        while !self.bot.queued(inventory_update).is_empty() {
            let update = self
                .bot
                .expect("an inventory update", inventory_update)
                .await;
            match &update {
                Update::Slot(state_id, slot, stack) => {
                    self.advance_state_id(*state_id);
                    self.client[*slot] = *stack;
                }
                Update::Content(state_id, slots, cursor) => {
                    self.advance_state_id(*state_id);
                    self.client = slots.clone();
                    self.client_cursor = *cursor;
                }
                Update::Cursor(stack) => self.client_cursor = *stack,
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

        for i in 0..SIZE {
            assert_eq!(
                state.slots[i], self.model.slots[i],
                "server slot {i} vs vanilla"
            );
            assert_eq!(self.client[i], state.slots[i], "client slot {i} vs server");
        }
        assert_eq!(state.cursor, self.model.carried, "server cursor vs vanilla");
        assert_eq!(self.client_cursor, state.cursor, "client cursor vs server");

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
use ItemKind::{Diamond, DiamondHelmet, DiamondSword, Dirt, EnderPearl, Stone};

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
            (5, Dirt, 2),
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
            Some((Dirt, 2)),
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

    // A vanilla client sends a helmet to the head slot; VoidMC, without
    // equipment data, keeps it in the inventory, and the client follows.
    let state_id = s.state_id;
    s.send_with(20, 0, QuickMove, state_id, |changed, _| {
        *changed = vec![(20, None), (5, Some((DiamondHelmet, 1)))];
    })
    .await;
    s.settle().await;
    assert_eq!((s.slot(5), s.slot(36)), (None, Some((DiamondHelmet, 1))));

    // A stale state id is answered with the whole window.
    let stale = s.state_id.wrapping_sub(3);
    s.send_with(36, 0, Pickup, stale, |_, _| {}).await;
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
                .send(ServerboundSetCreativeModeSlot {
                    slot_num: slot,
                    item_stack: ItemStack::new(kind, count),
                })
                .await;
            s.client[usize::from(slot)] = Some((kind, count));
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
