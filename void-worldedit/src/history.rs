use std::collections::VecDeque;
use std::sync::Arc;

use crate::block::BlockState;
use crate::extent::{ChangeMask, SectionBlocks};
use crate::math::{SECTION_VOLUME, SectionPos};

const UNCHANGED: u32 = u32::MAX;

/// The changes of one section as runs over its 4096 indices: each run is
/// either untouched or turns one state into another, so filling a section
/// of air with stone costs a single run.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SectionChange {
    pos: SectionPos,
    lengths: Vec<u16>,
    old: Vec<u32>,
    new: Vec<u32>,
}

impl SectionChange {
    fn record(pos: SectionPos, before: &SectionBlocks, after: &SectionBlocks) -> Self {
        let mut change = Self {
            pos,
            lengths: Vec::new(),
            old: Vec::new(),
            new: Vec::new(),
        };
        for index in 0..SECTION_VOLUME {
            let (b, a) = (before[index].0, after[index].0);
            let (old, new) = if b == a {
                (UNCHANGED, UNCHANGED)
            } else {
                (b, a)
            };
            match change.lengths.last_mut() {
                Some(length)
                    if *change.old.last().unwrap() == old && *change.new.last().unwrap() == new =>
                {
                    *length += 1;
                }
                _ => {
                    change.lengths.push(1);
                    change.old.push(old);
                    change.new.push(new);
                }
            }
        }
        change
    }

    fn apply(&self, blocks: &mut SectionBlocks, side: Side) {
        let values = match side {
            Side::Before => &self.old,
            Side::After => &self.new,
        };
        let slots = blocks.as_mut_slice();
        let mut index = 0;
        for (length, value) in self.lengths.iter().zip(values) {
            let end = index + usize::from(*length);
            if *value != UNCHANGED {
                slots[index..end].fill(BlockState(*value));
            }
            index = end;
        }
    }

    #[cfg(test)]
    fn changed(&self) -> u64 {
        self.lengths
            .iter()
            .zip(&self.old)
            .filter(|(_, old)| **old != UNCHANGED)
            .map(|(length, _)| u64::from(*length))
            .sum()
    }

    fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.lengths.capacity() * 2 + self.old.capacity() * 8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    sections: Vec<SectionChange>,
    changed: u64,
}

impl ChangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &mut self,
        pos: SectionPos,
        before: &SectionBlocks,
        after: &SectionBlocks,
        changed: &ChangeMask,
    ) {
        if changed.is_empty() {
            return;
        }
        self.changed += changed.count() as u64;
        self.sections
            .push(SectionChange::record(pos, before, after));
    }

    pub fn changed_blocks(&self) -> u64 {
        self.changed
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    pub fn sections(&self) -> impl Iterator<Item = SectionPos> + '_ {
        self.sections.iter().map(|change| change.pos)
    }

    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .sections
                .iter()
                .map(SectionChange::memory_bytes)
                .sum::<usize>()
    }

    pub(crate) fn apply_section(&self, index: usize, blocks: &mut SectionBlocks, side: Side) {
        self.sections[index].apply(blocks, side);
    }

    pub(crate) fn section_count(&self) -> usize {
        self.sections.len()
    }

    #[cfg(test)]
    fn recount(&self) -> u64 {
        self.sections.iter().map(SectionChange::changed).sum()
    }
}

/// A bounded undo/redo stack. Old entries fall off once either the entry
/// count or the memory budget is exceeded. `T` tags each entry with whatever
/// the host needs to replay it, such as the world it was made in.
#[derive(Debug)]
pub struct History<T = ()> {
    undo: VecDeque<(Arc<ChangeSet>, T)>,
    redo: Vec<(Arc<ChangeSet>, T)>,
    max_entries: usize,
    max_bytes: usize,
    bytes: usize,
}

impl<T> History<T> {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            max_entries: max_entries.max(1),
            max_bytes,
            bytes: 0,
        }
    }

    /// Records a fresh edit; it invalidates the redo stack.
    pub fn push(&mut self, changes: ChangeSet, tag: T) {
        if changes.is_empty() {
            return;
        }
        self.redo.clear();
        self.push_undo(Arc::new(changes), tag);
    }

    pub fn pop_undo(&mut self) -> Option<(Arc<ChangeSet>, T)> {
        let entry = self.undo.pop_back()?;
        self.bytes -= entry.0.memory_bytes();
        Some(entry)
    }

    pub fn push_redo(&mut self, changes: Arc<ChangeSet>, tag: T) {
        self.redo.push((changes, tag));
    }

    pub fn pop_redo(&mut self) -> Option<(Arc<ChangeSet>, T)> {
        self.redo.pop()
    }

    /// Stores a redone edit back on the undo stack, keeping the redo stack.
    pub fn push_undo(&mut self, changes: Arc<ChangeSet>, tag: T) {
        self.bytes += changes.memory_bytes();
        self.undo.push_back((changes, tag));
        while self.undo.len() > self.max_entries
            || (self.bytes > self.max_bytes && self.undo.len() > 1)
        {
            if let Some((dropped, _)) = self.undo.pop_front() {
                self.bytes -= dropped.memory_bytes();
            }
        }
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn memory_bytes(&self) -> usize {
        self.bytes
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self::new(25, 256 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(before: &SectionBlocks, after: &SectionBlocks) -> ChangeSet {
        let mut set = ChangeSet::new();
        set.record(
            SectionPos::new(0, 0, 0),
            before,
            after,
            &ChangeMask::diff(before, after),
        );
        set
    }

    #[test]
    fn uniform_fill_is_a_single_run() {
        let before = SectionBlocks::default();
        let after = SectionBlocks::filled(BlockState(1));
        let set = record(&before, &after);
        assert_eq!(set.changed_blocks(), 4096);
        assert_eq!(set.sections[0].lengths, vec![4096]);
        assert!(set.memory_bytes() < 200);
    }

    #[test]
    fn restores_both_sides_exactly() {
        let mut before = SectionBlocks::default();
        for index in 0..SECTION_VOLUME {
            before[index] = BlockState((index % 7) as u32);
        }
        let mut after = before.clone();
        for index in (0..SECTION_VOLUME).step_by(3) {
            after[index] = BlockState(100 + (index % 5) as u32);
        }
        let set = record(&before, &after);
        assert_eq!(set.changed_blocks(), set.recount());

        let mut undone = after.clone();
        set.apply_section(0, &mut undone, Side::Before);
        assert_eq!(undone, before);
        let mut redone = before.clone();
        set.apply_section(0, &mut redone, Side::After);
        assert_eq!(redone, after);
    }

    #[test]
    fn unchanged_sections_are_not_recorded() {
        let blocks = SectionBlocks::filled(BlockState(3));
        assert!(record(&blocks, &blocks).is_empty());
    }

    #[test]
    fn history_drops_oldest_and_new_edits_clear_redo() {
        let edit = |state| {
            record(
                &SectionBlocks::default(),
                &SectionBlocks::filled(BlockState(state)),
            )
        };
        let mut history = History::new(2, usize::MAX);
        history.push(edit(1), ());
        history.push(edit(2), ());
        history.push(edit(3), ());
        assert_eq!(history.undo_len(), 2);

        let (undone, tag) = history.pop_undo().unwrap();
        history.push_redo(undone, tag);
        assert_eq!(history.redo_len(), 1);
        history.push(edit(4), ());
        assert_eq!(history.redo_len(), 0);
    }

    #[test]
    fn history_respects_its_memory_budget() {
        let mut noisy = SectionBlocks::default();
        for index in 0..SECTION_VOLUME {
            noisy[index] = BlockState(index as u32 + 1);
        }
        let edit = record(&SectionBlocks::default(), &noisy);
        let size = edit.memory_bytes();
        let mut history = History::<u8>::new(100, size * 2);
        for tag in 0..5 {
            history.push(edit.clone(), tag);
        }
        assert_eq!(history.pop_undo().map(|(_, tag)| tag), Some(4));
        history.push(edit.clone(), 5);
        assert_eq!(history.undo_len(), 2);
        assert!(history.memory_bytes() <= size * 2);
    }
}
