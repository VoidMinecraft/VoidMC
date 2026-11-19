//! Vanilla container-click semantics over any slot window: the player
//! inventory (window 0) and open menus (menu slots followed by the player's
//! main inventory and hotbar) share this engine.

use std::ops::Range;

use voidmc_protocol::serverbound::{ClickContainer, ContainerInput};

use voidmc_protocol::slot::DataComponent;

use crate::item::ItemStack;

const VERSION: voidmc_data::Version = voidmc_data::Version::V26_1_2;
const OUTSIDE: i16 = ClickContainer::SLOT_OUTSIDE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    pub size: usize,
    pub container: Range<usize>,
    pub main: Range<usize>,
    pub hotbar: Range<usize>,
    pub offhand: Option<usize>,
    pub crafting_output: Option<usize>,
}

impl Layout {
    pub const PLAYER: Layout = Layout {
        size: 46,
        container: 0..9,
        main: 9..36,
        hotbar: 36..45,
        offhand: Some(45),
        crafting_output: Some(0),
    };

    pub fn menu(slots: usize, with_player_inventory: bool) -> Layout {
        let (main, hotbar) = if with_player_inventory {
            (slots..slots + 27, slots + 27..slots + 36)
        } else {
            (slots..slots, slots..slots)
        };
        Layout {
            size: hotbar.end,
            container: 0..slots,
            main,
            hotbar,
            offhand: None,
            crafting_output: None,
        }
    }

    pub fn is_player_window(&self) -> bool {
        *self == Layout::PLAYER
    }

    fn quick_move_targets(&self, from: usize) -> Vec<usize> {
        if self.is_player_window() {
            // Armor and shields are not routed to their equipment slots: the
            // server has no equipment data for items.
            if self.crafting_output == Some(from) {
                (self.main.start..self.hotbar.end).rev().collect()
            } else if self.hotbar.contains(&from) {
                self.main.clone().collect()
            } else if self.main.contains(&from) {
                self.hotbar.clone().collect()
            } else {
                self.main.clone().chain(self.hotbar.clone()).collect()
            }
        } else if self.container.contains(&from) {
            (self.main.start..self.hotbar.end).rev().collect()
        } else {
            self.container.clone().collect()
        }
    }

    fn swap_target(&self, button: i8) -> Option<usize> {
        if button == 40 {
            self.offhand
        } else if (0..=8).contains(&button) {
            let index = self.hotbar.start + button as usize;
            self.hotbar.contains(&index).then_some(index)
        } else {
            None
        }
    }
}

/// A drag (vanilla "quick craft") in progress: the slots collected so far,
/// in the order the client named them, without repeats.
#[derive(Debug, Clone)]
pub(crate) struct DragState {
    kind: DragKind,
    slots: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Left,
    Right,
    Middle,
}

/// The most items one slot holds of this stack: its `max_stack_size`
/// component if it has one, else the item's default.
pub(crate) fn max_stack(stack: &ItemStack) -> u8 {
    if stack.is_empty() {
        return 64;
    }
    stack
        .components
        .iter()
        .find_map(|component| match component {
            DataComponent::MaxStackSize(max) => Some((*max).clamp(1, 99) as u8),
            _ => None,
        })
        .unwrap_or_else(|| voidmc_data::item_max_stack(VERSION, stack.item.0))
}

fn same_item(a: &ItemStack, b: &ItemStack) -> bool {
    !a.is_empty() && a.item == b.item && a.components == b.components
}

fn with_count(stack: &ItemStack, count: u8) -> ItemStack {
    if count == 0 {
        ItemStack::EMPTY
    } else {
        ItemStack {
            item: stack.item,
            count,
            components: stack.components.clone(),
        }
    }
}

/// Merges `stack` into `indices` (matching stacks first, then empties) and
/// leaves whatever did not fit in `stack`.
pub(crate) fn deposit(slots: &mut [ItemStack], stack: &mut ItemStack, indices: &[usize]) {
    let max = max_stack(stack);
    for &i in indices {
        if stack.count == 0 {
            return;
        }
        let slot = &mut slots[i];
        if same_item(slot, stack) && slot.count < max {
            let moved = (max - slot.count).min(stack.count);
            slot.count += moved;
            stack.count -= moved;
        }
    }
    for &i in indices {
        if stack.count == 0 {
            return;
        }
        if slots[i].is_empty() {
            let moved = stack.count.min(max);
            slots[i] = with_count(stack, moved);
            stack.count -= moved;
        }
    }
}

pub(crate) struct Window<'a> {
    pub slots: &'a mut [ItemStack],
    pub cursor: &'a mut ItemStack,
    pub drag: &'a mut Option<DragState>,
    pub layout: &'a Layout,
    /// Creative players may clone and middle-drag full stacks out of nothing.
    pub creative: bool,
}

impl Window<'_> {
    /// Applies one Container Click with vanilla's rules and returns the stacks
    /// thrown into the world. `slot` is a window index, `-999` (outside the
    /// window) or `-1` (inside the window but on no slot); anything else is
    /// ignored, like an unknown button for the click type.
    pub fn apply_click(&mut self, slot: i16, button: i8, input: ContainerInput) -> Vec<ItemStack> {
        let index = match slot {
            OUTSIDE | -1 => None,
            _ => match usize::try_from(slot) {
                Ok(index) if index < self.layout.size => Some(index),
                _ => return Vec::new(),
            },
        };
        if input == ContainerInput::QuickCraft {
            self.quick_craft(index, button);
            return Vec::new();
        }
        if self.drag.take().is_some() {
            // Any other click during a drag only cancels the drag.
            return Vec::new();
        }
        match input {
            ContainerInput::Pickup | ContainerInput::QuickMove if button == 0 || button == 1 => {
                let right = button == 1;
                match index {
                    None if slot == OUTSIDE => return self.drop_cursor(right),
                    None => {}
                    Some(s) if input == ContainerInput::QuickMove => self.quick_move(s),
                    Some(s) => self.pickup(s, right),
                }
            }
            ContainerInput::Swap => {
                if let Some(s) = index {
                    self.swap(s, button);
                }
            }
            ContainerInput::Clone => {
                if let Some(s) = index {
                    self.clone_stack(s);
                }
            }
            ContainerInput::Throw => {
                if let Some(s) = index {
                    return self.throw(s, button);
                }
            }
            ContainerInput::PickupAll => {
                if let Some(s) = index {
                    self.pickup_all(s, button);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// Whether the player may put items into `slot` (not the crafting output).
    fn may_place(&self, slot: usize) -> bool {
        self.layout.crafting_output != Some(slot)
    }

    fn pickup(&mut self, slot: usize, right: bool) {
        let may_place = self.may_place(slot);
        let cursor = &mut *self.cursor;
        let target = &mut self.slots[slot];
        if target.is_empty() {
            if !cursor.is_empty() && may_place {
                let placed = if right { 1 } else { cursor.count }.min(max_stack(cursor));
                *target = with_count(cursor, placed);
                *cursor = with_count(cursor, cursor.count - placed);
            }
        } else if cursor.is_empty() {
            let total = target.count;
            let take = if right { total.div_ceil(2) } else { total };
            *cursor = with_count(target, take);
            *target = with_count(target, total - take);
        } else if may_place {
            if same_item(target, cursor) {
                let space = max_stack(cursor).saturating_sub(target.count);
                let moved = if right { 1 } else { cursor.count }.min(space);
                target.count += moved;
                *cursor = with_count(cursor, cursor.count - moved);
            } else if cursor.count <= max_stack(cursor) {
                std::mem::swap(target, cursor);
            }
        } else if same_item(target, cursor)
            && target.count <= max_stack(cursor).saturating_sub(cursor.count)
        {
            // An output slot only gives its whole stack, if the cursor has room.
            cursor.count += target.count;
            *target = ItemStack::EMPTY;
        }
    }

    fn quick_move(&mut self, from: usize) {
        let mut stack = std::mem::replace(&mut self.slots[from], ItemStack::EMPTY);
        if stack.is_empty() {
            return;
        }
        let targets = self.layout.quick_move_targets(from);
        deposit(self.slots, &mut stack, &targets);
        self.slots[from] = stack;
    }

    fn swap(&mut self, slot: usize, button: i8) {
        let Some(hotbar) = self.layout.swap_target(button) else {
            return;
        };
        if hotbar == slot || (!self.slots[hotbar].is_empty() && !self.may_place(slot)) {
            return;
        }
        self.slots.swap(slot, hotbar);
    }

    fn clone_stack(&mut self, slot: usize) {
        if self.creative && self.cursor.is_empty() && !self.slots[slot].is_empty() {
            *self.cursor = with_count(&self.slots[slot], max_stack(&self.slots[slot]));
        }
    }

    /// The drop key over a slot: button 0 throws one item, any other button
    /// (Ctrl+Q) the whole stack. Nothing happens while the cursor holds items.
    fn throw(&mut self, slot: usize, button: i8) -> Vec<ItemStack> {
        let target = &mut self.slots[slot];
        if target.is_empty() || !self.cursor.is_empty() {
            return Vec::new();
        }
        if button == 0 {
            let dropped = with_count(target, 1);
            *target = with_count(target, target.count - 1);
            vec![dropped]
        } else {
            vec![std::mem::replace(target, ItemStack::EMPTY)]
        }
    }

    fn drop_cursor(&mut self, single: bool) -> Vec<ItemStack> {
        if self.cursor.is_empty() {
            return Vec::new();
        }
        if single {
            let dropped = with_count(self.cursor, 1);
            *self.cursor = with_count(self.cursor, self.cursor.count - 1);
            vec![dropped]
        } else {
            vec![std::mem::replace(self.cursor, ItemStack::EMPTY)]
        }
    }

    /// Double-click: fills the cursor with matching items, scanning the window
    /// from the first slot (button 0) or the last, partial stacks before full
    /// ones. Only when the clicked slot is empty, as after the first click.
    fn pickup_all(&mut self, slot: usize, button: i8) {
        if self.cursor.is_empty() || !self.slots[slot].is_empty() {
            return;
        }
        let max = max_stack(self.cursor);
        let order: Vec<usize> = if button == 0 {
            (0..self.layout.size).collect()
        } else {
            (0..self.layout.size).rev().collect()
        };
        for full_stacks in [false, true] {
            for &i in &order {
                if self.cursor.count >= max {
                    return;
                }
                let slot = &mut self.slots[i];
                if self.layout.crafting_output == Some(i)
                    || !same_item(slot, self.cursor)
                    || (!full_stacks && slot.count >= max_stack(slot))
                {
                    continue;
                }
                let moved = (max - self.cursor.count).min(slot.count);
                self.cursor.count += moved;
                *slot = with_count(slot, slot.count - moved);
            }
        }
    }

    /// Whether a drag carrying the cursor stack may cover `slot`: an empty
    /// slot or one holding the same item (even a full one).
    fn can_drag_to(&self, slot: usize) -> bool {
        let target = &self.slots[slot];
        self.may_place(slot) && (target.is_empty() || same_item(target, self.cursor))
    }

    /// Vanilla's drag protocol: a start packet (header 0), one packet per
    /// slot (header 1), an end packet (header 2). Any packet out of that
    /// order, or arriving with an empty cursor, cancels the drag.
    fn quick_craft(&mut self, slot: Option<usize>, button: i8) {
        let header = button & 3;
        let collecting = self.drag.is_some();
        let in_order = match header {
            0 => !collecting,
            1 | 2 => collecting,
            _ => false,
        };
        if !in_order || self.cursor.is_empty() {
            *self.drag = None;
            return;
        }
        match header {
            0 => {
                let kind = match (button >> 2) & 3 {
                    0 => Some(DragKind::Left),
                    1 => Some(DragKind::Right),
                    2 if self.creative => Some(DragKind::Middle),
                    _ => None,
                };
                *self.drag = kind.map(|kind| DragState {
                    kind,
                    slots: Vec::new(),
                });
            }
            1 => {
                let Some(index) = slot else {
                    return;
                };
                let eligible = self.can_drag_to(index);
                let count = usize::from(self.cursor.count);
                if let Some(drag) = self.drag.as_mut()
                    && eligible
                    && !drag.slots.contains(&index)
                    && (drag.kind == DragKind::Middle || count > drag.slots.len())
                {
                    drag.slots.push(index);
                }
            }
            _ => {
                if let Some(drag) = self.drag.take() {
                    self.apply_drag(drag);
                }
            }
        }
    }

    fn apply_drag(&mut self, drag: DragState) {
        match drag.slots[..] {
            [] => return,
            [only] => {
                // One slot is a plain click; a middle drag places nothing.
                if drag.kind != DragKind::Middle {
                    self.pickup(only, drag.kind == DragKind::Right);
                }
                return;
            }
            _ => {}
        }
        let carried = self.cursor.clone();
        let max = usize::from(max_stack(&carried));
        let total = usize::from(carried.count);
        let per_slot = match drag.kind {
            DragKind::Left => total / drag.slots.len(),
            DragKind::Right => 1,
            DragKind::Middle => max,
        };
        let mut remaining = total;
        for &i in &drag.slots {
            if !self.can_drag_to(i) || (drag.kind != DragKind::Middle && total < drag.slots.len()) {
                continue;
            }
            let existing = if self.slots[i].is_empty() {
                0
            } else {
                usize::from(self.slots[i].count)
            };
            let placed = (per_slot + existing).min(max).max(existing);
            remaining = remaining.saturating_sub(placed - existing);
            self.slots[i] = with_count(&carried, placed as u8);
        }
        if drag.kind != DragKind::Middle {
            *self.cursor = with_count(&carried, remaining as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stone(n: u8) -> ItemStack {
        ItemStack::of("minecraft:stone", n).unwrap()
    }

    fn run(
        layout: &Layout,
        slots: &mut [ItemStack],
        cursor: &mut ItemStack,
        clicks: &[(i16, i8, ContainerInput)],
    ) -> Vec<ItemStack> {
        let mut drag = None;
        let mut window = Window {
            slots,
            cursor,
            drag: &mut drag,
            layout,
            creative: false,
        };
        clicks
            .iter()
            .flat_map(|&(slot, button, input)| window.apply_click(slot, button, input))
            .collect()
    }

    #[test]
    fn menu_layout_places_player_inventory_after_menu_slots() {
        let layout = Layout::menu(27, true);
        assert_eq!(layout.size, 63);
        assert_eq!(layout.main, 27..54);
        assert_eq!(layout.hotbar, 54..63);
        assert_eq!(layout.offhand, None);
        let lectern = Layout::menu(1, false);
        assert_eq!(lectern.size, 1);
        assert!(lectern.main.is_empty());
    }

    #[test]
    fn menu_quick_move_fills_hotbar_from_the_right_then_main() {
        let layout = Layout::menu(9, true);
        let mut slots = vec![ItemStack::EMPTY; layout.size];
        slots[0] = stone(5);
        let mut cursor = ItemStack::EMPTY;
        run(
            &layout,
            &mut slots,
            &mut cursor,
            &[(0, 0, ContainerInput::QuickMove)],
        );
        assert!(slots[0].is_empty());
        assert_eq!(slots[layout.hotbar.end - 1].count, 5);

        slots[layout.hotbar.end - 1] = ItemStack::EMPTY;
        slots[layout.main.start] = stone(3);
        run(
            &layout,
            &mut slots,
            &mut cursor,
            &[(layout.main.start as i16, 0, ContainerInput::QuickMove)],
        );
        assert_eq!(slots[0].count, 3);
    }

    #[test]
    fn menu_swap_ignores_offhand_button() {
        let layout = Layout::menu(9, true);
        let mut slots = vec![ItemStack::EMPTY; layout.size];
        slots[2] = stone(1);
        let mut cursor = ItemStack::EMPTY;
        run(
            &layout,
            &mut slots,
            &mut cursor,
            &[(2, 40, ContainerInput::Swap), (2, 3, ContainerInput::Swap)],
        );
        assert_eq!(slots[layout.hotbar.start + 3].count, 1);
        assert!(slots[2].is_empty());
    }

    #[test]
    fn menu_without_player_inventory_rejects_out_of_range_slots() {
        let layout = Layout::menu(1, false);
        let mut slots = vec![stone(4)];
        let mut cursor = ItemStack::EMPTY;
        run(
            &layout,
            &mut slots,
            &mut cursor,
            &[
                (5, 0, ContainerInput::Pickup),
                (0, 0, ContainerInput::Pickup),
            ],
        );
        assert_eq!(cursor.count, 4);
    }

    #[test]
    fn clone_and_middle_drag_need_creative() {
        let layout = Layout::menu(9, true);
        let mut slots = vec![ItemStack::EMPTY; layout.size];
        slots[0] = stone(1);
        let mut cursor = ItemStack::EMPTY;
        run(
            &layout,
            &mut slots,
            &mut cursor,
            &[(0, 2, ContainerInput::Clone)],
        );
        assert!(cursor.is_empty());

        cursor = stone(1);
        run(
            &layout,
            &mut slots,
            &mut cursor,
            &[
                (-999, 8, ContainerInput::QuickCraft),
                (1, 9, ContainerInput::QuickCraft),
                (-999, 10, ContainerInput::QuickCraft),
            ],
        );
        assert!(slots[1].is_empty());
        assert_eq!(cursor.count, 1);

        let mut drag = None;
        let mut window = Window {
            slots: &mut slots,
            cursor: &mut cursor,
            drag: &mut drag,
            layout: &layout,
            creative: true,
        };
        window.apply_click(-999, 8, ContainerInput::QuickCraft);
        window.apply_click(1, 9, ContainerInput::QuickCraft);
        window.apply_click(2, 9, ContainerInput::QuickCraft);
        window.apply_click(-999, 10, ContainerInput::QuickCraft);
        assert_eq!(window.slots[1].count, 64);
        assert_eq!(window.slots[2].count, 64);
        assert_eq!(window.cursor.count, 1);
        *window.cursor = ItemStack::EMPTY;
        window.apply_click(0, 2, ContainerInput::Clone);
        assert_eq!(window.cursor.count, 64);
    }

    #[test]
    fn max_stack_size_component_caps_merges_and_drags() {
        let layout = Layout::PLAYER;
        let small = |n: u8| stone(n).with_max_stack_size(16);
        let mut slots = vec![ItemStack::EMPTY; layout.size];
        slots[9] = small(10);
        let mut cursor = small(10);
        run(
            &layout,
            &mut slots,
            &mut cursor,
            &[(9, 0, ContainerInput::Pickup)],
        );
        assert_eq!((slots[9].count, cursor.count), (16, 4));
        cursor = small(16);
        run(
            &layout,
            &mut slots,
            &mut cursor,
            &[
                (-999, 0, ContainerInput::QuickCraft),
                (9, 1, ContainerInput::QuickCraft),
                (10, 1, ContainerInput::QuickCraft),
                (-999, 2, ContainerInput::QuickCraft),
            ],
        );
        assert_eq!((slots[9].count, slots[10].count, cursor.count), (16, 8, 8));
    }

    #[test]
    fn negative_slots_other_than_outside_are_ignored_and_reset_drag() {
        let layout = Layout::menu(9, true);
        let mut slots = vec![ItemStack::EMPTY; layout.size];
        let mut cursor = stone(4);
        run(
            &layout,
            &mut slots,
            &mut cursor,
            &[(-1, 0, ContainerInput::Pickup)],
        );
        assert_eq!(cursor.count, 4);

        let mut drag = None;
        let mut window = Window {
            slots: &mut slots,
            cursor: &mut cursor,
            drag: &mut drag,
            layout: &layout,
            creative: false,
        };
        window.apply_click(-999, 0, ContainerInput::QuickCraft);
        window.apply_click(1, 1, ContainerInput::QuickCraft);
        window.apply_click(5, 0, ContainerInput::QuickMove);
        assert!(window.drag.is_none());
        window.apply_click(-999, 2, ContainerInput::QuickCraft);
        assert!(window.slots[1].is_empty());
        assert_eq!(window.cursor.count, 4);
    }
}
