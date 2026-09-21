//! The player [`Inventory`] component and its slot layout.
//!
//! Window id 0 (the player's own inventory) has 46 slots in a fixed order:
//!
//! ```text
//!  0        crafting output
//!  1..=4    crafting grid
//!  5..=8    armor (head, chest, legs, feet)
//!  9..=35   main inventory (3 rows)
//!  36..=44  hotbar
//!  45       offhand
//! ```
//!
//! The selected hotbar key `k` (0..=8) maps to slot `36 + k`. Every setter
//! records what it changed; the inventory plugin turns that into the smallest
//! matching packets each tick, so nothing else has to remember to resync.

use bevy_ecs::prelude::*;
use voidmc_protocol::serverbound::ContainerInput;
use voidmc_protocol::slot::Slot;

use bevy_ecs::system::SystemParam;
use voidmc_protocol::clientbound::SetCooldown;

use crate::item::{ItemId, ItemStack};
use crate::menu::ContainerIds;
use crate::players::{Players, WorldPlayers};
use crate::window::{DragState, Layout, Window, deposit};

#[derive(Component, Clone)]
#[require(ContainerIds)]
pub struct Inventory {
    slots: [ItemStack; Self::SIZE],
    cursor: ItemStack,
    selected_hotbar: u8,
    drag: Option<DragState>,
    state_id: i32,
    dirty: Dirty,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Dirty {
    pub slots: u64,
    pub full: bool,
    pub cursor: bool,
    pub held: bool,
}

impl Dirty {
    #[cfg(test)]
    pub fn is_clean(&self) -> bool {
        self.slots == 0 && !self.full && !self.cursor && !self.held
    }

    pub fn slot_indices(&self) -> impl Iterator<Item = usize> + '_ {
        (0..64).filter(|i| self.slots & (1 << i) != 0)
    }

    pub fn slot_count(&self) -> u32 {
        self.slots.count_ones()
    }
}

impl Inventory {
    pub const CRAFTING_OUTPUT: usize = 0;
    pub const CRAFTING_GRID_START: usize = 1;
    pub const ARMOR_HEAD: usize = 5;
    pub const ARMOR_CHEST: usize = 6;
    pub const ARMOR_LEGS: usize = 7;
    pub const ARMOR_FEET: usize = 8;
    pub const MAIN_START: usize = 9;
    pub const HOTBAR_START: usize = 36;
    pub const OFFHAND: usize = 45;
    pub const SIZE: usize = 46;

    pub fn new() -> Self {
        Inventory {
            slots: std::array::from_fn(|_| ItemStack::EMPTY),
            cursor: ItemStack::EMPTY,
            selected_hotbar: 0,
            drag: None,
            state_id: 0,
            dirty: Dirty::default(),
        }
    }

    pub fn hotbar_slot_index(hotbar: u8) -> usize {
        Self::HOTBAR_START + (hotbar.min(8) as usize)
    }

    /// Out-of-range indices read as empty.
    pub fn get(&self, index: usize) -> &ItemStack {
        static EMPTY: ItemStack = ItemStack::EMPTY;
        self.slots.get(index).unwrap_or(&EMPTY)
    }

    /// Out-of-range indices are ignored; an unchanged value sends nothing.
    pub fn set(&mut self, index: usize, stack: ItemStack) {
        if let Some(slot) = self.slots.get_mut(index)
            && *slot != stack
        {
            *slot = stack;
            self.dirty.slots |= 1 << index;
        }
    }

    pub fn cursor(&self) -> &ItemStack {
        &self.cursor
    }

    pub fn set_cursor(&mut self, stack: ItemStack) {
        if self.cursor != stack {
            self.cursor = stack;
            self.dirty.cursor = true;
        }
    }

    /// The selected hotbar key (0..=8).
    pub fn selected_hotbar(&self) -> u8 {
        self.selected_hotbar
    }

    /// Moves the player's selection (clamped to 0..=8); the client is told
    /// with Set Held Slot.
    pub fn set_selected_hotbar(&mut self, hotbar: u8) {
        let hotbar = hotbar.min(8);
        if self.selected_hotbar != hotbar {
            self.selected_hotbar = hotbar;
            self.dirty.held = true;
        }
    }

    pub(crate) fn accept_selected_hotbar(&mut self, hotbar: u8) {
        self.selected_hotbar = hotbar.min(8);
    }

    pub fn held(&self) -> &ItemStack {
        &self.slots[Self::hotbar_slot_index(self.selected_hotbar)]
    }

    pub fn clear(&mut self) {
        self.slots.fill(ItemStack::EMPTY);
        self.cursor = ItemStack::EMPTY;
        self.dirty.full = true;
        self.dirty.cursor = true;
    }

    fn storage_order() -> impl Iterator<Item = usize> {
        (Self::HOTBAR_START..Self::OFFHAND).chain(Self::MAIN_START..Self::HOTBAR_START)
    }

    /// Stacks onto matching slots then fills empty ones (hotbar before main).
    /// Returns whatever did not fit.
    pub fn give(&mut self, mut stack: ItemStack) -> ItemStack {
        if stack.is_empty() {
            return ItemStack::EMPTY;
        }
        let before = self.slots.clone();
        let order: Vec<usize> = Self::storage_order().collect();
        deposit(&mut self.slots, &mut stack, &order);
        self.mark_changed_since(&before);
        stack
    }

    pub fn to_slots(&self) -> Vec<Slot> {
        self.slots.iter().map(ItemStack::to_slot).collect()
    }

    /// Applies a Container Click on window 0 and returns the stacks it threw
    /// into the world. `creative` allows cloning and middle-button drags.
    pub fn apply_click(
        &mut self,
        slot: i16,
        button: i8,
        input: ContainerInput,
        creative: bool,
    ) -> Vec<ItemStack> {
        let before = self.slots.clone();
        let cursor_before = self.cursor.clone();
        let dropped = Window {
            slots: &mut self.slots,
            cursor: &mut self.cursor,
            drag: &mut self.drag,
            layout: &Layout::PLAYER,
            creative,
        }
        .apply_click(slot, button, input);
        self.mark_changed_since(&before);
        if self.cursor != cursor_before {
            self.dirty.cursor = true;
        }
        dropped
    }

    fn mark_changed_since(&mut self, before: &[ItemStack; Self::SIZE]) {
        for (i, (old, new)) in before.iter().zip(&self.slots).enumerate() {
            if old != new {
                self.dirty.slots |= 1 << i;
            }
        }
    }

    pub(crate) fn slots(&self) -> &[ItemStack; Self::SIZE] {
        &self.slots
    }

    pub(crate) fn cursor_mut(&mut self) -> &mut ItemStack {
        &mut self.cursor
    }

    pub(crate) fn mark_slot(&mut self, index: usize) {
        self.dirty.slots |= 1 << index;
    }

    pub(crate) fn mark_cursor(&mut self) {
        self.dirty.cursor = true;
    }

    pub(crate) fn mark_full(&mut self) {
        self.dirty.full = true;
    }

    pub(crate) fn state_id(&self) -> i32 {
        self.state_id
    }

    pub(crate) fn next_state_id(&mut self) -> i32 {
        self.state_id = (self.state_id + 1) & 32767;
        self.state_id
    }

    pub(crate) fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
    }

    /// Puts back changes a menu window could not show, for the resync that
    /// follows its close.
    pub(crate) fn retain_dirty(&mut self, slots: u64) {
        self.dirty.slots |= slots;
    }

    #[cfg(test)]
    pub(crate) fn dirty(&self) -> Dirty {
        self.dirty
    }
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

/// A Set Cooldown request: the client greys the item out for `ticks`.
/// Items share a cooldown group named after the item unless their
/// `use_cooldown` component says otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cooldown {
    group: Option<String>,
    ticks: i32,
}

impl Cooldown {
    pub fn item(item: ItemId) -> Self {
        Cooldown {
            group: item.name().map(str::to_owned),
            ticks: 0,
        }
    }

    pub fn group(name: impl Into<String>) -> Self {
        let name = name.into();
        let group = if name.contains(':') {
            name
        } else {
            format!("minecraft:{name}")
        };
        Cooldown {
            group: Some(group),
            ticks: 0,
        }
    }

    pub fn ticks(mut self, ticks: u32) -> Self {
        self.ticks = ticks.min(i32::MAX as u32) as i32;
        self
    }

    fn packet(&self) -> Option<SetCooldown> {
        let Some(group) = &self.group else {
            debug_assert!(false, "cooldown for an unknown item");
            tracing::warn!("Cooldown for an unknown item; not sent");
            return None;
        };
        Some(SetCooldown {
            cooldown_group: group.clone(),
            duration: self.ticks,
        })
    }
}

#[derive(SystemParam)]
pub struct Inventories<'w, 's> {
    players: Players<'w, 's>,
    inventories: Query<'w, 's, &'static mut Inventory>,
}

impl Inventories<'_, '_> {
    /// Selects hotbar key `hotbar` (0..=8) for `player`; the client is told
    /// with Set Held Slot at the end of the tick.
    pub fn set_held_slot(&mut self, player: Entity, hotbar: u8) -> bool {
        match self.inventories.get_mut(player) {
            Ok(mut inv) => {
                inv.set_selected_hotbar(hotbar);
                true
            }
            Err(_) => false,
        }
    }

    pub fn cooldown(&self, player: Entity, cooldown: Cooldown) -> bool {
        match cooldown.packet() {
            Some(packet) => {
                self.players.send(player, packet);
                true
            }
            None => false,
        }
    }
}

pub struct WorldInventories<'w> {
    world: &'w mut World,
}

impl<'w> WorldInventories<'w> {
    pub fn new(world: &'w mut World) -> Self {
        Self { world }
    }

    pub fn set_held_slot(&mut self, player: Entity, hotbar: u8) -> bool {
        match self.world.get_mut::<Inventory>(player) {
            Some(mut inv) => {
                inv.set_selected_hotbar(hotbar);
                true
            }
            None => false,
        }
    }

    pub fn cooldown(&self, player: Entity, cooldown: Cooldown) -> bool {
        match cooldown.packet() {
            Some(packet) => {
                WorldPlayers::new(self.world).send(player, packet);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemId;

    #[test]
    fn hotbar_index_mapping() {
        assert_eq!(Inventory::hotbar_slot_index(0), 36);
        assert_eq!(Inventory::hotbar_slot_index(8), 44);
        assert_eq!(Inventory::hotbar_slot_index(20), 44);
    }

    #[test]
    fn to_slots_has_46_entries() {
        let inv = Inventory::new();
        assert_eq!(inv.to_slots().len(), Inventory::SIZE);
        assert!(inv.to_slots().iter().all(Slot::is_empty));
    }

    #[test]
    fn give_stacks_then_fills_and_returns_leftover() {
        let mut inv = Inventory::new();
        let left = inv.give(ItemStack::of("minecraft:stone", 10).unwrap());
        assert!(left.is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 10);
        let left = inv.give(ItemStack::of("minecraft:stone", 60).unwrap());
        assert!(left.is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 64);
        assert_eq!(inv.get(Inventory::HOTBAR_START + 1).count, 6);
    }

    #[test]
    fn give_overflows_when_full() {
        let mut inv = Inventory::new();
        for _ in 0..36 {
            inv.give(ItemStack::of("minecraft:dirt", 64).unwrap());
        }
        let leftover = inv.give(ItemStack::of("minecraft:dirt", 5).unwrap());
        assert_eq!(leftover.count, 5);
    }

    #[test]
    fn held_follows_selected_hotbar() {
        let mut inv = Inventory::new();
        inv.set(
            Inventory::hotbar_slot_index(3),
            ItemStack::of("minecraft:stone", 1).unwrap(),
        );
        inv.set_selected_hotbar(3);
        assert_eq!(
            inv.held().item,
            ItemId::from_name("minecraft:stone").unwrap()
        );
    }

    fn stone(n: u8) -> ItemStack {
        ItemStack::of("minecraft:stone", n).unwrap()
    }

    const H0: i16 = Inventory::HOTBAR_START as i16;
    const M0: i16 = Inventory::MAIN_START as i16;

    #[test]
    fn left_click_picks_up_then_places() {
        let mut inv = Inventory::new();
        inv.set(Inventory::HOTBAR_START, stone(10));
        inv.apply_click(H0, 0, ContainerInput::Pickup, false);
        assert!(inv.get(Inventory::HOTBAR_START).is_empty());
        assert_eq!(inv.cursor().count, 10);
        inv.apply_click(M0, 0, ContainerInput::Pickup, false);
        assert_eq!(inv.get(Inventory::MAIN_START).count, 10);
        assert!(inv.cursor().is_empty());
    }

    #[test]
    fn left_click_merges_same_item() {
        let mut inv = Inventory::new();
        inv.set(Inventory::HOTBAR_START, stone(60));
        inv.set(Inventory::MAIN_START, stone(20));
        inv.apply_click(M0, 0, ContainerInput::Pickup, false);
        inv.apply_click(H0, 0, ContainerInput::Pickup, false);
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 64);
        assert_eq!(inv.cursor().count, 16);
    }

    #[test]
    fn right_click_splits_and_places_one() {
        let mut inv = Inventory::new();
        inv.set(Inventory::HOTBAR_START, stone(9));
        inv.apply_click(H0, 1, ContainerInput::Pickup, false);
        assert_eq!(inv.cursor().count, 5);
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 4);
        inv.apply_click(M0, 1, ContainerInput::Pickup, false);
        assert_eq!(inv.get(Inventory::MAIN_START).count, 1);
        assert_eq!(inv.cursor().count, 4);
    }

    #[test]
    fn number_key_swaps_with_hotbar() {
        let mut inv = Inventory::new();
        inv.set(Inventory::MAIN_START, stone(5));
        inv.apply_click(M0, 0, ContainerInput::Swap, false);
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 5);
        assert!(inv.get(Inventory::MAIN_START).is_empty());
    }

    #[test]
    fn shift_click_moves_main_to_hotbar() {
        let mut inv = Inventory::new();
        inv.set(Inventory::MAIN_START, stone(32));
        inv.apply_click(M0, 0, ContainerInput::QuickMove, false);
        assert!(inv.get(Inventory::MAIN_START).is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 32);
    }

    #[test]
    fn throw_drops_items() {
        let mut inv = Inventory::new();
        inv.set(Inventory::HOTBAR_START, stone(5));
        let dropped = inv.apply_click(H0, 1, ContainerInput::Throw, false);
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].count, 5);
        assert!(inv.get(Inventory::HOTBAR_START).is_empty());
    }

    #[test]
    fn double_click_collects_matching() {
        let mut inv = Inventory::new();
        inv.set(Inventory::MAIN_START, stone(10));
        inv.set(Inventory::MAIN_START + 1, stone(20));
        inv.set(Inventory::MAIN_START + 2, stone(5));
        let slot = (Inventory::MAIN_START + 2) as i16;
        inv.apply_click(slot, 0, ContainerInput::Pickup, false);
        inv.apply_click(slot, 0, ContainerInput::PickupAll, false);
        assert_eq!(inv.cursor().count, 35);
    }

    #[test]
    fn give_respects_per_item_max_stack() {
        let mut inv = Inventory::new();
        let left = inv.give(ItemStack::of("minecraft:ender_pearl", 20).unwrap());
        assert!(left.is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 16);
        assert_eq!(inv.get(Inventory::HOTBAR_START + 1).count, 4);
    }

    #[test]
    fn left_drag_distributes_evenly() {
        let mut inv = Inventory::new();
        inv.set(Inventory::HOTBAR_START, stone(4));
        inv.apply_click(H0, 0, ContainerInput::Pickup, false);
        inv.apply_click(-999, 0, ContainerInput::QuickCraft, false);
        inv.apply_click(M0, 1, ContainerInput::QuickCraft, false);
        inv.apply_click(M0 + 1, 1, ContainerInput::QuickCraft, false);
        inv.apply_click(-999, 2, ContainerInput::QuickCraft, false);
        assert_eq!(inv.get(Inventory::MAIN_START).count, 2);
        assert_eq!(inv.get(Inventory::MAIN_START + 1).count, 2);
        assert!(inv.cursor().is_empty());
    }

    #[test]
    fn setters_mark_only_what_changed() {
        let mut inv = Inventory::new();
        assert!(inv.dirty().is_clean());
        inv.set(Inventory::MAIN_START, stone(1));
        inv.set(Inventory::MAIN_START, stone(1));
        inv.set(Inventory::SIZE + 3, stone(1));
        let dirty = inv.dirty();
        assert_eq!(dirty.slot_indices().collect::<Vec<_>>(), vec![9]);
        assert!(!dirty.cursor && !dirty.held && !dirty.full);

        inv.set_cursor(stone(2));
        inv.set_selected_hotbar(4);
        inv.set_selected_hotbar(4);
        let dirty = inv.take_dirty();
        assert!(dirty.cursor && dirty.held);
        assert!(inv.dirty().is_clean());

        inv.accept_selected_hotbar(2);
        assert_eq!(inv.selected_hotbar(), 2);
        assert!(inv.dirty().is_clean());
    }

    #[test]
    fn give_and_clicks_mark_touched_slots() {
        let mut inv = Inventory::new();
        inv.give(stone(70));
        assert_eq!(inv.dirty().slot_indices().collect::<Vec<_>>(), vec![36, 37]);
        inv.take_dirty();

        inv.apply_click(H0, 0, ContainerInput::Pickup, false);
        let dirty = inv.take_dirty();
        assert_eq!(dirty.slot_indices().collect::<Vec<_>>(), vec![36]);
        assert!(dirty.cursor);

        inv.apply_click(-999, 5, ContainerInput::QuickCraft, false);
        assert!(inv.take_dirty().is_clean());

        inv.clear();
        assert!(inv.dirty().full);
    }

    #[test]
    fn state_id_wraps_like_vanilla() {
        let mut inv = Inventory::new();
        assert_eq!(inv.next_state_id(), 1);
        for _ in 0..32766 {
            inv.next_state_id();
        }
        assert_eq!(inv.state_id(), 32767);
        assert_eq!(inv.next_state_id(), 0);
    }

    #[test]
    fn cooldown_builds_the_group_from_the_item_name() {
        let pearl = ItemId::from_name("minecraft:ender_pearl").unwrap();
        let packet = Cooldown::item(pearl).ticks(20).packet().unwrap();
        assert_eq!(packet.cooldown_group, "minecraft:ender_pearl");
        assert_eq!(packet.duration, 20);
        let packet = Cooldown::group("chorus_fruit").packet().unwrap();
        assert_eq!(packet.cooldown_group, "minecraft:chorus_fruit");
        assert_eq!(packet.duration, 0);
    }
}
