//! Vanilla's click rules for the player inventory (window 0), written in the
//! shape of the vanilla code (`AbstractContainerMenu.doClick` and
//! `InventoryMenu.quickMoveStack`), independently from VoidMC.
//!
//! A [`Bot`](crate::Bot) predicts its clicks with them, as the vanilla client
//! does before it sends a click, and its [`ClientView`](crate::ClientView)
//! shows the prediction at once; a scenario can also run an
//! [`InventoryMenu`] of its own as the state it expects the server to reach.
//!
//! Item stacks are modelled as a kind and a count: components are not. Each
//! item's maximum stack size and equipment slot come from Azalea's default
//! components, a source independent from VoidMC's data.

use azalea_inventory::ItemStack;
use azalea_inventory::components::{EquipmentSlot, Equippable, MaxStackSize};
use azalea_inventory::default_components::get_default_component;
use azalea_inventory::operations::ClickType;
use azalea_protocol::packets::game::s_container_click::{
    HashedActualItem, HashedPatchMap, HashedStack, ServerboundContainerClick,
};
use azalea_registry::builtin::ItemKind;

/// Slots of the player inventory window: the crafting result (0), the
/// crafting grid (1 to 4), the armor (5 to 8), the main inventory (9 to 35),
/// the hotbar (36 to 44) and the off-hand (45).
pub const SIZE: usize = 46;
/// The slot number of a click outside the window.
pub const OUTSIDE: i16 = -999;
pub const RESULT: usize = 0;
pub const HOTBAR: usize = 36;
pub const OFFHAND: usize = 45;

/// A slot's contents as the rules see them: a kind and a count, `None` for
/// an empty slot.
pub type Stack = Option<(ItemKind, i32)>;

/// The item's maximum stack size, from its default `max_stack_size`.
pub fn max_stack(kind: ItemKind) -> i32 {
    get_default_component::<MaxStackSize>(kind).map_or(64, |max| max.count)
}

/// `LivingEntity.getEquipmentSlotForItem` for a player: the slot of the
/// item's `equippable` component if a player can use it (not `body` or
/// `saddle`), else the main hand.
pub fn equipment_slot(kind: ItemKind) -> EquipmentSlot {
    match get_default_component::<Equippable>(kind).map(|equippable| equippable.slot) {
        Some(EquipmentSlot::Body | EquipmentSlot::Saddle) | None => EquipmentSlot::Mainhand,
        Some(slot) => slot,
    }
}

/// The window slot of each armor equipment slot (`8 - index`): head 5,
/// chest 6, legs 7, feet 8.
pub fn armor_window_slot(slot: EquipmentSlot) -> Option<usize> {
    match slot {
        EquipmentSlot::Head => Some(5),
        EquipmentSlot::Chest => Some(6),
        EquipmentSlot::Legs => Some(7),
        EquipmentSlot::Feet => Some(8),
        _ => None,
    }
}

/// `count` × `kind`, or the empty slot for a count of zero or less.
pub fn stack(kind: ItemKind, count: i32) -> Stack {
    (count > 0).then_some((kind, count))
}

pub fn count(stack: Stack) -> i32 {
    stack.map_or(0, |(_, count)| count)
}

/// A client item stack as the rules see it.
pub fn from_client(item: &ItemStack) -> Stack {
    match item {
        ItemStack::Empty => None,
        ItemStack::Present(data) => stack(data.kind, data.count),
    }
}

/// `stack` as a client item stack. It keeps the components of `like` if
/// that is the same item, and has the item's default components otherwise.
pub fn to_client(stack: Stack, like: &ItemStack) -> ItemStack {
    match (stack, like) {
        (None, _) => ItemStack::Empty,
        (Some((kind, count)), ItemStack::Present(data)) if data.kind == kind => {
            let mut data = data.clone();
            data.count = count;
            ItemStack::Present(data)
        }
        (Some((kind, count)), _) => ItemStack::new(kind, count),
    }
}

/// `stack` as a click packet carries it, with no component changes.
pub fn hashed(stack: Stack) -> HashedStack {
    HashedStack(stack.map(|(kind, count)| HashedActualItem {
        kind,
        count,
        components: HashedPatchMap {
            added_components: Vec::new(),
            removed_components: Vec::new(),
        },
    }))
}

/// Vanilla's click rules for window 0. Deliberate difference, documented in
/// VoidMC: a creative middle-drag keeps the cursor stack.
#[derive(Clone, Debug, PartialEq)]
pub struct InventoryMenu {
    pub slots: Vec<Stack>,
    pub carried: Stack,
    /// The drag in progress (`quickcraftStatus`, `quickcraftType` and
    /// `quickcraftSlots`), which lasts across clicks.
    pub quickcraft_status: i32,
    pub quickcraft_type: i32,
    pub quickcraft_slots: Vec<usize>,
    /// Whether the player is in creative mode, which allows cloning and
    /// middle drags.
    pub creative: bool,
    /// The stacks the clicks threw, in order.
    pub drops: Vec<(ItemKind, i32)>,
}

impl Default for InventoryMenu {
    fn default() -> Self {
        Self::new(false)
    }
}

impl InventoryMenu {
    pub fn new(creative: bool) -> Self {
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

    /// `Slot.mayPlace`: nothing goes into the crafting output, and an armor
    /// slot (`ArmorSlot`) only takes an item equippable in it.
    fn may_place(&self, index: usize, kind: ItemKind) -> bool {
        match index {
            RESULT => false,
            5..=8 => armor_window_slot(equipment_slot(kind)) == Some(index),
            _ => true,
        }
    }

    /// `Slot.getMaxStackSize(stack)`: an armor slot holds one item.
    fn slot_max(&self, index: usize, kind: ItemKind) -> i32 {
        if (5..=8).contains(&index) {
            1
        } else {
            max_stack(kind)
        }
    }

    /// `Inventory.add`: tops up the selected slot (always the first hotbar
    /// slot here), the off-hand, then the hotbar and the main inventory,
    /// fills the first empty slot of the hotbar then the main inventory, and
    /// drops what is left.
    fn add_to_inventory(&mut self, (kind, mut left): (ItemKind, i32)) {
        let max = max_stack(kind);
        let merge = [HOTBAR, OFFHAND]
            .into_iter()
            .chain(HOTBAR..OFFHAND)
            .chain(9..HOTBAR);
        for i in merge {
            if let Some((held, count)) = self.slots[i]
                && held == kind
                && count < max
            {
                let moved = left.min(max - count);
                self.slots[i] = stack(kind, count + moved);
                left -= moved;
            }
        }
        for i in (HOTBAR..OFFHAND).chain(9..HOTBAR) {
            if left > 0 && self.slots[i].is_none() {
                let placed = left.min(max);
                self.slots[i] = stack(kind, placed);
                left -= placed;
            }
        }
        if left > 0 {
            self.drops.push((kind, left));
        }
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
    pub fn click(&mut self, slot: i16, button: u8, mode: ClickType) {
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
                    && self.may_place(index, carried.0)
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
                    let mut remaining = carried.1;
                    for index in self.quickcraft_slots.clone() {
                        if self.can_item_quick_replace(index, carried)
                            && self.may_place(index, carried.0)
                            && (self.quickcraft_type == 2 || carried.1 >= slots)
                        {
                            let existing = count(self.slots[index]);
                            let max = self.slot_max(index, carried.0);
                            let place = match self.quickcraft_type {
                                0 => carried.1 / slots,
                                1 => 1,
                                _ => max_stack(carried.0),
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
            (Some(held), Some(carried)) if self.may_place(index, carried.0) => {
                if held.0 == carried.0 {
                    let increment = if button == 0 { carried.1 } else { 1 };
                    self.carried = self.safe_insert(index, carried, increment);
                } else if carried.1 <= self.slot_max(index, carried.0) {
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
        if !self.may_place(index, carried.0) {
            return Some(carried);
        }
        let existing = count(self.slots[index]);
        let moved = increment
            .min(carried.1)
            .min(self.slot_max(index, carried.0) - existing);
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
    /// the slot still holds the same item. Outside the crafting grid and the
    /// armor slots, an item equippable in an empty armor slot or the empty
    /// off-hand goes there first.
    fn quick_move(&mut self, index: usize) {
        loop {
            let Some((kind, before)) = self.slots[index] else {
                return;
            };
            let equipment = equipment_slot(kind);
            let armor = armor_window_slot(equipment).filter(|&slot| self.slots[slot].is_none());
            let (start, end, reverse) = if index == RESULT {
                (9, 45, true)
            } else if (1..=8).contains(&index) {
                (9, 45, false)
            } else if let Some(slot) = armor {
                (slot, slot + 1, false)
            } else if equipment == EquipmentSlot::Offhand && self.slots[OFFHAND].is_none() {
                (OFFHAND, OFFHAND + 1, false)
            } else if (9..=35).contains(&index) {
                (36, 45, false)
            } else if (36..=44).contains(&index) {
                (9, 36, false)
            } else {
                (9, 45, false)
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
                    let max = self.slot_max(i, kind);
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
                if self.slots[i].is_none() && self.may_place(i, kind) {
                    let placed = moving.min(self.slot_max(i, kind));
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

    /// A number key (or the off-hand key, 40) over a slot. A stack larger
    /// than the slot allows leaves only what fits, and what that slot held
    /// goes back into the inventory.
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
            (Some((kind, count)), None) => {
                if self.may_place(index, kind) {
                    let max = self.slot_max(index, kind);
                    if count > max {
                        self.slots[index] = stack(kind, max);
                        self.slots[hotbar] = stack(kind, count - max);
                    } else {
                        self.slots[index] = held;
                        self.slots[hotbar] = None;
                    }
                }
            }
            (Some((kind, count)), Some(old)) => {
                if self.may_place(index, kind) {
                    let max = self.slot_max(index, kind);
                    if count > max {
                        self.slots[index] = stack(kind, max);
                        self.slots[hotbar] = stack(kind, count - max);
                        self.add_to_inventory(old);
                    } else {
                        self.slots[hotbar] = item;
                        self.slots[index] = held;
                    }
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

/// A click in the player inventory and the client's prediction of it: what
/// a [`Bot`](crate::Bot) sent, and showed in its view as it sent it.
#[derive(Clone, Debug, PartialEq)]
pub struct Click {
    /// The state id the click is sent with: the last one the client received.
    pub state_id: u32,
    pub slot: i16,
    pub button: u8,
    pub mode: ClickType,
    /// The slots the client predicts the click changes, with their new
    /// contents.
    pub changed: Vec<(u16, Stack)>,
    /// The cursor the client predicts.
    pub carried: Stack,
    /// What the client expects the click to throw.
    pub drops: Vec<(ItemKind, i32)>,
}

impl Click {
    /// The packet a vanilla client sends for this click.
    pub fn packet(&self) -> ServerboundContainerClick {
        ServerboundContainerClick {
            container_id: 0,
            state_id: self.state_id,
            slot_num: self.slot,
            button_num: self.button,
            click_type: self.mode,
            changed_slots: self
                .changed
                .iter()
                .map(|&(slot, stack)| (slot, hashed(stack)))
                .collect(),
            carried_item: hashed(self.carried),
        }
    }
}
