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
use voidmc_protocol::slot::Slot;

use crate::item::ItemStack;

/// A player's 46-slot inventory plus the cursor (drag) item and the selected
/// hotbar index.
#[derive(Component, Clone)]
pub struct Inventory {
    slots: [ItemStack; Self::SIZE],
    cursor: ItemStack,
    selected_hotbar: u8,
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
        let max = Self::DEFAULT_MAX_STACK;

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
}
