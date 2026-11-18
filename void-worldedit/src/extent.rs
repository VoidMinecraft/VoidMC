use std::collections::HashMap;
use std::ops::{Index, IndexMut};

use crate::block::BlockState;
use crate::math::{BlockPos, SECTION_VOLUME, SectionPos};

/// The 4096 block states of a 16³ section, indexed `y << 8 | z << 4 | x`.
#[derive(Clone, PartialEq, Eq)]
pub struct SectionBlocks(Box<[BlockState; SECTION_VOLUME]>);

impl SectionBlocks {
    pub fn filled(state: BlockState) -> Self {
        Self(Box::new([state; SECTION_VOLUME]))
    }

    pub fn as_slice(&self) -> &[BlockState; SECTION_VOLUME] {
        &self.0
    }

    pub fn as_mut_slice(&mut self) -> &mut [BlockState; SECTION_VOLUME] {
        &mut self.0
    }

    pub fn fill(&mut self, state: BlockState) {
        self.0.fill(state);
    }

    pub fn uniform(&self) -> Option<BlockState> {
        let first = self.0[0];
        self.0.iter().all(|s| *s == first).then_some(first)
    }
}

impl Default for SectionBlocks {
    fn default() -> Self {
        Self::filled(BlockState::AIR)
    }
}

impl std::fmt::Debug for SectionBlocks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.uniform() {
            Some(state) => write!(f, "SectionBlocks(all {})", state.0),
            None => f.write_str("SectionBlocks(mixed)"),
        }
    }
}

impl Index<usize> for SectionBlocks {
    type Output = BlockState;
    #[inline]
    fn index(&self, index: usize) -> &BlockState {
        &self.0[index]
    }
}

impl IndexMut<usize> for SectionBlocks {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut BlockState {
        &mut self.0[index]
    }
}

/// Which of a section's 4096 blocks an edit changed.
#[derive(Clone, PartialEq, Eq)]
pub struct ChangeMask {
    bits: [u64; SECTION_VOLUME / 64],
    count: u16,
}

impl ChangeMask {
    pub fn empty() -> Self {
        Self {
            bits: [0; SECTION_VOLUME / 64],
            count: 0,
        }
    }

    pub fn diff(before: &SectionBlocks, after: &SectionBlocks) -> Self {
        let mut mask = Self::empty();
        for (word_index, word) in mask.bits.iter_mut().enumerate() {
            let base = word_index * 64;
            let (b, a) = (&before.0[base..base + 64], &after.0[base..base + 64]);
            for bit in 0..64 {
                *word |= u64::from(b[bit] != a[bit]) << bit;
            }
        }
        mask.count = mask.bits.iter().map(|w| w.count_ones() as u16).sum();
        mask
    }

    pub fn set(&mut self, index: usize) {
        let (word, bit) = (index / 64, index % 64);
        if self.bits[word] & (1 << bit) == 0 {
            self.bits[word] |= 1 << bit;
            self.count += 1;
        }
    }

    #[inline]
    pub fn contains(&self, index: usize) -> bool {
        self.bits[index / 64] & (1 << (index % 64)) != 0
    }

    pub fn count(&self) -> usize {
        usize::from(self.count)
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn is_full(&self) -> bool {
        self.count() == SECTION_VOLUME
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits
            .iter()
            .enumerate()
            .flat_map(|(word_index, &word)| {
                let mut word = word;
                std::iter::from_fn(move || {
                    (word != 0).then(|| {
                        let bit = word.trailing_zeros() as usize;
                        word &= word - 1;
                        word_index * 64 + bit
                    })
                })
            })
    }
}

impl std::fmt::Debug for ChangeMask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChangeMask({} changed)", self.count)
    }
}

/// Block storage an edit reads and writes, one 16³ section at a time. This is
/// the whole contract between the editing core and a server: implement it on
/// your chunk storage and every operation, undo and brush works unchanged.
pub trait Extent {
    /// Inclusive world height.
    fn height(&self) -> (i32, i32);

    /// `None` when the block is not loaded or outside the world.
    fn block(&self, pos: BlockPos) -> Option<BlockState>;

    /// Fills `out` with the section, or returns `false` when it is not loaded.
    fn read_section(&self, section: SectionPos, out: &mut SectionBlocks) -> bool;

    /// Stores the full section; `changed` marks the blocks that differ from
    /// the last read, so the implementation can send minimal updates.
    fn write_section(&mut self, section: SectionPos, blocks: &SectionBlocks, changed: &ChangeMask);
}

/// A fully loaded in-memory world, for tests, benchmarks and offline tools.
#[derive(Debug, Clone)]
pub struct MemoryExtent {
    sections: HashMap<SectionPos, SectionBlocks>,
    min_y: i32,
    max_y: i32,
    pub writes: u64,
}

impl MemoryExtent {
    pub fn new(min_y: i32, max_y: i32) -> Self {
        Self {
            sections: HashMap::new(),
            min_y,
            max_y,
            writes: 0,
        }
    }

    pub fn set(&mut self, pos: BlockPos, state: BlockState) {
        if (self.min_y..=self.max_y).contains(&pos.y) {
            self.sections.entry(pos.section()).or_default()[pos.section_index()] = state;
        }
    }

    pub fn section_count(&self) -> usize {
        self.sections.len()
    }
}

impl Default for MemoryExtent {
    fn default() -> Self {
        Self::new(-64, 319)
    }
}

impl Extent for MemoryExtent {
    fn height(&self) -> (i32, i32) {
        (self.min_y, self.max_y)
    }

    fn block(&self, pos: BlockPos) -> Option<BlockState> {
        if !(self.min_y..=self.max_y).contains(&pos.y) {
            return None;
        }
        Some(
            self.sections
                .get(&pos.section())
                .map_or(BlockState::AIR, |section| section[pos.section_index()]),
        )
    }

    fn read_section(&self, section: SectionPos, out: &mut SectionBlocks) -> bool {
        match self.sections.get(&section) {
            Some(blocks) => out.clone_from(blocks),
            None => out.fill(BlockState::AIR),
        }
        true
    }

    fn write_section(
        &mut self,
        section: SectionPos,
        blocks: &SectionBlocks,
        _changed: &ChangeMask,
    ) {
        self.writes += 1;
        match self.sections.get_mut(&section) {
            Some(existing) => existing.clone_from(blocks),
            None => {
                self.sections.insert(section, blocks.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_mask_diffs_and_iterates_in_index_order() {
        let before = SectionBlocks::default();
        let mut after = before.clone();
        for index in [0, 63, 64, 4095] {
            after[index] = BlockState(1);
        }
        let mask = ChangeMask::diff(&before, &after);
        assert_eq!(mask.count(), 4);
        assert_eq!(mask.iter().collect::<Vec<_>>(), vec![0, 63, 64, 4095]);
        assert!(mask.contains(63) && !mask.contains(62));
        assert!(ChangeMask::diff(&after, &after).is_empty());
    }

    #[test]
    fn memory_extent_reads_air_for_untouched_sections() {
        let mut extent = MemoryExtent::default();
        let pos = BlockPos::new(-3, 70, 18);
        assert_eq!(extent.block(pos), Some(BlockState::AIR));
        extent.set(pos, BlockState(5));
        assert_eq!(extent.block(pos), Some(BlockState(5)));
        assert_eq!(extent.block(BlockPos::new(0, 400, 0)), None);
        let mut out = SectionBlocks::default();
        assert!(extent.read_section(pos.section(), &mut out));
        assert_eq!(out[pos.section_index()], BlockState(5));
    }
}
