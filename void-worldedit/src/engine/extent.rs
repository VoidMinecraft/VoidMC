use std::collections::{BTreeMap, HashMap};

use bevy_ecs::prelude::*;
use voidmc::players::WorldPlayers;
use voidmc::world::{CHUNK_MAX_Y, CHUNK_MIN_Y};
use voidmc::{ChunkData, ChunkDirty, ChunkIndex, ChunkPos, DimensionId};
use voidmc_protocol::clientbound::chunk::{ChunkSection, PaletteData};
use voidmc_protocol::clientbound::{BlockUpdate, SectionBlockChange, SectionBlocksUpdate};
use voidmc_protocol::types::BlockPosition;

use crate::block::{BlockState, VERSION};
use crate::extent::{ChangeMask, Extent, SectionBlocks};
use crate::math::{BlockPos, SECTION_VOLUME, SectionPos};

/// A chunk receiving at least this many changed blocks in one flush is
/// resent whole instead of as per-section updates: past ~6 full sections
/// the section packets outweigh the chunk packet.
pub const CHUNK_RESEND_THRESHOLD: usize = 6 * SECTION_VOLUME;

/// Touching this many sections of a chunk in one flush also resends it, so a
/// tall sparse edit costs one packet per chunk instead of one per section.
pub const CHUNK_RESEND_SECTIONS: usize = 8;

const MIN_SECTION_Y: i32 = CHUNK_MIN_Y >> 4;

#[derive(Default)]
struct ChunkChanges {
    sections: BTreeMap<i32, Vec<SectionBlockChange>>,
    touched: usize,
    changed: usize,
    resend: bool,
}

/// The VoidMC side of the [`Extent`] contract: reads and writes chunk
/// sections of one dimension in place, and batches the client updates of
/// everything written until [`flush`](Self::flush).
pub struct ChunkExtent<'w> {
    world: &'w mut World,
    dimension: DimensionId,
    pending: HashMap<ChunkPos, ChunkChanges>,
}

impl<'w> ChunkExtent<'w> {
    pub fn new(world: &'w mut World, dimension: DimensionId) -> Self {
        Self {
            world,
            dimension,
            pending: HashMap::new(),
        }
    }

    pub fn dimension(&self) -> DimensionId {
        self.dimension
    }

    fn chunk(&self, x: i32, z: i32) -> Option<Entity> {
        self.world
            .get_resource::<ChunkIndex>()?
            .0
            .get(&(self.dimension, ChunkPos::new(x, z)))
            .copied()
    }

    fn section(&self, section: SectionPos) -> Option<&ChunkSection> {
        let index = usize::try_from(section.y - MIN_SECTION_Y).ok()?;
        self.world
            .get::<ChunkData>(self.chunk(section.x, section.z)?)?
            .sections
            .get(index)
    }

    /// Sends every change written since the last flush to the players who
    /// have the chunk loaded.
    pub fn flush(&mut self) {
        let players = WorldPlayers::new(self.world);
        for (chunk, changes) in self.pending.drain() {
            let recipients = players.ready().seeing_chunk(self.dimension, chunk);
            if recipients.is_empty() {
                continue;
            }
            if changes.resend {
                let data = self
                    .world
                    .get_resource::<ChunkIndex>()
                    .and_then(|index| index.0.get(&(self.dimension, chunk)))
                    .and_then(|entity| self.world.get::<ChunkData>(*entity));
                if let Some(data) = data {
                    recipients.send(data.to_packet(chunk.x, chunk.z));
                    continue;
                }
            }
            for (section_y, blocks) in changes.sections {
                if let [single] = blocks.as_slice() {
                    recipients.send(BlockUpdate {
                        position: BlockPosition {
                            x: chunk.x * 16 + i32::from(single.x),
                            y: (section_y * 16 + i32::from(single.y)) as i16,
                            z: chunk.z * 16 + i32::from(single.z),
                        },
                        block_state_id: single.block_state_id,
                    });
                } else {
                    recipients.send(SectionBlocksUpdate {
                        section_x: chunk.x,
                        section_y,
                        section_z: chunk.z,
                        blocks,
                    });
                }
            }
        }
    }
}

impl Drop for ChunkExtent<'_> {
    fn drop(&mut self) {
        self.flush();
    }
}

impl Extent for ChunkExtent<'_> {
    fn height(&self) -> (i32, i32) {
        (CHUNK_MIN_Y, CHUNK_MAX_Y)
    }

    fn block(&self, pos: BlockPos) -> Option<BlockState> {
        let section = self.section(pos.section())?;
        Some(BlockState(section.get_block_state(
            (pos.x & 15) as u8,
            (pos.y & 15) as u8,
            (pos.z & 15) as u8,
        ) as u32))
    }

    fn read_section(&self, section: SectionPos, out: &mut SectionBlocks) -> bool {
        match self.section(section) {
            Some(data) => {
                decode_palette(&data.block_state, out);
                true
            }
            None => false,
        }
    }

    fn write_section(&mut self, section: SectionPos, blocks: &SectionBlocks, changed: &ChangeMask) {
        let Some(entity) = self.chunk(section.x, section.z) else {
            return;
        };
        let Ok(index) = usize::try_from(section.y - MIN_SECTION_Y) else {
            return;
        };
        let chunk = ChunkPos::new(section.x, section.z);
        let Some(mut data) = self.world.get_mut::<ChunkData>(entity) else {
            return;
        };
        if index >= data.sections.len() {
            return;
        }
        let stale: Vec<BlockPosition> = data
            .block_entities(chunk)
            .map(|(position, _)| position)
            .filter(|position| {
                let pos = BlockPos::new(position.x, i32::from(position.y), position.z);
                pos.section() == section && changed.contains(pos.section_index())
            })
            .collect();
        for position in stale {
            let pos = BlockPos::new(position.x, i32::from(position.y), position.z);
            data.set_block(
                (pos.x & 15) as u8,
                pos.y,
                (pos.z & 15) as u8,
                blocks[pos.section_index()].0 as i32,
            );
        }
        let target = &mut data.sections[index];
        target.block_state = encode_palette(blocks);
        target.block_count = blocks
            .as_slice()
            .iter()
            .filter(|s| **s != BlockState::AIR)
            .count() as i16;
        self.world.entity_mut(entity).insert(ChunkDirty);

        let pending = self.pending.entry(chunk).or_default();
        pending.changed += changed.count();
        pending.touched += 1;
        pending.resend |=
            pending.changed >= CHUNK_RESEND_THRESHOLD || pending.touched >= CHUNK_RESEND_SECTIONS;
        if pending.resend {
            pending.sections.clear();
            return;
        }
        pending
            .sections
            .entry(section.y)
            .or_default()
            .extend(changed.iter().map(|i| SectionBlockChange {
                x: (i & 15) as u8,
                y: (i >> 8) as u8,
                z: ((i >> 4) & 15) as u8,
                block_state_id: blocks[i].0 as i32,
            }));
    }
}

fn decode_palette(palette: &PaletteData, out: &mut SectionBlocks) {
    match palette {
        PaletteData::SingleValue(id) => out.fill(BlockState(*id as u32)),
        PaletteData::Indirect {
            bits_per_entry,
            palette,
            data,
        } => unpack(*bits_per_entry, data, out, |v| {
            BlockState(palette.get(v as usize).copied().unwrap_or(0) as u32)
        }),
        PaletteData::Direct {
            bits_per_entry,
            data,
        } => unpack(*bits_per_entry, data, out, |v| BlockState(v as u32)),
    }
}

fn unpack(bits: u8, data: &[u64], out: &mut SectionBlocks, map: impl Fn(u64) -> BlockState) {
    let bits = usize::from(bits.max(1));
    let per_long = 64 / bits;
    let mask = (1u64 << bits) - 1;
    let slots = out.as_mut_slice();
    for (long_index, chunk) in slots.chunks_mut(per_long).enumerate() {
        let long = data.get(long_index).copied().unwrap_or(0);
        for (offset, slot) in chunk.iter_mut().enumerate() {
            *slot = map((long >> (offset * bits)) & mask);
        }
    }
}

fn ceil_log2(count: usize) -> u8 {
    if count <= 1 {
        0
    } else {
        (usize::BITS - (count - 1).leading_zeros()) as u8
    }
}

/// Packs a section the way vanilla does: one value, an indirect palette of
/// 4..=8 bits, or global ids at `ceil(log2(state count))` bits.
pub(crate) fn encode_palette(blocks: &SectionBlocks) -> PaletteData {
    if let Some(state) = blocks.uniform() {
        return PaletteData::SingleValue(state.0 as i32);
    }
    let mut palette: Vec<u32> = Vec::new();
    let mut lookup: HashMap<u32, u16> = HashMap::new();
    let mut entries = [0u16; SECTION_VOLUME];
    let mut last = (u32::MAX, 0u16);
    for (slot, state) in entries.iter_mut().zip(blocks.as_slice()) {
        if state.0 != last.0 {
            let entry = *lookup.entry(state.0).or_insert_with(|| {
                palette.push(state.0);
                (palette.len() - 1) as u16
            });
            last = (state.0, entry);
        }
        *slot = last.1;
    }
    let bits = ceil_log2(palette.len()).max(4);
    if bits <= 8 {
        PaletteData::Indirect {
            bits_per_entry: bits,
            palette: palette.iter().map(|p| *p as i32).collect(),
            data: pack(bits, entries.iter().map(|e| u64::from(*e))),
        }
    } else {
        let bits = ceil_log2(voidmc_data::block_state_count(VERSION) as usize);
        PaletteData::Direct {
            bits_per_entry: bits,
            data: pack(bits, blocks.as_slice().iter().map(|s| u64::from(s.0))),
        }
    }
}

fn pack(bits: u8, values: impl Iterator<Item = u64>) -> Vec<u64> {
    let bits = usize::from(bits);
    let per_long = 64 / bits;
    let mut data = vec![0u64; SECTION_VOLUME.div_ceil(per_long)];
    for (index, value) in values.enumerate() {
        data[index / per_long] |= value << ((index % per_long) * bits);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(blocks: &SectionBlocks) -> SectionBlocks {
        let mut out = SectionBlocks::default();
        decode_palette(&encode_palette(blocks), &mut out);
        out
    }

    #[test]
    fn uniform_sections_become_single_value() {
        let blocks = SectionBlocks::filled(BlockState(1));
        assert_eq!(encode_palette(&blocks), PaletteData::SingleValue(1));
        assert_eq!(round_trip(&blocks), blocks);
    }

    #[test]
    fn small_palettes_use_at_least_four_bits() {
        let mut blocks = SectionBlocks::default();
        blocks[17] = BlockState(9);
        let PaletteData::Indirect {
            bits_per_entry,
            palette,
            data,
        } = encode_palette(&blocks)
        else {
            panic!("expected an indirect palette");
        };
        assert_eq!(bits_per_entry, 4);
        assert_eq!(palette, vec![0, 9]);
        assert_eq!(data.len(), 256);
        assert_eq!(round_trip(&blocks), blocks);
    }

    #[test]
    fn palettes_past_256_entries_switch_to_direct_ids() {
        let mut blocks = SectionBlocks::default();
        for index in 0..SECTION_VOLUME {
            blocks[index] = BlockState((index % 300) as u32 + 1);
        }
        let encoded = encode_palette(&blocks);
        assert_eq!(encoded.bits_per_entry(), 15);
        assert!(matches!(encoded, PaletteData::Direct { .. }));
        assert_eq!(round_trip(&blocks), blocks);

        let mut medium = SectionBlocks::default();
        for index in 0..SECTION_VOLUME {
            medium[index] = BlockState((index % 200) as u32);
        }
        assert_eq!(encode_palette(&medium).bits_per_entry(), 8);
        assert_eq!(round_trip(&medium), medium);
    }

    #[test]
    fn encoding_matches_the_engine_section_reader() {
        let mut blocks = SectionBlocks::default();
        for index in 0..SECTION_VOLUME {
            blocks[index] = BlockState((index % 37) as u32 * 3);
        }
        let section = ChunkSection {
            block_count: 0,
            block_state: encode_palette(&blocks),
            biome: PaletteData::SingleValue(0),
        };
        for index in [0usize, 1, 255, 256, 4095, 1234] {
            let (x, y, z) = (index & 15, index >> 8, (index >> 4) & 15);
            assert_eq!(
                section.get_block_state(x as u8, y as u8, z as u8),
                blocks[index].0 as i32
            );
        }
    }
}
