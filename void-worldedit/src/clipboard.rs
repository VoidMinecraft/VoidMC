use std::collections::HashMap;

use crate::block::BlockState;
use crate::extent::{Extent, SectionBlocks};
use crate::math::{Axis, BlockPos, SectionPos};
use crate::region::{Cuboid, Region};

const KEEP: u16 = u16::MAX;

/// A copied volume: a palette of block states and one `u16` per block, laid
/// out `x + z * width + y * width * length` like a Sponge schematic. Blocks
/// outside the copied region are "keep" entries that pasting never writes.
///
/// `offset` places the volume relative to the paste origin: pasting at `p`
/// puts the clipboard's minimum corner at `p + offset`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clipboard {
    size: BlockPos,
    offset: BlockPos,
    palette: Vec<BlockState>,
    data: Vec<u16>,
}

impl Clipboard {
    pub fn new(size: BlockPos, offset: BlockPos) -> Self {
        let size = size.max(BlockPos::new(1, 1, 1));
        let volume = size.x as usize * size.y as usize * size.z as usize;
        Self {
            size,
            offset,
            palette: Vec::new(),
            data: vec![KEEP; volume],
        }
    }

    pub(crate) fn from_parts(
        size: BlockPos,
        offset: BlockPos,
        palette: Vec<BlockState>,
        data: Vec<u16>,
    ) -> Self {
        debug_assert_eq!(
            data.len(),
            size.x as usize * size.y as usize * size.z as usize
        );
        Self {
            size,
            offset,
            palette,
            data,
        }
    }

    pub fn size(&self) -> BlockPos {
        self.size
    }

    pub fn offset(&self) -> BlockPos {
        self.offset
    }

    pub fn set_offset(&mut self, offset: BlockPos) {
        self.offset = offset;
    }

    pub fn volume(&self) -> usize {
        self.data.len()
    }

    pub fn palette(&self) -> &[BlockState] {
        &self.palette
    }

    pub fn bounds_at(&self, origin: BlockPos) -> Cuboid {
        let min = origin + self.offset;
        Cuboid {
            min,
            max: min + self.size - BlockPos::new(1, 1, 1),
        }
    }

    #[inline]
    fn index(&self, local: BlockPos) -> usize {
        local.x as usize
            + local.z as usize * self.size.x as usize
            + local.y as usize * self.size.x as usize * self.size.z as usize
    }

    fn local(&self, index: usize) -> BlockPos {
        let (w, l) = (self.size.x as usize, self.size.z as usize);
        BlockPos::new(
            (index % w) as i32,
            (index / (w * l)) as i32,
            (index / w % l) as i32,
        )
    }

    fn in_bounds(&self, local: BlockPos) -> bool {
        (0..self.size.x).contains(&local.x)
            && (0..self.size.y).contains(&local.y)
            && (0..self.size.z).contains(&local.z)
    }

    /// The block at `local` (0-based from the minimum corner), or `None`
    /// for a keep entry or a position outside the clipboard.
    pub fn get(&self, local: BlockPos) -> Option<BlockState> {
        if !self.in_bounds(local) {
            return None;
        }
        match self.data[self.index(local)] {
            KEEP => None,
            entry => Some(self.palette[usize::from(entry)]),
        }
    }

    pub fn set(&mut self, local: BlockPos, state: BlockState) {
        if self.in_bounds(local) {
            let entry = self.palette_entry(state);
            let index = self.index(local);
            self.data[index] = entry;
        }
    }

    fn palette_entry(&mut self, state: BlockState) -> u16 {
        match self.palette.iter().position(|s| *s == state) {
            Some(entry) => entry as u16,
            None => {
                self.palette.push(state);
                (self.palette.len() - 1) as u16
            }
        }
    }

    pub fn blocks(&self) -> impl Iterator<Item = (BlockPos, BlockState)> + '_ {
        self.data
            .iter()
            .enumerate()
            .filter(|(_, entry)| **entry != KEEP)
            .map(|(index, entry)| (self.local(index), self.palette[usize::from(*entry)]))
    }

    pub fn block_count(&self) -> usize {
        self.data.iter().filter(|entry| **entry != KEEP).count()
    }

    /// Copies `region` out of `extent` in one go; `origin` becomes the paste
    /// anchor. Use [`CopyJob`](crate::job::CopyJob) to spread a large copy
    /// over several ticks.
    pub fn copy(extent: &dyn Extent, region: &dyn Region, origin: BlockPos) -> Self {
        let mut builder = ClipboardBuilder::new(region, origin);
        let mut buffer = SectionBlocks::default();
        for section in region.bounds().sections() {
            if extent.read_section(section, &mut buffer) {
                builder.copy_section(region, section, &buffer);
            }
        }
        builder.finish()
    }

    /// Rotates around the paste origin by `quarter_turns` × 90° clockwise
    /// seen from above, turning the block states with it.
    pub fn rotated(&self, quarter_turns: u8) -> Self {
        let turns = quarter_turns % 4;
        if turns == 0 {
            return self.clone();
        }
        let turn = |p: BlockPos| (0..turns).fold(p, |p, _| BlockPos::new(-p.z, p.y, p.x));
        self.remapped(turn, |state| state.rotated(turns))
    }

    /// Mirrors across the plane through the paste origin perpendicular to `axis`.
    pub fn flipped(&self, axis: Axis) -> Self {
        let mirror = |p: BlockPos| match axis {
            Axis::X => BlockPos::new(-p.x, p.y, p.z),
            Axis::Y => BlockPos::new(p.x, -p.y, p.z),
            Axis::Z => BlockPos::new(p.x, p.y, -p.z),
        };
        self.remapped(mirror, |state| state.flipped(axis))
    }

    fn remapped(
        &self,
        transform: impl Fn(BlockPos) -> BlockPos,
        state: impl Fn(BlockState) -> BlockState,
    ) -> Self {
        let far = self.offset + self.size - BlockPos::new(1, 1, 1);
        let (a, b) = (transform(self.offset), transform(far));
        let offset = a.min(b);
        let size = a.max(b) - offset + BlockPos::new(1, 1, 1);
        let mut out = Self {
            size,
            offset,
            palette: self.palette.iter().map(|s| state(*s)).collect(),
            data: vec![KEEP; self.data.len()],
        };
        for (index, entry) in self.data.iter().enumerate() {
            if *entry != KEEP {
                let local = transform(self.offset + self.local(index)) - offset;
                let target = out.index(local);
                out.data[target] = *entry;
            }
        }
        out
    }

    #[inline]
    pub(crate) fn raw(&self, index: usize) -> Option<BlockState> {
        match self.data[index] {
            KEEP => None,
            entry => Some(self.palette[usize::from(entry)]),
        }
    }

    pub(crate) fn raw_entry(&self, index: usize) -> Option<u16> {
        Some(self.data[index]).filter(|entry| *entry != KEEP)
    }

    pub(crate) fn row_index(&self, local_y: i32, local_z: i32) -> usize {
        self.index(BlockPos::new(0, local_y, local_z))
    }
}

/// Fills a clipboard section by section; shared by [`Clipboard::copy`] and
/// the tick-budgeted copy job.
#[derive(Debug)]
pub(crate) struct ClipboardBuilder {
    clipboard: Clipboard,
    min: BlockPos,
    entries: HashMap<BlockState, u16>,
}

impl ClipboardBuilder {
    pub(crate) fn new(region: &dyn Region, origin: BlockPos) -> Self {
        let bounds = region.bounds();
        Self {
            clipboard: Clipboard::new(bounds.size(), bounds.min - origin),
            min: bounds.min,
            entries: HashMap::new(),
        }
    }

    pub(crate) fn copy_section(
        &mut self,
        region: &dyn Region,
        section: SectionPos,
        blocks: &SectionBlocks,
    ) {
        let Some(clip) = region.bounds().intersection(&Cuboid::of_section(section)) else {
            return;
        };
        let base = section.min_block();
        let mut last: Option<(BlockState, u16)> = None;
        for y in clip.min.y..=clip.max.y {
            for z in clip.min.z..=clip.max.z {
                let row = self.clipboard.row_index(y - self.min.y, z - self.min.z);
                let section_row =
                    crate::math::section_index(0, (y - base.y) as usize, (z - base.z) as usize);
                let Self {
                    clipboard,
                    entries,
                    min,
                    ..
                } = self;
                region.for_each_span(y, z, clip.min.x, clip.max.x, &mut |from, to| {
                    for x in from..=to {
                        let state = blocks[section_row + (x - base.x) as usize];
                        let entry = match last {
                            Some((cached, entry)) if cached == state => entry,
                            _ => {
                                let entry = *entries.entry(state).or_insert_with(|| {
                                    clipboard.palette.push(state);
                                    (clipboard.palette.len() - 1) as u16
                                });
                                last = Some((state, entry));
                                entry
                            }
                        };
                        clipboard.data[row + (x - min.x) as usize] = entry;
                    }
                });
            }
        }
    }

    pub(crate) fn finish(self) -> Clipboard {
        self.clipboard
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extent::MemoryExtent;
    use crate::region::Ellipsoid;

    fn state(input: &str) -> BlockState {
        BlockState::parse(input).unwrap()
    }

    #[test]
    fn copy_reads_across_sections_and_keeps_outside_region() {
        let mut extent = MemoryExtent::default();
        let stone = state("stone");
        for pos in Cuboid::new(BlockPos::new(-2, 14, -2), BlockPos::new(2, 18, 2)).positions() {
            extent.set(pos, stone);
        }
        let sphere = Ellipsoid::sphere(BlockPos::new(0, 16, 0), 2.0);
        let origin = BlockPos::new(0, 10, 0);
        let clipboard = Clipboard::copy(&extent, &sphere, origin);
        assert_eq!(clipboard.size(), BlockPos::new(5, 5, 5));
        assert_eq!(clipboard.offset(), BlockPos::new(-2, 4, -2));
        assert_eq!(clipboard.block_count() as u64, sphere.volume());
        assert_eq!(clipboard.get(BlockPos::new(2, 2, 2)), Some(stone));
        assert_eq!(clipboard.get(BlockPos::new(0, 0, 0)), None);
        assert_eq!(clipboard.bounds_at(origin), sphere.bounds());
    }

    #[test]
    fn rotation_moves_blocks_and_turns_states() {
        let mut clipboard = Clipboard::new(BlockPos::new(3, 1, 1), BlockPos::new(1, 0, 0));
        let stairs = state("oak_stairs[facing=east]");
        clipboard.set(BlockPos::new(2, 0, 0), stairs);
        let turned = clipboard.rotated(1);
        assert_eq!(turned.size(), BlockPos::new(1, 1, 3));
        assert_eq!(turned.offset(), BlockPos::new(0, 0, 1));
        assert_eq!(
            turned
                .get(BlockPos::new(0, 0, 2))
                .unwrap()
                .property("facing"),
            Some("south")
        );
        assert_eq!(clipboard.rotated(4), clipboard);
        assert_eq!(clipboard.rotated(1).rotated(3), clipboard);
    }

    #[test]
    fn flip_mirrors_around_the_origin() {
        let mut clipboard = Clipboard::new(BlockPos::new(2, 1, 1), BlockPos::new(1, 0, 0));
        clipboard.set(BlockPos::new(1, 0, 0), state("oak_stairs[facing=east]"));
        let flipped = clipboard.flipped(Axis::X);
        assert_eq!(flipped.offset(), BlockPos::new(-2, 0, 0));
        assert_eq!(
            flipped
                .get(BlockPos::new(0, 0, 0))
                .unwrap()
                .property("facing"),
            Some("west")
        );
        assert_eq!(flipped.flipped(Axis::X), clipboard);
    }
}
