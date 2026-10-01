use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::sync::Arc;

use crate::block::BlockState;
use crate::clipboard::Clipboard;
use crate::extent::SectionBlocks;
use crate::history::{ChangeSet, Side};
use crate::mask::Mask;
use crate::math::{BlockPos, SectionPos, section_index};
use crate::pattern::Pattern;
use crate::region::{Cuboid, Region};

/// A world edit expressed per section: the job reads each section it
/// touches, lets the operation rewrite it in place, then records and writes
/// back whatever changed. Operations never see sections they did not ask for.
pub trait Operation: Send + Sync + Debug {
    fn sections(&self) -> Vec<SectionPos>;

    fn apply(&self, section: SectionPos, blocks: &mut SectionBlocks);
}

fn sorted(sections: impl IntoIterator<Item = SectionPos>) -> Vec<SectionPos> {
    let unique: BTreeSet<(i32, i32, i32)> = sections.into_iter().map(|s| (s.x, s.z, s.y)).collect();
    unique
        .into_iter()
        .map(|(x, z, y)| SectionPos::new(x, y, z))
        .collect()
}

/// Sets every block of `region` that passes `mask` to `pattern`: covers
/// set, replace, walls, outline and the shape brushes.
#[derive(Debug, Clone)]
pub struct Fill {
    pub region: Arc<dyn Region>,
    pub pattern: Pattern,
    pub mask: Mask,
}

impl Fill {
    pub fn new(region: impl Region + 'static, pattern: impl Into<Pattern>) -> Self {
        Self {
            region: Arc::new(region),
            pattern: pattern.into(),
            mask: Mask::Any,
        }
    }

    pub fn masked(mut self, mask: Mask) -> Self {
        self.mask = mask;
        self
    }
}

impl Operation for Fill {
    fn sections(&self) -> Vec<SectionPos> {
        sorted(self.region.bounds().sections())
    }

    fn apply(&self, section: SectionPos, blocks: &mut SectionBlocks) {
        let section_box = Cuboid::of_section(section);
        let Some(clip) = self.region.bounds().intersection(&section_box) else {
            return;
        };
        let base = section.min_block();
        let uniform = self.pattern.single();
        if let (Some(state), true) = (uniform, self.mask.is_any())
            && clip == section_box
            && self.region.covers(&section_box)
        {
            blocks.fill(state);
            return;
        }
        let slots = blocks.as_mut_slice();
        for y in clip.min.y..=clip.max.y {
            for z in clip.min.z..=clip.max.z {
                let row = section_index(0, (y - base.y) as usize, (z - base.z) as usize);
                self.region
                    .for_each_span(y, z, clip.min.x, clip.max.x, &mut |from, to| {
                        let (a, b) = (row + (from - base.x) as usize, row + (to - base.x) as usize);
                        match (uniform, self.mask.is_any()) {
                            (Some(state), true) => slots[a..=b].fill(state),
                            _ => {
                                for (offset, slot) in slots[a..=b].iter_mut().enumerate() {
                                    if self.mask.test(*slot) {
                                        *slot = self.pattern.at(BlockPos::new(
                                            from + offset as i32,
                                            y,
                                            z,
                                        ));
                                    }
                                }
                            }
                        }
                    });
            }
        }
    }
}

/// Pastes a clipboard at one or more origins (several origins = a stack).
#[derive(Debug, Clone)]
pub struct Paste {
    pub clipboard: Arc<Clipboard>,
    pub origins: Vec<BlockPos>,
    pub skip_air: bool,
}

impl Paste {
    pub fn new(clipboard: Arc<Clipboard>, origin: BlockPos) -> Self {
        Self {
            clipboard,
            origins: vec![origin],
            skip_air: false,
        }
    }

    pub fn skip_air(mut self, skip: bool) -> Self {
        self.skip_air = skip;
        self
    }

    pub fn bounds(&self) -> Option<Cuboid> {
        self.origins
            .iter()
            .map(|origin| self.clipboard.bounds_at(*origin))
            .reduce(|a, b| Cuboid::new(a.min.min(b.min), a.max.max(b.max)))
    }
}

impl Operation for Paste {
    fn sections(&self) -> Vec<SectionPos> {
        sorted(
            self.origins
                .iter()
                .flat_map(|origin| self.clipboard.bounds_at(*origin).sections()),
        )
    }

    fn apply(&self, section: SectionPos, blocks: &mut SectionBlocks) {
        let section_box = Cuboid::of_section(section);
        let base = section.min_block();
        let slots = blocks.as_mut_slice();
        for origin in &self.origins {
            let placed = self.clipboard.bounds_at(*origin);
            let Some(clip) = placed.intersection(&section_box) else {
                continue;
            };
            for y in clip.min.y..=clip.max.y {
                for z in clip.min.z..=clip.max.z {
                    let source = self.clipboard.row_index(y - placed.min.y, z - placed.min.z);
                    let target = section_index(0, (y - base.y) as usize, (z - base.z) as usize);
                    for x in clip.min.x..=clip.max.x {
                        if let Some(state) =
                            self.clipboard.raw(source + (x - placed.min.x) as usize)
                            && !(self.skip_air && state.is_air())
                        {
                            slots[target + (x - base.x) as usize] = state;
                        }
                    }
                }
            }
        }
    }
}

/// Several operations applied in order to each section they touch, so a
/// move clears its source and pastes its destination in a single pass.
#[derive(Debug, Default)]
pub struct Sequence(pub Vec<Box<dyn Operation>>);

impl Operation for Sequence {
    fn sections(&self) -> Vec<SectionPos> {
        sorted(self.0.iter().flat_map(|op| op.sections()))
    }

    fn apply(&self, section: SectionPos, blocks: &mut SectionBlocks) {
        for op in &self.0 {
            op.apply(section, blocks);
        }
    }
}

/// Puts a recorded edit's before (undo) or after (redo) states back.
#[derive(Debug)]
pub struct Restore {
    changes: Arc<ChangeSet>,
    side: Side,
    index: HashMap<SectionPos, usize>,
}

impl Restore {
    pub fn undo(changes: Arc<ChangeSet>) -> Self {
        Self::new(changes, Side::Before)
    }

    pub fn redo(changes: Arc<ChangeSet>) -> Self {
        Self::new(changes, Side::After)
    }

    fn new(changes: Arc<ChangeSet>, side: Side) -> Self {
        let index = changes
            .sections()
            .enumerate()
            .map(|(index, section)| (section, index))
            .collect();
        Self {
            changes,
            side,
            index,
        }
    }

    pub fn changes(&self) -> &Arc<ChangeSet> {
        &self.changes
    }
}

impl Operation for Restore {
    fn sections(&self) -> Vec<SectionPos> {
        sorted(self.changes.sections())
    }

    fn apply(&self, section: SectionPos, blocks: &mut SectionBlocks) {
        if let Some(index) = self.index.get(&section) {
            debug_assert!(*index < self.changes.section_count());
            self.changes.apply_section(*index, blocks, self.side);
        }
    }
}

/// An explicit list of block changes, grouped by section; brushes that
/// compute their result up front (smooth, gravity) emit one.
#[derive(Debug, Default, Clone)]
pub struct BlockList {
    sections: HashMap<SectionPos, Vec<(u16, BlockState)>>,
}

impl BlockList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, pos: BlockPos, state: BlockState) {
        self.sections
            .entry(pos.section())
            .or_default()
            .push((pos.section_index() as u16, state));
    }

    pub fn len(&self) -> usize {
        self.sections.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

impl Operation for BlockList {
    fn sections(&self) -> Vec<SectionPos> {
        sorted(self.sections.keys().copied())
    }

    fn apply(&self, section: SectionPos, blocks: &mut SectionBlocks) {
        for (index, state) in self.sections.get(&section).into_iter().flatten() {
            blocks[usize::from(*index)] = *state;
        }
    }
}
