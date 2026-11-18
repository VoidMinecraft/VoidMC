use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::clipboard::{Clipboard, ClipboardBuilder};
use crate::extent::{ChangeMask, Extent, SectionBlocks};
use crate::history::ChangeSet;
use crate::math::{BlockPos, SECTION_VOLUME, SectionPos};
use crate::operation::Operation;
use crate::region::Region;

/// How much work one step may do: a block count (each processed section
/// costs 4096, the work of reading, diffing and writing it) and an optional
/// wall-clock deadline. At least one section always runs, so every job
/// progresses.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    blocks: u64,
    deadline: Option<Instant>,
    spent: bool,
}

impl Budget {
    pub fn blocks(blocks: u64) -> Self {
        Self {
            blocks,
            deadline: None,
            spent: false,
        }
    }

    pub fn unlimited() -> Self {
        Self::blocks(u64::MAX)
    }

    pub fn with_time(mut self, limit: Duration) -> Self {
        self.deadline = Some(Instant::now() + limit);
        self
    }

    pub fn exhausted(&self) -> bool {
        self.spent && (self.blocks == 0 || self.deadline.is_some_and(|d| Instant::now() >= d))
    }

    fn spend(&mut self, blocks: u64) {
        self.blocks = self.blocks.saturating_sub(blocks);
        self.spent = true;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditStats {
    pub changed: u64,
    pub sections: usize,
    pub unloaded_sections: usize,
}

fn in_height(sections: Vec<SectionPos>, (min_y, max_y): (i32, i32)) -> Vec<SectionPos> {
    sections
        .into_iter()
        .filter(|s| s.y >= min_y >> 4 && s.y <= max_y >> 4)
        .collect()
}

/// Runs an [`Operation`] section by section over as many steps as needed,
/// recording every change for undo.
#[derive(Debug)]
pub struct EditJob {
    op: Arc<dyn Operation>,
    sections: Vec<SectionPos>,
    next: usize,
    before: SectionBlocks,
    after: SectionBlocks,
    changes: ChangeSet,
    stats: EditStats,
}

impl EditJob {
    pub fn new(op: impl Operation + 'static, height: (i32, i32)) -> Self {
        Self::from_arc(Arc::new(op), height)
    }

    pub fn from_arc(op: Arc<dyn Operation>, height: (i32, i32)) -> Self {
        let sections = in_height(op.sections(), height);
        Self {
            op,
            sections,
            next: 0,
            before: SectionBlocks::default(),
            after: SectionBlocks::default(),
            changes: ChangeSet::new(),
            stats: EditStats::default(),
        }
    }

    pub fn operation(&self) -> &Arc<dyn Operation> {
        &self.op
    }

    /// Processes sections until the budget runs out; `true` once finished.
    pub fn step(&mut self, extent: &mut dyn Extent, budget: &mut Budget) -> bool {
        while self.next < self.sections.len() && !budget.exhausted() {
            let section = self.sections[self.next];
            self.next += 1;
            budget.spend(SECTION_VOLUME as u64);
            if !extent.read_section(section, &mut self.before) {
                self.stats.unloaded_sections += 1;
                continue;
            }
            self.after.clone_from(&self.before);
            self.op.apply(section, &mut self.after);
            let changed = ChangeMask::diff(&self.before, &self.after);
            self.stats.sections += 1;
            if changed.is_empty() {
                continue;
            }
            self.stats.changed += changed.count() as u64;
            self.changes
                .record(section, &self.before, &self.after, &changed);
            extent.write_section(section, &self.after, &changed);
        }
        self.is_done()
    }

    pub fn run(mut self, extent: &mut dyn Extent) -> (ChangeSet, EditStats) {
        self.step(extent, &mut Budget::unlimited());
        self.finish()
    }

    pub fn is_done(&self) -> bool {
        self.next >= self.sections.len()
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.next, self.sections.len())
    }

    pub fn finish(self) -> (ChangeSet, EditStats) {
        (self.changes, self.stats)
    }
}

/// Copies a region into a clipboard over as many steps as needed.
#[derive(Debug)]
pub struct CopyJob {
    region: Arc<dyn Region>,
    sections: Vec<SectionPos>,
    next: usize,
    buffer: SectionBlocks,
    builder: ClipboardBuilder,
    unloaded_sections: usize,
}

impl CopyJob {
    pub fn new(region: Arc<dyn Region>, origin: BlockPos, height: (i32, i32)) -> Self {
        let sections = in_height(region.bounds().sections().collect(), height);
        let builder = ClipboardBuilder::new(region.as_ref(), origin);
        Self {
            region,
            sections,
            next: 0,
            buffer: SectionBlocks::default(),
            builder,
            unloaded_sections: 0,
        }
    }

    pub fn step(&mut self, extent: &mut dyn Extent, budget: &mut Budget) -> bool {
        while self.next < self.sections.len() && !budget.exhausted() {
            let section = self.sections[self.next];
            self.next += 1;
            budget.spend(SECTION_VOLUME as u64);
            if extent.read_section(section, &mut self.buffer) {
                self.builder
                    .copy_section(self.region.as_ref(), section, &self.buffer);
            } else {
                self.unloaded_sections += 1;
            }
        }
        self.next >= self.sections.len()
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.next, self.sections.len())
    }

    pub fn unloaded_sections(&self) -> usize {
        self.unloaded_sections
    }

    pub fn finish(self) -> Clipboard {
        self.builder.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockState;
    use crate::extent::MemoryExtent;
    use crate::mask::Mask;
    use crate::operation::{Fill, Paste, Restore, Sequence};
    use crate::pattern::Pattern;
    use crate::region::{Cuboid, Ellipsoid, Faces, Walls};

    fn state(input: &str) -> BlockState {
        BlockState::parse(input).unwrap()
    }

    fn cuboid(a: (i32, i32, i32), b: (i32, i32, i32)) -> Cuboid {
        Cuboid::new(BlockPos::new(a.0, a.1, a.2), BlockPos::new(b.0, b.1, b.2))
    }

    fn count(extent: &MemoryExtent, region: &Cuboid, target: BlockState) -> usize {
        region
            .positions()
            .filter(|p| extent.block(*p) == Some(target))
            .count()
    }

    #[test]
    fn set_fills_exactly_the_region_and_undo_restores_it() {
        let mut extent = MemoryExtent::default();
        let dirt = state("dirt");
        let area = cuboid((-5, 60, -5), (20, 70, 20));
        for pos in area.positions().step_by(3) {
            extent.set(pos, dirt);
        }
        let original = extent.clone();

        let region = cuboid((-3, 62, -3), (17, 66, 17));
        let (changes, stats) =
            EditJob::new(Fill::new(region, state("stone")), (-64, 319)).run(&mut extent);
        assert_eq!(
            count(&extent, &region, state("stone")) as u64,
            region.volume()
        );
        assert_eq!(stats.changed, changes.changed_blocks());
        assert_eq!(
            count(&extent, &area, dirt),
            count(&original, &area, dirt) - count(&original, &region, dirt)
        );

        EditJob::new(Restore::undo(Arc::new(changes)), (-64, 319)).run(&mut extent);
        for pos in area.positions() {
            assert_eq!(extent.block(pos), original.block(pos), "{pos}");
        }
    }

    #[test]
    fn replace_only_touches_masked_blocks() {
        let mut extent = MemoryExtent::default();
        let region = cuboid((0, 0, 0), (9, 9, 9));
        for pos in region.positions() {
            extent.set(
                pos,
                if pos.x % 2 == 0 {
                    state("dirt")
                } else {
                    state("sand")
                },
            );
        }
        let op = Fill::new(region, state("glass")).masked(Mask::parse("dirt").unwrap());
        let (_, stats) = EditJob::new(op, (-64, 319)).run(&mut extent);
        assert_eq!(stats.changed, 500);
        assert_eq!(count(&extent, &region, state("sand")), 500);
        assert_eq!(count(&extent, &region, state("glass")), 500);
    }

    #[test]
    fn weighted_patterns_fill_every_block_with_one_of_their_states() {
        let mut extent = MemoryExtent::default();
        let region = cuboid((0, 0, 0), (31, 3, 31));
        let pattern = Pattern::parse("stone,dirt").unwrap().with_seed(3);
        EditJob::new(Fill::new(region, pattern), (-64, 319)).run(&mut extent);
        let stone = count(&extent, &region, state("stone"));
        let dirt = count(&extent, &region, state("dirt"));
        assert_eq!(stone + dirt, region.volume() as usize);
        assert!(stone > 1500 && dirt > 1500);
    }

    #[test]
    fn walls_and_outline_leave_the_inside_untouched() {
        let mut extent = MemoryExtent::default();
        let region = cuboid((0, 0, 0), (6, 4, 8));
        EditJob::new(Fill::new(Walls(region), state("stone")), (-64, 319)).run(&mut extent);
        assert_eq!(
            count(&extent, &region, state("stone")) as u64,
            Walls(region).volume()
        );
        assert_eq!(extent.block(BlockPos::new(3, 2, 4)), Some(BlockState::AIR));
        assert_eq!(extent.block(BlockPos::new(3, 0, 4)), Some(BlockState::AIR));

        EditJob::new(Fill::new(Faces(region), state("glass")), (-64, 319)).run(&mut extent);
        assert_eq!(extent.block(BlockPos::new(3, 0, 4)), Some(state("glass")));
        assert_eq!(extent.block(BlockPos::new(3, 2, 4)), Some(BlockState::AIR));
    }

    #[test]
    fn budget_spreads_work_and_matches_a_single_run() {
        let region = Ellipsoid::sphere(BlockPos::new(0, 64, 0), 20.0);
        let mut stepped = MemoryExtent::default();
        let mut job = EditJob::new(Fill::new(region, state("stone")), (-64, 319));
        let mut steps = 0;
        while !job.step(&mut stepped, &mut Budget::blocks(4096 * 4)) {
            steps += 1;
            let (done, total) = job.progress();
            assert!(done <= total);
        }
        assert!(steps > 5);
        let (stepped_changes, _) = job.finish();

        let mut single = MemoryExtent::default();
        let (single_changes, _) =
            EditJob::new(Fill::new(region, state("stone")), (-64, 319)).run(&mut single);
        assert_eq!(stepped_changes, single_changes);
        assert_eq!(stepped_changes.changed_blocks(), region.volume());
    }

    #[test]
    fn a_zero_budget_still_makes_progress() {
        let mut extent = MemoryExtent::default();
        let mut job = EditJob::new(
            Fill::new(cuboid((0, 0, 0), (40, 0, 0)), state("stone")),
            (-64, 319),
        );
        assert!(!job.step(&mut extent, &mut Budget::blocks(0)));
        assert_eq!(job.progress().0, 1);
    }

    #[test]
    fn sections_outside_the_world_height_are_skipped() {
        let mut extent = MemoryExtent::default();
        let region = cuboid((0, 300, 0), (0, 400, 0));
        let (_, stats) =
            EditJob::new(Fill::new(region, state("stone")), (-64, 319)).run(&mut extent);
        assert_eq!(stats.changed, 20);
    }

    #[test]
    fn copy_job_matches_one_shot_copy() {
        let mut extent = MemoryExtent::default();
        let region = cuboid((-20, 0, -20), (20, 20, 20));
        EditJob::new(
            Fill::new(region, Pattern::parse("stone,dirt,sand").unwrap()),
            (-64, 319),
        )
        .run(&mut extent);
        let origin = BlockPos::new(1, 2, 3);
        let mut job = CopyJob::new(Arc::new(region), origin, (-64, 319));
        while !job.step(&mut extent, &mut Budget::blocks(4096)) {}
        assert_eq!(job.finish(), Clipboard::copy(&extent, &region, origin));
    }

    #[test]
    fn move_clears_the_source_and_pastes_the_destination_in_one_pass() {
        let mut extent = MemoryExtent::default();
        let source = cuboid((0, 0, 0), (4, 4, 4));
        EditJob::new(Fill::new(source, state("stone")), (-64, 319)).run(&mut extent);
        let clipboard = Arc::new(Clipboard::copy(&extent, &source, BlockPos::ZERO));
        let offset = BlockPos::new(2, 0, 0);
        let op = Sequence(vec![
            Box::new(Fill::new(source, BlockState::AIR)),
            Box::new(Paste::new(clipboard, offset)),
        ]);
        EditJob::new(op, (-64, 319)).run(&mut extent);
        let moved = source.shifted(offset);
        assert_eq!(
            count(&extent, &moved, state("stone")) as u64,
            moved.volume()
        );
        assert_eq!(extent.block(BlockPos::new(0, 2, 2)), Some(BlockState::AIR));
        assert_eq!(extent.block(BlockPos::new(1, 2, 2)), Some(BlockState::AIR));
    }

    #[test]
    fn stack_repeats_the_clipboard_and_skip_air_keeps_the_world() {
        let mut extent = MemoryExtent::default();
        let source = cuboid((0, 0, 0), (1, 1, 1));
        extent.set(BlockPos::ZERO, state("stone"));
        let clipboard = Arc::new(Clipboard::copy(&extent, &source, BlockPos::ZERO));
        extent.set(BlockPos::new(3, 1, 1), state("dirt"));
        let mut paste = Paste::new(clipboard, BlockPos::new(2, 0, 0)).skip_air(true);
        paste.origins.push(BlockPos::new(4, 0, 0));
        EditJob::new(paste, (-64, 319)).run(&mut extent);
        assert_eq!(extent.block(BlockPos::new(2, 0, 0)), Some(state("stone")));
        assert_eq!(extent.block(BlockPos::new(4, 0, 0)), Some(state("stone")));
        assert_eq!(extent.block(BlockPos::new(3, 1, 1)), Some(state("dirt")));
    }

    #[test]
    fn redo_reapplies_an_undone_edit() {
        let mut extent = MemoryExtent::default();
        let region = cuboid((0, 0, 0), (17, 17, 17));
        let (changes, _) =
            EditJob::new(Fill::new(region, state("stone")), (-64, 319)).run(&mut extent);
        let changes = Arc::new(changes);
        let after = extent.clone();
        EditJob::new(Restore::undo(changes.clone()), (-64, 319)).run(&mut extent);
        assert_eq!(
            count(&extent, &region, BlockState::AIR) as u64,
            region.volume()
        );
        EditJob::new(Restore::redo(changes), (-64, 319)).run(&mut extent);
        for pos in region.positions() {
            assert_eq!(extent.block(pos), after.block(pos));
        }
    }
}
