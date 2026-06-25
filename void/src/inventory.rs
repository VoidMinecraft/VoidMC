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
//! The selected hotbar key `k` (0..=8) maps to slot `36 + k`.

use bevy_ecs::prelude::*;
use voidmc_protocol::slot::{DataComponent, Slot};

use crate::item::{ItemId, ItemStack};

/// A player's 46-slot inventory plus the cursor (drag) item and the selected
/// hotbar index.
#[derive(Component, Clone)]
pub struct Inventory {
    slots: [ItemStack; Self::SIZE],
    cursor: ItemStack,
    selected_hotbar: u8,
    drag: Option<DragState>,
}

/// In-progress quick-craft (drag) state, accumulated across click packets.
#[derive(Clone)]
struct DragState {
    kind: DragKind,
    slots: Vec<usize>,
}

#[derive(Clone, Copy)]
enum DragKind {
    /// Split the cursor evenly across the painted slots.
    Left,
    /// Place one on each painted slot.
    Right,
    /// Fill each painted slot to max (creative).
    Middle,
}

impl Inventory {
    pub const CRAFTING_OUTPUT: usize = 0;
    pub const CRAFTING_GRID_START: usize = 1; // 1..=4
    pub const ARMOR_HEAD: usize = 5;
    pub const ARMOR_CHEST: usize = 6;
    pub const ARMOR_LEGS: usize = 7;
    pub const ARMOR_FEET: usize = 8;
    pub const MAIN_START: usize = 9; // 9..=35
    pub const HOTBAR_START: usize = 36; // 36..=44
    pub const OFFHAND: usize = 45;
    pub const SIZE: usize = 46;

    /// Default stack cap until per-item `max_stack_size` is wired up (M6).
    const DEFAULT_MAX_STACK: u8 = 64;

    /// An empty inventory.
    pub fn new() -> Self {
        Inventory {
            slots: std::array::from_fn(|_| ItemStack::EMPTY),
            cursor: ItemStack::EMPTY,
            selected_hotbar: 0,
            drag: None,
        }
    }

    /// The window slot index for hotbar key `hotbar` (0..=8).
    pub fn hotbar_slot_index(hotbar: u8) -> usize {
        Self::HOTBAR_START + (hotbar.min(8) as usize)
    }

    /// Reads a slot. Out-of-range indices read as empty.
    pub fn get(&self, index: usize) -> &ItemStack {
        static EMPTY: ItemStack = ItemStack::EMPTY;
        self.slots.get(index).unwrap_or(&EMPTY)
    }

    /// Writes a slot. Out-of-range indices are ignored.
    pub fn set(&mut self, index: usize, stack: ItemStack) {
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = stack;
        }
    }

    /// The cursor (held-while-dragging) item.
    pub fn cursor(&self) -> &ItemStack {
        &self.cursor
    }

    /// Sets the cursor item.
    pub fn set_cursor(&mut self, stack: ItemStack) {
        self.cursor = stack;
    }

    /// The selected hotbar key (0..=8).
    pub fn selected_hotbar(&self) -> u8 {
        self.selected_hotbar
    }

    /// Sets the selected hotbar key (clamped to 0..=8).
    pub fn set_selected_hotbar(&mut self, hotbar: u8) {
        self.selected_hotbar = hotbar.min(8);
    }

    /// The item in the currently selected hotbar slot.
    pub fn held(&self) -> &ItemStack {
        &self.slots[Self::hotbar_slot_index(self.selected_hotbar)]
    }

    /// Empties every slot and the cursor.
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = ItemStack::EMPTY;
        }
        self.cursor = ItemStack::EMPTY;
    }

    /// Storage slot indices in fill order: hotbar first, then main inventory.
    fn storage_order() -> impl Iterator<Item = usize> {
        (Self::HOTBAR_START..Self::OFFHAND).chain(Self::MAIN_START..Self::HOTBAR_START)
    }

    /// Inserts `stack`, stacking onto matching slots then filling empty ones
    /// (hotbar before main). Returns whatever did not fit.
    pub fn give(&mut self, mut stack: ItemStack) -> ItemStack {
        if stack.is_empty() {
            return ItemStack::EMPTY;
        }
        let max = Self::max_stack(&stack);

        // Stack onto existing matching slots.
        for i in Self::storage_order() {
            if stack.count == 0 {
                return ItemStack::EMPTY;
            }
            let slot = &mut self.slots[i];
            if !slot.is_empty()
                && slot.item == stack.item
                && slot.components == stack.components
                && slot.count < max
            {
                let moved = (max - slot.count).min(stack.count);
                slot.count += moved;
                stack.count -= moved;
            }
        }

        // Fill empty slots.
        for i in Self::storage_order() {
            if stack.count == 0 {
                return ItemStack::EMPTY;
            }
            if self.slots[i].is_empty() {
                let moved = stack.count.min(max);
                self.slots[i] = ItemStack {
                    item: stack.item,
                    count: moved,
                    components: stack.components.clone(),
                };
                stack.count -= moved;
            }
        }

        stack
    }

    /// The full slot list in window order, as protocol [`Slot`]s, for
    /// `SetContainerContent`.
    pub fn to_slots(&self) -> Vec<Slot> {
        self.slots.iter().map(ItemStack::to_slot).collect()
    }

    // -- Click handling (window id 0) ---------------------------------------

    /// Applies a `ClickContainer` action to this inventory and returns any items
    /// the action dropped into the world (for the caller to spawn as entities).
    ///
    /// `mode` is the `ContainerInput`: 0 pickup, 1 quick-move, 2 swap, 3 clone,
    /// 4 throw, 5 quick-craft (drag), 6 pickup-all.
    pub fn apply_click(&mut self, slot: i16, button: i8, mode: i32) -> Vec<ItemStack> {
        let valid = (slot >= 0 && (slot as usize) < Self::SIZE).then_some(slot as usize);
        match mode {
            0 => match valid {
                Some(s) => self.click_pickup(s, button == 1),
                None => self.drop_cursor(button == 1),
            },
            1 => {
                if let Some(s) = valid {
                    self.quick_move(s);
                }
                Vec::new()
            }
            2 => {
                if let Some(s) = valid {
                    self.click_swap(s, button);
                }
                Vec::new()
            }
            3 => {
                if let Some(s) = valid {
                    self.click_clone(s);
                }
                Vec::new()
            }
            4 => match valid {
                Some(s) => self.click_throw(s, button),
                None => Vec::new(),
            },
            5 => {
                self.quick_craft(slot, button);
                Vec::new()
            }
            6 => {
                if valid.is_some() {
                    self.pickup_all();
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// The maximum stack size for the given stack's item (64 by default).
    fn max_stack(stack: &ItemStack) -> u8 {
        if stack.is_empty() {
            return Self::DEFAULT_MAX_STACK;
        }
        voidmc_data::item_max_stack(voidmc_data::Version::V26_1_2, stack.item.0)
    }

    fn same_item(a: &ItemStack, b: &ItemStack) -> bool {
        !a.is_empty() && a.item == b.item && a.components == b.components
    }

    fn click_pickup(&mut self, slot: usize, right: bool) -> Vec<ItemStack> {
        let max = Self::max_stack(if self.cursor.is_empty() {
            &self.slots[slot]
        } else {
            &self.cursor
        });
        if self.cursor.is_empty() {
            if self.slots[slot].is_empty() {
                return Vec::new();
            }
            if right {
                let total = self.slots[slot].count;
                let take = total.div_ceil(2);
                self.cursor = ItemStack {
                    item: self.slots[slot].item,
                    count: take,
                    components: self.slots[slot].components.clone(),
                };
                let left = total - take;
                self.slots[slot] = if left == 0 {
                    ItemStack::EMPTY
                } else {
                    ItemStack {
                        item: self.slots[slot].item,
                        count: left,
                        components: self.slots[slot].components.clone(),
                    }
                };
            } else {
                self.cursor = std::mem::replace(&mut self.slots[slot], ItemStack::EMPTY);
            }
        } else if self.slots[slot].is_empty() {
            if right {
                self.slots[slot] = ItemStack {
                    item: self.cursor.item,
                    count: 1,
                    components: self.cursor.components.clone(),
                };
                self.shrink_cursor(1);
            } else {
                self.slots[slot] = std::mem::replace(&mut self.cursor, ItemStack::EMPTY);
            }
        } else if Self::same_item(&self.slots[slot], &self.cursor) {
            let space = max.saturating_sub(self.slots[slot].count);
            if space > 0 {
                let moved = if right { 1 } else { self.cursor.count }.min(space);
                self.slots[slot].count += moved;
                self.shrink_cursor(moved);
            }
        } else {
            std::mem::swap(&mut self.slots[slot], &mut self.cursor);
        }
        Vec::new()
    }

    fn shrink_cursor(&mut self, n: u8) {
        self.cursor.count = self.cursor.count.saturating_sub(n);
        if self.cursor.count == 0 {
            self.cursor = ItemStack::EMPTY;
        }
    }

    /// Merges/fills `stack` into the given slot indices (merge first, then empties).
    fn deposit(&mut self, stack: &mut ItemStack, indices: &[usize]) {
        let max = Self::max_stack(stack);
        for &i in indices {
            if stack.count == 0 {
                return;
            }
            let slot = &mut self.slots[i];
            if !slot.is_empty()
                && slot.item == stack.item
                && slot.components == stack.components
                && slot.count < max
            {
                let moved = (max - slot.count).min(stack.count);
                slot.count += moved;
                stack.count -= moved;
            }
        }
        for &i in indices {
            if stack.count == 0 {
                return;
            }
            if self.slots[i].is_empty() {
                let moved = stack.count.min(max);
                self.slots[i] = ItemStack {
                    item: stack.item,
                    count: moved,
                    components: stack.components.clone(),
                };
                stack.count -= moved;
            }
        }
    }

    fn quick_move(&mut self, from: usize) {
        let mut stack = std::mem::replace(&mut self.slots[from], ItemStack::EMPTY);
        if stack.is_empty() {
            return;
        }
        let targets: Vec<usize> = if (Self::HOTBAR_START..Self::OFFHAND).contains(&from) {
            (Self::MAIN_START..Self::HOTBAR_START).collect()
        } else if (Self::MAIN_START..Self::HOTBAR_START).contains(&from) {
            (Self::HOTBAR_START..Self::OFFHAND).collect()
        } else {
            (Self::MAIN_START..Self::HOTBAR_START)
                .chain(Self::HOTBAR_START..Self::OFFHAND)
                .collect()
        };
        self.deposit(&mut stack, &targets);
        self.slots[from] = stack;
    }

    fn click_swap(&mut self, slot: usize, button: i8) {
        let target = if button == 40 {
            Self::OFFHAND
        } else if (0..=8).contains(&button) {
            Self::hotbar_slot_index(button as u8)
        } else {
            return;
        };
        if target != slot {
            self.slots.swap(slot, target);
        }
    }

    fn click_clone(&mut self, slot: usize) {
        if self.cursor.is_empty() && !self.slots[slot].is_empty() {
            self.cursor = ItemStack {
                item: self.slots[slot].item,
                count: Self::max_stack(&self.slots[slot]),
                components: self.slots[slot].components.clone(),
            };
        }
    }

    fn click_throw(&mut self, slot: usize, button: i8) -> Vec<ItemStack> {
        if self.slots[slot].is_empty() {
            return Vec::new();
        }
        if button == 1 {
            vec![std::mem::replace(&mut self.slots[slot], ItemStack::EMPTY)]
        } else {
            let dropped = ItemStack {
                item: self.slots[slot].item,
                count: 1,
                components: self.slots[slot].components.clone(),
            };
            self.slots[slot].count -= 1;
            if self.slots[slot].count == 0 {
                self.slots[slot] = ItemStack::EMPTY;
            }
            vec![dropped]
        }
    }

    fn drop_cursor(&mut self, single: bool) -> Vec<ItemStack> {
        if self.cursor.is_empty() {
            return Vec::new();
        }
        if single {
            let dropped = ItemStack {
                item: self.cursor.item,
                count: 1,
                components: self.cursor.components.clone(),
            };
            self.shrink_cursor(1);
            vec![dropped]
        } else {
            vec![std::mem::replace(&mut self.cursor, ItemStack::EMPTY)]
        }
    }

    fn pickup_all(&mut self) {
        if self.cursor.is_empty() {
            return;
        }
        let max = Self::max_stack(&self.cursor);
        let item = self.cursor.item;
        let components = self.cursor.components.clone();
        for i in 0..Self::SIZE {
            if self.cursor.count >= max {
                break;
            }
            if i == Self::CRAFTING_OUTPUT {
                continue;
            }
            let slot = &mut self.slots[i];
            if !slot.is_empty() && slot.item == item && slot.components == components {
                let moved = (max - self.cursor.count).min(slot.count);
                self.cursor.count += moved;
                slot.count -= moved;
                if slot.count == 0 {
                    *slot = ItemStack::EMPTY;
                }
            }
        }
    }

    fn quick_craft(&mut self, slot: i16, button: i8) {
        match button {
            0 | 4 | 8 => {
                let kind = match button {
                    0 => DragKind::Left,
                    4 => DragKind::Right,
                    _ => DragKind::Middle,
                };
                self.drag = Some(DragState {
                    kind,
                    slots: Vec::new(),
                });
            }
            1 | 5 | 9 => {
                if slot >= 0
                    && (slot as usize) < Self::SIZE
                    && let Some(drag) = &mut self.drag
                {
                    let idx = slot as usize;
                    if !drag.slots.contains(&idx) {
                        drag.slots.push(idx);
                    }
                }
            }
            2 | 6 | 10 => {
                if let Some(drag) = self.drag.take() {
                    self.apply_drag(drag);
                }
            }
            _ => self.drag = None,
        }
    }

    fn apply_drag(&mut self, drag: DragState) {
        if self.cursor.is_empty() || drag.slots.is_empty() {
            return;
        }
        let max = Self::max_stack(&self.cursor);
        let item = self.cursor.item;
        let components = self.cursor.components.clone();
        let eligible: Vec<usize> = drag
            .slots
            .into_iter()
            .filter(|&i| {
                let s = &self.slots[i];
                s.is_empty() || (s.item == item && s.components == components && s.count < max)
            })
            .collect();
        if eligible.is_empty() {
            return;
        }

        match drag.kind {
            DragKind::Left => {
                let each = self.cursor.count / eligible.len() as u8;
                for i in eligible {
                    if self.cursor.count == 0 || each == 0 {
                        break;
                    }
                    let base = self.slots[i].count;
                    let moved = each.min(max - base).min(self.cursor.count);
                    if moved == 0 {
                        continue;
                    }
                    self.add_to_slot(i, &item, &components, moved);
                    self.cursor.count -= moved;
                }
            }
            DragKind::Right => {
                for i in eligible {
                    if self.cursor.count == 0 {
                        break;
                    }
                    self.add_to_slot(i, &item, &components, 1);
                    self.cursor.count -= 1;
                }
            }
            DragKind::Middle => {
                for i in eligible {
                    self.slots[i] = ItemStack {
                        item,
                        count: max,
                        components: components.clone(),
                    };
                }
            }
        }
        if self.cursor.count == 0 {
            self.cursor = ItemStack::EMPTY;
        }
    }

    fn add_to_slot(&mut self, i: usize, item: &ItemId, components: &[DataComponent], n: u8) {
        if self.slots[i].is_empty() {
            self.slots[i] = ItemStack {
                item: *item,
                count: n,
                components: components.to_vec(),
            };
        } else {
            self.slots[i].count += n;
        }
    }
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotbar_index_mapping() {
        assert_eq!(Inventory::hotbar_slot_index(0), 36);
        assert_eq!(Inventory::hotbar_slot_index(8), 44);
        assert_eq!(Inventory::hotbar_slot_index(20), 44); // clamped
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
        // First give lands in the first hotbar slot.
        let left = inv.give(ItemStack::of("minecraft:stone", 10).unwrap());
        assert!(left.is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 10);

        // Second give stacks onto it up to 64.
        let left = inv.give(ItemStack::of("minecraft:stone", 60).unwrap());
        assert!(left.is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 64);
        // Overflow (6) spills into the next slot.
        assert_eq!(inv.get(Inventory::HOTBAR_START + 1).count, 6);
    }

    #[test]
    fn give_overflows_when_full() {
        let mut inv = Inventory::new();
        // Fill every storage slot (hotbar 9 + main 27 = 36 slots) with full stacks.
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
            crate::item::ItemId::from_name("minecraft:stone").unwrap()
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
        // Left-click the slot: whole stack goes to the cursor.
        inv.apply_click(H0, 0, 0);
        assert!(inv.get(Inventory::HOTBAR_START).is_empty());
        assert_eq!(inv.cursor().count, 10);
        // Left-click an empty slot: cursor drops into it.
        inv.apply_click(M0, 0, 0);
        assert_eq!(inv.get(Inventory::MAIN_START).count, 10);
        assert!(inv.cursor().is_empty());
    }

    #[test]
    fn left_click_merges_same_item() {
        let mut inv = Inventory::new();
        inv.set(Inventory::HOTBAR_START, stone(60));
        inv.set(Inventory::MAIN_START, stone(20));
        inv.apply_click(M0, 0, 0); // pick up 20
        inv.apply_click(H0, 0, 0); // deposit onto the 60 (caps at 64)
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 64);
        assert_eq!(inv.cursor().count, 16); // 20 - 4 fit
    }

    #[test]
    fn right_click_splits_and_places_one() {
        let mut inv = Inventory::new();
        inv.set(Inventory::HOTBAR_START, stone(9));
        inv.apply_click(H0, 1, 0); // right-click: take ceil(9/2) = 5
        assert_eq!(inv.cursor().count, 5);
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 4);
        inv.apply_click(M0, 1, 0); // right-click empty slot: place one
        assert_eq!(inv.get(Inventory::MAIN_START).count, 1);
        assert_eq!(inv.cursor().count, 4);
    }

    #[test]
    fn number_key_swaps_with_hotbar() {
        let mut inv = Inventory::new();
        inv.set(Inventory::MAIN_START, stone(5));
        inv.apply_click(M0, 0, 2); // swap mode, button 0 -> hotbar slot 0
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 5);
        assert!(inv.get(Inventory::MAIN_START).is_empty());
    }

    #[test]
    fn shift_click_moves_main_to_hotbar() {
        let mut inv = Inventory::new();
        inv.set(Inventory::MAIN_START, stone(32));
        inv.apply_click(M0, 0, 1); // quick-move
        assert!(inv.get(Inventory::MAIN_START).is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 32);
    }

    #[test]
    fn throw_drops_items() {
        let mut inv = Inventory::new();
        inv.set(Inventory::HOTBAR_START, stone(5));
        let dropped = inv.apply_click(H0, 1, 4); // throw whole stack
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
        inv.apply_click((Inventory::MAIN_START + 2) as i16, 0, 0); // cursor = 5
        inv.apply_click((Inventory::MAIN_START + 2) as i16, 0, 6); // pickup-all
        assert_eq!(inv.cursor().count, 35);
    }

    #[test]
    fn give_respects_per_item_max_stack() {
        let mut inv = Inventory::new();
        // Ender pearls cap at 16, not 64.
        let left = inv.give(ItemStack::of("minecraft:ender_pearl", 20).unwrap());
        assert!(left.is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START).count, 16);
        assert_eq!(inv.get(Inventory::HOTBAR_START + 1).count, 4);
    }

    #[test]
    fn left_drag_distributes_evenly() {
        let mut inv = Inventory::new();
        inv.set(Inventory::HOTBAR_START, stone(4));
        inv.apply_click(H0, 0, 0); // cursor = 4
        inv.apply_click(-999, 0, 5); // start left drag
        inv.apply_click(M0, 1, 5); // paint slot 9
        inv.apply_click(M0 + 1, 1, 5); // paint slot 10
        inv.apply_click(-999, 2, 5); // end
        assert_eq!(inv.get(Inventory::MAIN_START).count, 2);
        assert_eq!(inv.get(Inventory::MAIN_START + 1).count, 2);
        assert!(inv.cursor().is_empty());
    }
}
