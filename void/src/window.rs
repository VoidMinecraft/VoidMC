//! Vanilla container-click semantics over any slot window: the player
//! inventory (window 0) and open menus (menu slots followed by the player's
//! main inventory and hotbar) share this engine.

use std::ops::Range;

use voidmc_protocol::serverbound::{ClickContainer, ContainerInput};

use crate::item::{ItemId, ItemStack};

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
            if self.hotbar.contains(&from) {
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

#[derive(Debug, Clone)]
pub(crate) struct DragState {
    kind: DragKind,
    slots: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
enum DragKind {
    Left,
    Right,
    Middle,
}

pub(crate) fn max_stack(stack: &ItemStack) -> u8 {
    if stack.is_empty() {
        return 64;
    }
    voidmc_data::item_max_stack(VERSION, stack.item.0)
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
    /// Applies one Container Click and returns the stacks thrown into the world.
    /// `slot` is a window index or `-999` (outside); anything else is ignored.
    pub fn apply_click(&mut self, slot: i16, button: i8, input: ContainerInput) -> Vec<ItemStack> {
        if slot < 0 && slot != OUTSIDE {
            return Vec::new();
        }
        if input != ContainerInput::QuickCraft {
            *self.drag = None;
        }
        let valid = (slot >= 0 && (slot as usize) < self.layout.size).then_some(slot as usize);
        match input {
            ContainerInput::Pickup => match valid {
                Some(s) => {
                    self.pickup(s, button == 1);
                    Vec::new()
                }
                None => self.drop_cursor(button == 1),
            },
            ContainerInput::QuickMove => {
                if let Some(s) = valid {
                    self.quick_move(s);
                }
                Vec::new()
            }
            ContainerInput::Swap => {
                if let Some(s) = valid {
                    self.swap(s, button);
                }
                Vec::new()
            }
            ContainerInput::Clone => {
                if let Some(s) = valid {
                    self.clone_stack(s);
                }
                Vec::new()
            }
            ContainerInput::Throw => match valid {
                Some(s) => self.throw(s, button),
                None => Vec::new(),
            },
            ContainerInput::QuickCraft => {
                self.quick_craft(valid, button);
                Vec::new()
            }
            ContainerInput::PickupAll => {
                if valid.is_some() {
                    self.pickup_all();
                }
                Vec::new()
            }
        }
    }

    fn pickup(&mut self, slot: usize, right: bool) {
        let cursor = &mut *self.cursor;
        let target = &mut self.slots[slot];
        if cursor.is_empty() {
            if target.is_empty() {
                return;
            }
            if right {
                let total = target.count;
                let take = total.div_ceil(2);
                *cursor = with_count(target, take);
                *target = with_count(target, total - take);
            } else {
                *cursor = std::mem::replace(target, ItemStack::EMPTY);
            }
        } else if target.is_empty() {
            if right {
                *target = with_count(cursor, 1);
                *cursor = with_count(cursor, cursor.count - 1);
            } else {
                *target = std::mem::replace(cursor, ItemStack::EMPTY);
            }
        } else if same_item(target, cursor) {
            let space = max_stack(cursor).saturating_sub(target.count);
            if space > 0 {
                let moved = if right { 1 } else { cursor.count }.min(space);
                target.count += moved;
                *cursor = with_count(cursor, cursor.count - moved);
            }
        } else {
            std::mem::swap(target, cursor);
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
        if let Some(target) = self.layout.swap_target(button)
            && target != slot
        {
            self.slots.swap(slot, target);
        }
    }

    fn clone_stack(&mut self, slot: usize) {
        if self.creative && self.cursor.is_empty() && !self.slots[slot].is_empty() {
            *self.cursor = with_count(&self.slots[slot], max_stack(&self.slots[slot]));
        }
    }

    fn throw(&mut self, slot: usize, button: i8) -> Vec<ItemStack> {
        let target = &mut self.slots[slot];
        if target.is_empty() {
            return Vec::new();
        }
        if button == 1 {
            vec![std::mem::replace(target, ItemStack::EMPTY)]
        } else {
            let dropped = with_count(target, 1);
            *target = with_count(target, target.count - 1);
            vec![dropped]
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

    fn pickup_all(&mut self) {
        if self.cursor.is_empty() {
            return;
        }
        let max = max_stack(self.cursor);
        for i in 0..self.layout.size {
            if self.cursor.count >= max {
                break;
            }
            if self.layout.crafting_output == Some(i) {
                continue;
            }
            let slot = &mut self.slots[i];
            if same_item(slot, self.cursor) {
                let moved = (max - self.cursor.count).min(slot.count);
                self.cursor.count += moved;
                *slot = with_count(slot, slot.count - moved);
            }
        }
    }

    fn quick_craft(&mut self, slot: Option<usize>, button: i8) {
        match button {
            8 if !self.creative => *self.drag = None,
            0 | 4 | 8 => {
                let kind = match button {
                    0 => DragKind::Left,
                    4 => DragKind::Right,
                    _ => DragKind::Middle,
                };
                *self.drag = Some(DragState {
                    kind,
                    slots: Vec::new(),
                });
            }
            1 | 5 | 9 => {
                if let (Some(idx), Some(drag)) = (slot, self.drag.as_mut())
                    && !drag.slots.contains(&idx)
                {
                    drag.slots.push(idx);
                }
            }
            2 | 6 | 10 => {
                if let Some(drag) = self.drag.take() {
                    self.apply_drag(drag);
                }
            }
            _ => *self.drag = None,
        }
    }

    fn apply_drag(&mut self, drag: DragState) {
        if self.cursor.is_empty() || drag.slots.is_empty() {
            return;
        }
        let max = max_stack(self.cursor);
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
                    let moved = each.min(max - self.slots[i].count).min(self.cursor.count);
                    if moved == 0 {
                        continue;
                    }
                    self.add_to_slot(i, item, &components, moved);
                    self.cursor.count -= moved;
                }
            }
            DragKind::Right => {
                for i in eligible {
                    if self.cursor.count == 0 {
                        break;
                    }
                    self.add_to_slot(i, item, &components, 1);
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
            *self.cursor = ItemStack::EMPTY;
        }
    }

    fn add_to_slot(
        &mut self,
        i: usize,
        item: ItemId,
        components: &[voidmc_protocol::slot::DataComponent],
        n: u8,
    ) {
        if self.slots[i].is_empty() {
            self.slots[i] = ItemStack {
                item,
                count: n,
                components: components.to_vec(),
            };
        } else {
            self.slots[i].count += n;
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
        window.apply_click(-999, 10, ContainerInput::QuickCraft);
        assert_eq!(window.slots[1].count, 64);
        *window.cursor = ItemStack::EMPTY;
        window.apply_click(0, 2, ContainerInput::Clone);
        assert_eq!(window.cursor.count, 64);
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
