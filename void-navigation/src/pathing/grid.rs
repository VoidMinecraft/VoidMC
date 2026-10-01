use super::cell::Cell;
use super::hash::FastMap;
use super::math::BlockPos;

pub const SECTION_CELLS: usize = 16 * 16 * 16;

const RECENT: usize = 8;
const NO_SLOT: u32 = u32::MAX;
const NO_RECENT: (SectionPos, u32) = (SectionPos::new(i32::MIN, i32::MIN, i32::MIN), NO_SLOT);

#[inline]
const fn recent_slot(section: SectionPos) -> usize {
    ((section.x & 1) | (section.y & 1) << 1 | (section.z & 1) << 2) as usize
}

pub type SectionCells = [Cell; SECTION_CELLS];

/// The world queries the pathfinder needs: one packed [`Cell`] per block.
/// Implementations are expected to be cheap; [`CellCache`] turns any
/// section-granular [`CellSource`] into one.
pub trait NavWorld {
    fn cell(&mut self, pos: BlockPos) -> Cell;
}

/// Produces the cells of one 16³ section, indexed by [`cell_index`]. Returns
/// `false` when the section is not loaded, in which case the cache answers
/// [`Cell::UNLOADED`] and asks again next time.
pub trait CellSource {
    fn fill_section(&self, section: SectionPos, cells: &mut SectionCells) -> bool;
}

impl<T: CellSource + ?Sized> CellSource for &T {
    fn fill_section(&self, section: SectionPos, cells: &mut SectionCells) -> bool {
        (**self).fill_section(section, cells)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SectionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl SectionPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    #[inline]
    pub const fn of(pos: BlockPos) -> Self {
        Self::new(pos.x >> 4, pos.y >> 4, pos.z >> 4)
    }

    pub const fn min_block(self) -> BlockPos {
        BlockPos::new(self.x << 4, self.y << 4, self.z << 4)
    }
}

#[inline]
pub const fn cell_index(pos: BlockPos) -> usize {
    ((pos.y & 15) << 8 | (pos.z & 15) << 4 | (pos.x & 15)) as usize
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub sections: usize,
    pub fills: u64,
    pub unloaded_queries: u64,
}

/// Lazily built per-section cell arrays. Sections are filled on first query
/// and patched in place on block changes ([`CellCache::set_cell`]), so a
/// steady-state search never touches the block storage.
pub struct CellCache {
    slots: Vec<Box<SectionCells>>,
    free: Vec<u32>,
    index: FastMap<SectionPos, u32>,
    recent: [(SectionPos, u32); RECENT],
    fills: u64,
    unloaded_queries: u64,
}

impl Default for CellCache {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            index: FastMap::default(),
            recent: [NO_RECENT; RECENT],
            fills: 0,
            unloaded_queries: 0,
        }
    }
}

impl CellCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn forget_recent(&mut self) {
        self.recent = [NO_RECENT; RECENT];
    }

    pub fn view<S: CellSource>(&mut self, source: S) -> CachedWorld<'_, S> {
        CachedWorld {
            cache: self,
            source,
        }
    }

    pub fn is_cached(&self, section: SectionPos) -> bool {
        self.index.contains_key(&section)
    }

    /// Patches one cell of a cached section; uncached sections are left to be
    /// filled fresh. Returns whether a cached section was touched.
    pub fn set_cell(&mut self, pos: BlockPos, cell: Cell) -> bool {
        match self.index.get(&SectionPos::of(pos)) {
            Some(&slot) => {
                self.slots[slot as usize][cell_index(pos)] = cell;
                true
            }
            None => false,
        }
    }

    pub fn invalidate_section(&mut self, section: SectionPos) {
        if let Some(slot) = self.index.remove(&section) {
            self.free.push(slot);
            self.forget_recent();
        }
    }

    /// Drops every cached section of the 16-wide column `(x, z)` in section
    /// coordinates, e.g. when its chunk unloads.
    pub fn invalidate_column(&mut self, x: i32, z: i32) {
        let free = &mut self.free;
        self.index.retain(|section, slot| {
            let keep = section.x != x || section.z != z;
            if !keep {
                free.push(*slot);
            }
            keep
        });
        self.forget_recent();
    }

    pub fn clear(&mut self) {
        self.free.extend(self.index.drain().map(|(_, slot)| slot));
        self.forget_recent();
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            sections: self.index.len(),
            fills: self.fills,
            unloaded_queries: self.unloaded_queries,
        }
    }

    pub fn memory_bytes(&self) -> usize {
        self.slots.len() * std::mem::size_of::<SectionCells>()
    }

    fn take_slot(&mut self) -> u32 {
        match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.slots.push(Box::new([Cell::EMPTY; SECTION_CELLS]));
                (self.slots.len() - 1) as u32
            }
        }
    }
}

pub struct CachedWorld<'a, S> {
    cache: &'a mut CellCache,
    source: S,
}

impl<S: CellSource> CachedWorld<'_, S> {
    #[cold]
    fn miss(&mut self, section: SectionPos, pos: BlockPos) -> Cell {
        if let Some(&slot) = self.cache.index.get(&section) {
            self.cache.recent[recent_slot(section)] = (section, slot);
            return self.cache.slots[slot as usize][cell_index(pos)];
        }
        let slot = self.cache.take_slot();
        if self
            .source
            .fill_section(section, &mut self.cache.slots[slot as usize])
        {
            self.cache.fills += 1;
            self.cache.index.insert(section, slot);
            self.cache.recent[recent_slot(section)] = (section, slot);
            self.cache.slots[slot as usize][cell_index(pos)]
        } else {
            self.cache.free.push(slot);
            self.cache.unloaded_queries += 1;
            Cell::UNLOADED
        }
    }
}

impl<S: CellSource> NavWorld for CachedWorld<'_, S> {
    #[inline]
    fn cell(&mut self, pos: BlockPos) -> Cell {
        let section = SectionPos::of(pos);
        let (recent, slot) = self.cache.recent[recent_slot(section)];
        if recent == section {
            return self.cache.slots[slot as usize][cell_index(pos)];
        }
        self.miss(section, pos)
    }
}

/// A dense in-memory world: the reference [`NavWorld`]/[`CellSource`] for
/// tests, benchmarks and engines without their own block storage. Everything
/// outside the box reads as `outside`.
#[derive(Clone, Debug)]
pub struct ArrayWorld {
    min: BlockPos,
    size: [i32; 3],
    cells: Vec<Cell>,
    outside: Cell,
}

impl ArrayWorld {
    pub fn new(min: BlockPos, size_x: i32, size_y: i32, size_z: i32) -> Self {
        assert!(size_x > 0 && size_y > 0 && size_z > 0);
        Self {
            min,
            size: [size_x, size_y, size_z],
            cells: vec![Cell::EMPTY; (size_x * size_y * size_z) as usize],
            outside: Cell::UNLOADED,
        }
    }

    pub fn with_outside(mut self, outside: Cell) -> Self {
        self.outside = outside;
        self
    }

    pub fn min(&self) -> BlockPos {
        self.min
    }

    pub fn max(&self) -> BlockPos {
        self.min
            .offset(self.size[0] - 1, self.size[1] - 1, self.size[2] - 1)
    }

    #[inline]
    fn slot(&self, pos: BlockPos) -> Option<usize> {
        let x = pos.x - self.min.x;
        let y = pos.y - self.min.y;
        let z = pos.z - self.min.z;
        if x < 0 || y < 0 || z < 0 || x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return None;
        }
        Some(((y * self.size[2] + z) * self.size[0] + x) as usize)
    }

    pub fn get(&self, pos: BlockPos) -> Cell {
        self.slot(pos).map_or(self.outside, |slot| self.cells[slot])
    }

    pub fn set(&mut self, pos: BlockPos, cell: Cell) {
        if let Some(slot) = self.slot(pos) {
            self.cells[slot] = cell;
        }
    }

    pub fn fill(&mut self, from: BlockPos, to: BlockPos, cell: Cell) {
        for y in from.y.min(to.y)..=from.y.max(to.y) {
            for z in from.z.min(to.z)..=from.z.max(to.z) {
                for x in from.x.min(to.x)..=from.x.max(to.x) {
                    self.set(BlockPos::new(x, y, z), cell);
                }
            }
        }
    }
}

impl NavWorld for ArrayWorld {
    #[inline]
    fn cell(&mut self, pos: BlockPos) -> Cell {
        self.get(pos)
    }
}

impl CellSource for ArrayWorld {
    fn fill_section(&self, section: SectionPos, cells: &mut SectionCells) -> bool {
        let base = section.min_block();
        let max = self.max();
        let inside = base.x + 15 >= self.min.x
            && base.x <= max.x
            && base.y + 15 >= self.min.y
            && base.y <= max.y
            && base.z + 15 >= self.min.z
            && base.z <= max.z;
        if !inside && self.outside.is_unloaded() {
            return false;
        }
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let pos = base.offset(x, y, z);
                    cells[cell_index(pos)] = self.get(pos);
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_matches_source_and_patches_in_place() {
        let mut world = ArrayWorld::new(BlockPos::new(-20, 0, -20), 40, 32, 40);
        world.set(BlockPos::new(-17, 3, 5), Cell::FULL);
        let mut cache = CellCache::new();
        {
            let mut view = cache.view(&world);
            assert_eq!(view.cell(BlockPos::new(-17, 3, 5)), Cell::FULL);
            assert_eq!(view.cell(BlockPos::new(-17, 4, 5)), Cell::EMPTY);
            assert_eq!(view.cell(BlockPos::new(500, 4, 5)), Cell::UNLOADED);
        }
        assert!(cache.set_cell(BlockPos::new(-17, 3, 5), Cell::EMPTY));
        assert_eq!(
            cache.view(&world).cell(BlockPos::new(-17, 3, 5)),
            Cell::EMPTY
        );
        assert!(!cache.set_cell(BlockPos::new(200, 3, 5), Cell::FULL));
    }

    #[test]
    fn invalidation_releases_slots_for_reuse() {
        let world = ArrayWorld::new(BlockPos::new(0, 0, 0), 64, 16, 16);
        let mut cache = CellCache::new();
        for x in 0..4 {
            cache.view(&world).cell(BlockPos::new(x * 16, 0, 0));
        }
        assert_eq!(cache.stats().sections, 4);
        cache.invalidate_column(1, 0);
        cache.invalidate_section(SectionPos::new(2, 0, 0));
        assert_eq!(cache.stats().sections, 2);
        cache.view(&world).cell(BlockPos::new(16, 0, 0));
        cache.view(&world).cell(BlockPos::new(32, 0, 0));
        assert_eq!(
            cache.memory_bytes(),
            4 * std::mem::size_of::<SectionCells>()
        );
    }

    #[test]
    fn unloaded_sections_are_retried() {
        let world = ArrayWorld::new(BlockPos::new(0, 0, 0), 16, 16, 16);
        let mut cache = CellCache::new();
        let mut view = cache.view(&world);
        assert!(view.cell(BlockPos::new(-1, 0, 0)).is_unloaded());
        assert!(view.cell(BlockPos::new(-1, 0, 0)).is_unloaded());
        assert_eq!(cache.stats().unloaded_queries, 2);
        assert_eq!(cache.stats().sections, 0);
    }
}
