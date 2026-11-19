//! Regions: the unit of parallel per-tick world work.
//!
//! A region is the vanilla 32×32-chunk area of one dimension, the same area a
//! region file (`r.<x>.<z>.mca`) covers. Per-tick work that only touches
//! entity-local state and reads chunk data, such as the entity physics step, is
//! grouped by region with [`for_each_region`]. Under
//! [`ExecutionMode::MultiThreaded`] the regions run as concurrent tasks on
//! Bevy's compute task pool; under [`ExecutionMode::SingleThreaded`] they run
//! one after another on the calling thread.

use bevy_tasks::{ComputeTaskPool, TaskPool};

use super::{ChunkPos, DimensionId};
use crate::config::ExecutionMode;

/// Side length of a region, in chunks.
pub const REGION_CHUNKS: i32 = 32;

/// A 32×32-chunk area of one dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionPos {
    pub dimension: DimensionId,
    pub x: i32,
    pub z: i32,
}

impl RegionPos {
    /// The region holding `chunk`.
    pub fn of_chunk(dimension: DimensionId, chunk: ChunkPos) -> Self {
        Self {
            dimension,
            x: chunk.x.div_euclid(REGION_CHUNKS),
            z: chunk.z.div_euclid(REGION_CHUNKS),
        }
    }

    /// The region holding the block column at `block_x`, `block_z`.
    pub fn of_block(dimension: DimensionId, block_x: f64, block_z: f64) -> Self {
        Self::of_chunk(dimension, ChunkPos::from_block(block_x, block_z))
    }

    fn sort_key(&self) -> (i32, i32, i32) {
        (self.dimension.protocol_id(), self.x, self.z)
    }
}

/// Runs `work` once per region over the items in that region.
///
/// `region_of` places each item in a region. Items are grouped by region,
/// keeping their relative order within a region, and regions are visited in a
/// fixed order (dimension, then x, then z). Every item is handed to `work`
/// exactly once.
///
/// With [`ExecutionMode::MultiThreaded`] and more than one region, each region
/// is a task on the compute task pool and regions run concurrently; this call
/// returns once all of them have finished. Otherwise the regions run in order
/// on the calling thread. `work` must therefore only change the items it is
/// given and only read anything else, so that both paths produce the same
/// result.
pub fn for_each_region<T: Send>(
    mode: ExecutionMode,
    mut items: Vec<T>,
    region_of: impl Fn(&T) -> RegionPos,
    work: impl Fn(RegionPos, &mut [T]) + Sync,
) {
    // Stable, so items keep their order within a region.
    items.sort_by_key(|item| region_of(item).sort_key());
    let (Some(first), Some(last)) = (items.first(), items.last()) else {
        return;
    };
    // Sorted, so the items span several regions exactly when the first and
    // last differ. A single region gains nothing from a task.
    let parallel = mode == ExecutionMode::MultiThreaded && region_of(first) != region_of(last);
    let regions = items.chunk_by_mut(|a, b| region_of(a) == region_of(b));

    if !parallel {
        for region in regions {
            work(region_of(&region[0]), region);
        }
        return;
    }

    let work = &work;
    ComputeTaskPool::get_or_init(TaskPool::default).scope(|scope| {
        for region in regions {
            let at = region_of(&region[0]);
            scope.spawn(async move { work(at, region) });
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Condvar, Mutex};
    use std::thread::{self, ThreadId};
    use std::time::Duration;

    use super::*;

    /// Passed only once `parties` callers are inside it at the same time.
    struct Rendezvous {
        arrived: Mutex<usize>,
        all_in: Condvar,
        parties: usize,
    }

    impl Rendezvous {
        fn new(parties: usize) -> Self {
            Self {
                arrived: Mutex::new(0),
                all_in: Condvar::new(),
                parties,
            }
        }

        /// Whether every party arrived within the bound.
        fn arrive(&self) -> bool {
            let mut arrived = self.arrived.lock().unwrap();
            *arrived += 1;
            self.all_in.notify_all();
            let (arrived, _) = self
                .all_in
                .wait_timeout_while(arrived, Duration::from_secs(30), |arrived| {
                    *arrived < self.parties
                })
                .unwrap();
            *arrived >= self.parties
        }
    }

    fn overworld(x: i32, z: i32) -> RegionPos {
        RegionPos {
            dimension: DimensionId::Overworld,
            x,
            z,
        }
    }

    #[test]
    fn regions_are_32_by_32_chunks_on_both_sides_of_zero() {
        let region = |x, z| RegionPos::of_chunk(DimensionId::Overworld, ChunkPos::new(x, z));
        assert_eq!(region(0, 0), overworld(0, 0));
        assert_eq!(region(31, 31), overworld(0, 0));
        assert_eq!(region(32, -1), overworld(1, -1));
        assert_eq!(region(-32, -33), overworld(-1, -2));
        assert_eq!(
            RegionPos::of_block(DimensionId::Nether, -0.5, 511.9),
            RegionPos {
                dimension: DimensionId::Nether,
                x: -1,
                z: 0
            }
        );
        assert_eq!(
            RegionPos::of_block(DimensionId::Overworld, 512.0, -512.0),
            overworld(1, -1)
        );
    }

    fn visits_every_item_once_in_order(mode: ExecutionMode) {
        // Items 0..1000 interleaved over seven regions.
        let items: Vec<(RegionPos, usize)> = (0..1_000)
            .map(|i| (overworld(i as i32 % 7 - 3, 0), i))
            .collect();
        let mut expected_regions: Vec<RegionPos> = items.iter().map(|(at, _)| *at).collect();
        expected_regions.sort_by_key(RegionPos::sort_key);
        expected_regions.dedup();
        let visits: Vec<AtomicUsize> = (0..items.len()).map(|_| AtomicUsize::new(0)).collect();
        let seen_regions = Mutex::new(Vec::new());

        for_each_region(
            mode,
            items,
            |(at, _)| *at,
            |region, members| {
                seen_regions.lock().unwrap().push(region);
                let indices: Vec<usize> = members.iter().map(|(_, i)| *i).collect();
                assert!(
                    indices.is_sorted(),
                    "{region:?} lost the order of its items: {indices:?}"
                );
                for (at, i) in members.iter() {
                    assert_eq!(*at, region, "item {i} handed to the wrong region");
                    visits[*i].fetch_add(1, Ordering::SeqCst);
                }
            },
        );

        let wrong: Vec<(usize, usize)> = visits
            .iter()
            .map(|visits| visits.load(Ordering::SeqCst))
            .enumerate()
            .filter(|(_, count)| *count != 1)
            .collect();
        assert!(
            wrong.is_empty(),
            "{mode:?}: every item must be processed exactly once; (item, times): {wrong:?}"
        );
        let mut seen = seen_regions.into_inner().unwrap();
        if mode == ExecutionMode::MultiThreaded {
            // Tasks finish in any order.
            seen.sort_by_key(RegionPos::sort_key);
        }
        assert_eq!(seen, expected_regions, "{mode:?}: each region exactly once");
    }

    #[test]
    fn single_threaded_mode_visits_every_item_exactly_once() {
        visits_every_item_once_in_order(ExecutionMode::SingleThreaded);
    }

    #[test]
    fn multi_threaded_mode_visits_every_item_exactly_once() {
        visits_every_item_once_in_order(ExecutionMode::MultiThreaded);
    }

    #[test]
    fn multi_threaded_mode_processes_independent_regions_concurrently() {
        // Each region blocks until the other is in flight too, which only a
        // concurrent run satisfies; a serial run times out instead.
        let rendezvous = Rendezvous::new(2);
        let threads = Mutex::new(Vec::<ThreadId>::new());
        let met = AtomicUsize::new(0);

        for_each_region(
            ExecutionMode::MultiThreaded,
            vec![overworld(0, 0), overworld(-1, -1)],
            |at| *at,
            |_, _| {
                threads.lock().unwrap().push(thread::current().id());
                if rendezvous.arrive() {
                    met.fetch_add(1, Ordering::SeqCst);
                }
            },
        );

        assert_eq!(
            met.load(Ordering::SeqCst),
            2,
            "under MultiThreaded both regions must be in flight at the same time"
        );
        let threads = threads.into_inner().unwrap();
        assert_ne!(threads[0], threads[1], "the regions shared a thread");
    }

    #[test]
    fn single_threaded_mode_runs_regions_one_at_a_time_on_the_calling_thread() {
        let in_flight = AtomicUsize::new(0);
        let most_in_flight = AtomicUsize::new(0);
        let caller = thread::current().id();
        let order = Mutex::new(Vec::new());

        for_each_region(
            ExecutionMode::SingleThreaded,
            vec![overworld(1, 0), overworld(-1, -1), overworld(0, 5)],
            |at| *at,
            |region, _| {
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                most_in_flight.fetch_max(now, Ordering::SeqCst);
                assert_eq!(thread::current().id(), caller, "ran off the calling thread");
                order.lock().unwrap().push(region);
                in_flight.fetch_sub(1, Ordering::SeqCst);
            },
        );

        assert_eq!(most_in_flight.load(Ordering::SeqCst), 1);
        assert_eq!(
            order.into_inner().unwrap(),
            vec![overworld(-1, -1), overworld(0, 5), overworld(1, 0)],
            "regions run in their fixed order"
        );
    }
}
