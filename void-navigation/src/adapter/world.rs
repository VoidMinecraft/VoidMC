use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc::events::BlockChangeEvent;
use voidmc::world::{
    CHUNK_MIN_Y, ChunkData, ChunkDimension, ChunkIndex, ChunkPos, ChunkPosition, DimensionId,
};
use voidmc_protocol::clientbound::chunk::{ChunkSection, PaletteData};

use super::blocks::{BlockModel, CellTable};
use crate::pathing::hash::FastMap;
use crate::pathing::{
    BlockPos, CachedWorld, Cell, CellCache, CellSource, SECTION_CELLS, SectionCells, SectionPos,
};

/// Navigation cells for every dimension, built lazily from chunk data. A
/// single block change patches one cell; any other chunk change or an unload
/// drops the chunk's cached sections.
#[derive(Resource)]
pub struct NavigationWorld {
    table: CellTable,
    caches: FastMap<DimensionId, CellCache>,
    stamp: u64,
    changes: FastMap<(DimensionId, ChunkPos), u64>,
    patched: Vec<(DimensionId, ChunkPos)>,
}

impl NavigationWorld {
    pub fn new(model: BlockModel) -> Self {
        Self {
            table: CellTable::new(model),
            caches: FastMap::default(),
            stamp: 0,
            changes: FastMap::default(),
            patched: Vec::new(),
        }
    }

    pub fn table(&self) -> &CellTable {
        &self.table
    }

    pub fn cache(&self, dimension: DimensionId) -> Option<&CellCache> {
        self.caches.get(&dimension)
    }

    pub fn cached_sections(&self) -> usize {
        self.caches
            .values()
            .map(|cache| cache.stats().sections)
            .sum()
    }

    pub fn memory_bytes(&self) -> usize {
        self.caches.values().map(CellCache::memory_bytes).sum()
    }

    /// Increases whenever cached cells change. Compare with
    /// [`Self::changed_since`] to tell whether results derived from cells read
    /// at a given stamp (cached paths) are still current.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// Whether a cached cell of any chunk between `min` and `max` (inclusive)
    /// changed after `stamp`.
    pub fn changed_since(
        &self,
        dimension: DimensionId,
        min: ChunkPos,
        max: ChunkPos,
        stamp: u64,
    ) -> bool {
        if self.stamp <= stamp {
            return false;
        }
        let area = (max.x - min.x + 1) as usize * (max.z - min.z + 1) as usize;
        if area > self.changes.len() {
            return self.changes.iter().any(|(&(d, chunk), &changed)| {
                changed > stamp
                    && d == dimension
                    && (min.x..=max.x).contains(&chunk.x)
                    && (min.z..=max.z).contains(&chunk.z)
            });
        }
        (min.z..=max.z).any(|z| {
            (min.x..=max.x).any(|x| {
                self.changes
                    .get(&(dimension, ChunkPos::new(x, z)))
                    .is_some_and(|&changed| changed > stamp)
            })
        })
    }

    fn touch(&mut self, dimension: DimensionId, chunk: ChunkPos) {
        self.stamp += 1;
        self.changes.insert((dimension, chunk), self.stamp);
    }

    pub fn invalidate_chunk(&mut self, dimension: DimensionId, chunk: ChunkPos) {
        let dropped = self
            .caches
            .get_mut(&dimension)
            .map_or(0, |cache| cache.invalidate_column(chunk.x, chunk.z));
        if dropped > 0 {
            self.touch(dimension, chunk);
        }
    }

    /// Re-reads one block. Returns false when its section was not cached.
    pub fn set_block(&mut self, dimension: DimensionId, pos: BlockPos, state: i32) -> bool {
        let cell = self.table.get(state);
        let Some(cache) = self.caches.get_mut(&dimension) else {
            return false;
        };
        if cache.cached(pos) == Some(cell) {
            return true;
        }
        if !cache.set_cell(pos, cell) {
            return false;
        }
        self.touch(dimension, ChunkPos::new(pos.x >> 4, pos.z >> 4));
        true
    }

    pub fn view<'a, 'w, 's>(
        &'a mut self,
        dimension: DimensionId,
        chunks: &'a ChunkCells<'w, 's>,
    ) -> CachedWorld<'a, DimensionCells<'a, 'w, 's>> {
        let cache = self.caches.entry(dimension).or_default();
        let source = chunks.in_dimension(dimension, &self.table);
        cache.view(source)
    }
}

/// Read access to loaded chunks, as one system parameter.
#[derive(bevy_ecs::system::SystemParam)]
pub struct ChunkCells<'w, 's> {
    index: Res<'w, ChunkIndex>,
    chunks: Query<'w, 's, &'static ChunkData>,
}

impl<'w, 's> ChunkCells<'w, 's> {
    fn in_dimension<'a>(
        &'a self,
        dimension: DimensionId,
        table: &'a CellTable,
    ) -> DimensionCells<'a, 'w, 's> {
        DimensionCells {
            cells: self,
            dimension,
            table,
        }
    }

    pub fn is_loaded(&self, dimension: DimensionId, chunk: ChunkPos) -> bool {
        self.index.0.contains_key(&(dimension, chunk))
    }
}

pub struct DimensionCells<'a, 'w, 's> {
    cells: &'a ChunkCells<'w, 's>,
    dimension: DimensionId,
    table: &'a CellTable,
}

impl CellSource for DimensionCells<'_, '_, '_> {
    fn fill_section(&self, section: SectionPos, cells: &mut SectionCells) -> bool {
        let chunk = ChunkPos::new(section.x, section.z);
        let Some(&entity) = self.cells.index.0.get(&(self.dimension, chunk)) else {
            return false;
        };
        let Ok(data) = self.cells.chunks.get(entity) else {
            return false;
        };
        let index = section.y - (CHUNK_MIN_Y >> 4);
        if index < 0 {
            return false;
        }
        match data.sections.get(index as usize) {
            Some(blocks) => fill_from_section(self.table, blocks, cells),
            None => cells.fill(Cell::EMPTY),
        }
        true
    }
}

fn fill_from_section(table: &CellTable, section: &ChunkSection, cells: &mut SectionCells) {
    match &section.block_state {
        PaletteData::SingleValue(state) => cells.fill(table.get(*state)),
        PaletteData::Indirect {
            bits_per_entry,
            palette,
            data,
        } => {
            let mut mapped = [Cell::EMPTY; 256];
            for (slot, &state) in mapped.iter_mut().zip(palette) {
                *slot = table.get(state);
            }
            unpack(*bits_per_entry, data, cells, |entry| {
                mapped.get(entry as usize).copied().unwrap_or(Cell::EMPTY)
            });
        }
        PaletteData::Direct {
            bits_per_entry,
            data,
        } => unpack(*bits_per_entry, data, cells, |entry| {
            table.get(entry as i32)
        }),
    }
}

fn unpack(bits: u8, data: &[u64], cells: &mut SectionCells, mut resolve: impl FnMut(u64) -> Cell) {
    if bits == 0 {
        cells.fill(resolve(0));
        return;
    }
    let bits = bits as usize;
    let per_long = 64 / bits;
    let mask = (1u64 << bits) - 1;
    let mut index = 0;
    for &long in data {
        let mut word = long;
        for _ in 0..per_long {
            if index == SECTION_CELLS {
                return;
            }
            cells[index] = resolve(word & mask);
            word >>= bits;
            index += 1;
        }
    }
    cells[index..].fill(resolve(0));
}

pub(crate) fn invalidate_changed_chunks(
    mut world: ResMut<NavigationWorld>,
    changed: Query<(&ChunkPosition, &ChunkDimension), Changed<ChunkData>>,
) {
    let world = world.as_mut();
    let patched = std::mem::take(&mut world.patched);
    for (position, dimension) in &changed {
        if !patched.contains(&(dimension.0, position.0)) {
            world.invalidate_chunk(dimension.0, position.0);
        }
    }
    world.patched = patched;
    world.patched.clear();
}

pub(crate) fn patch_changed_block(event: On<BlockChangeEvent>, mut world: ResMut<NavigationWorld>) {
    let pos = BlockPos::new(event.position.x, event.position.y as i32, event.position.z);
    world.set_block(event.dimension, pos, event.new_state);
    let chunk = (event.dimension, ChunkPos::new(pos.x >> 4, pos.z >> 4));
    if !world.patched.contains(&chunk) {
        world.patched.push(chunk);
    }
}

pub(crate) fn evict_unloaded_chunk(
    event: On<Remove, ChunkData>,
    mut world: ResMut<NavigationWorld>,
    chunks: Query<(&ChunkPosition, &ChunkDimension)>,
) {
    if let Ok((position, dimension)) = chunks.get(event.entity) {
        world.invalidate_chunk(dimension.0, position.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathing::cell_index;

    #[test]
    fn unpacks_like_the_section_reader() {
        let table = CellTable::new(BlockModel::Vanilla);
        let mut section = ChunkSection::filled(voidmc_data::v26_1_2::blocks::STONE, 0);
        let slab = voidmc_data::v26_1_2::blocks::OAK_SLAB;
        let water = voidmc_data::v26_1_2::blocks::WATER;
        for (x, y, z) in [(0, 0, 0), (15, 15, 15), (3, 7, 9)] {
            section.set_block_state(x, y, z, slab);
        }
        for i in 0..40u8 {
            section.set_block_state(i % 16, i / 16, 5, water);
            section.set_block_state(i % 16, 9, i / 16, voidmc_data::v26_1_2::blocks::AIR);
        }
        let mut cells = [Cell::EMPTY; SECTION_CELLS];
        fill_from_section(&table, &section, &mut cells);
        for y in 0..16u8 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    let expected = table.get(section.get_block_state(x, y, z));
                    let index = cell_index(BlockPos::new(x as i32, y as i32, z as i32));
                    assert_eq!(cells[index], expected, "at {x} {y} {z}");
                }
            }
        }
    }
}
