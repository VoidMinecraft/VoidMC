//! Conversion between in-memory [`ChunkData`] and the on-disk NBT representation.
//!
//! The format is *not* vanilla-compatible; it is a compact, self-describing
//! mirror of VoidMC's own chunk types built on standard (named-root) NBT via
//! `ussr_nbt`. Compression and region packing are handled by the region store —
//! this module deals only in uncompressed NBT bytes.

use ussr_nbt::endian::RawVec;
use ussr_nbt::owned::{Compound, List, Nbt, Tag};
use voidmc::{BlockEntity, BlockEntityKind, ChunkData, ChunkPos, DimensionId};
use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData, PaletteData};
use voidmc_protocol::types::BlockPosition;

use crate::error::{PersistenceError, Result};

/// On-disk format version. Bump on incompatible schema changes.
pub const FORMAT_VERSION: i32 = 1;

/// A chunk decoded from disk, ready to be inserted into the ECS.
pub struct LoadedChunk {
    pub dimension: DimensionId,
    pub x: i32,
    pub z: i32,
    pub data: ChunkData,
}

/// Serializes a chunk to uncompressed NBT bytes.
///
/// When `include_light` is false, the light section is omitted (it will be
/// regenerated on load).
pub fn serialize_chunk(
    dimension: DimensionId,
    x: i32,
    z: i32,
    data: &ChunkData,
    include_light: bool,
) -> Result<Vec<u8>> {
    let nbt = chunk_to_nbt(dimension, x, z, data, include_light);
    let mut bytes = Vec::new();
    nbt.write(&mut bytes)
        .map_err(|e| PersistenceError::Corrupt(format!("nbt write failed: {e}")))?;
    Ok(bytes)
}

/// Deserializes a chunk from uncompressed NBT bytes.
///
/// When the stored chunk has no light section, light is regenerated as full sky.
pub fn deserialize_chunk(bytes: &[u8]) -> Result<LoadedChunk> {
    let mut cursor = bytes;
    let nbt = Nbt::read(&mut cursor)
        .map_err(|e| PersistenceError::Corrupt(format!("nbt read failed: {e}")))?;
    chunk_from_nbt(&nbt)
}

fn chunk_to_nbt(
    dimension: DimensionId,
    x: i32,
    z: i32,
    data: &ChunkData,
    include_light: bool,
) -> Nbt {
    let sections: Vec<Compound> = data.sections.iter().map(section_to_nbt).collect();
    let block_entities: Vec<Compound> = data
        .block_entities(ChunkPos::new(x, z))
        .map(|(position, block_entity)| block_entity_to_nbt(position, block_entity))
        .collect();

    let mut tags: Vec<(ussr_nbt::mutf8::MString, Tag)> = vec![
        ("version".into(), Tag::Int(FORMAT_VERSION)),
        ("dimension".into(), Tag::Int(dimension.protocol_id())),
        ("x".into(), Tag::Int(x)),
        ("z".into(), Tag::Int(z)),
        ("sections".into(), Tag::List(List::Compound(sections))),
        (
            "heightmaps".into(),
            Tag::Compound(heightmaps_to_nbt(&data.heightmaps)),
        ),
        (
            "block_entities".into(),
            Tag::List(compound_list(block_entities)),
        ),
    ];

    if include_light {
        tags.push(("light".into(), Tag::Compound(light_to_nbt(&data.light))));
    }

    Nbt {
        name: "".into(),
        compound: Compound { tags },
    }
}

fn chunk_from_nbt(nbt: &Nbt) -> Result<LoadedChunk> {
    let c = &nbt.compound;
    let dimension = dim_from_protocol_id(get_int(c, "dimension")?)?;
    let x = get_int(c, "x")?;
    let z = get_int(c, "z")?;

    let sections = match field(c, "sections") {
        Some(Tag::List(List::Compound(list))) => list
            .iter()
            .map(section_from_nbt)
            .collect::<Result<Vec<_>>>()?,
        Some(Tag::List(List::Empty)) => Vec::new(),
        _ => return Err(corrupt("missing/invalid sections")),
    };

    let heightmaps = match field(c, "heightmaps") {
        Some(Tag::Compound(hc)) => heightmaps_from_nbt(hc)?,
        _ => ChunkHeightmaps::empty(),
    };

    let light = match field(c, "light") {
        Some(Tag::Compound(lc)) => light_from_nbt(lc)?,
        // No light stored -> regenerate full sky light.
        _ => LightData::full_sky_light(),
    };

    let mut data = ChunkData::new(sections, heightmaps, light);
    match field(c, "block_entities") {
        Some(Tag::List(List::Compound(list))) => data.restore_block_entities(
            ChunkPos::new(x, z),
            list.iter()
                .map(block_entity_from_nbt)
                .collect::<Result<Vec<_>>>()?,
        ),
        Some(Tag::List(List::Empty)) | None => {}
        _ => return Err(corrupt("invalid block_entities")),
    }

    Ok(LoadedChunk {
        dimension,
        x,
        z,
        data,
    })
}

// ---- block entities -------------------------------------------------------

// Anvil shape: `id`, `x`, `y`, `z` first, then the block entity's own tags,
// so loading can strip exactly the four written here even if the payload
// carries keys of the same name.
const METADATA_TAGS: usize = 4;

fn block_entity_to_nbt(position: BlockPosition, block_entity: &BlockEntity) -> Compound {
    let mut tags = vec![
        ("id".into(), Tag::String(block_entity.kind().name().into())),
        ("x".into(), Tag::Int(position.x)),
        ("y".into(), Tag::Int(position.y as i32)),
        ("z".into(), Tag::Int(position.z)),
    ];
    tags.extend(block_entity.data().tags.iter().cloned());
    Compound { tags }
}

fn block_entity_from_nbt(c: &Compound) -> Result<(BlockPosition, BlockEntity)> {
    let names: Vec<_> = c
        .tags
        .iter()
        .take(METADATA_TAGS)
        .map(|(name, _)| name.decode().map(|n| n.into_owned()))
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| corrupt("block entity metadata"))?;
    if names != ["id", "x", "y", "z"] {
        return Err(corrupt("block entity metadata"));
    }
    let id = get_string(c, "id")?;
    let kind = BlockEntityKind::from_name(&id)
        .ok_or_else(|| PersistenceError::Corrupt(format!("unknown block entity type {id}")))?;
    let position = BlockPosition {
        x: get_int(c, "x")?,
        y: get_int(c, "y")? as i16,
        z: get_int(c, "z")?,
    };
    let tags = c.tags[METADATA_TAGS..].to_vec();
    Ok((position, BlockEntity::raw(kind, Compound { tags })))
}

// ---- sections -------------------------------------------------------------

fn section_to_nbt(section: &ChunkSection) -> Compound {
    Compound {
        tags: vec![
            ("block_count".into(), Tag::Short(section.block_count)),
            (
                "block_states".into(),
                Tag::Compound(palette_to_nbt(&section.block_state)),
            ),
            (
                "biome".into(),
                Tag::Compound(palette_to_nbt(&section.biome)),
            ),
        ],
    }
}

fn section_from_nbt(c: &Compound) -> Result<ChunkSection> {
    let block_count = get_short(c, "block_count")?;
    let block_state = match field(c, "block_states") {
        Some(Tag::Compound(pc)) => palette_from_nbt(pc)?,
        _ => return Err(corrupt("missing block_states")),
    };
    let biome = match field(c, "biome") {
        Some(Tag::Compound(pc)) => palette_from_nbt(pc)?,
        _ => return Err(corrupt("missing biome")),
    };
    Ok(ChunkSection {
        block_count,
        block_state,
        biome,
    })
}

fn palette_to_nbt(palette: &PaletteData) -> Compound {
    match palette {
        PaletteData::SingleValue(id) => Compound {
            tags: vec![("single".into(), Tag::Int(*id))],
        },
        PaletteData::Indirect {
            bits_per_entry,
            palette,
            data,
        } => Compound {
            tags: vec![
                ("bits".into(), Tag::Byte(*bits_per_entry)),
                (
                    "palette".into(),
                    Tag::IntArray(RawVec::from_vec(palette.clone())),
                ),
                (
                    "data".into(),
                    Tag::LongArray(RawVec::from_vec(data.iter().map(|&v| v as i64).collect())),
                ),
            ],
        },
        PaletteData::Direct {
            bits_per_entry,
            data,
        } => Compound {
            tags: vec![
                ("bits".into(), Tag::Byte(*bits_per_entry)),
                (
                    "data".into(),
                    Tag::LongArray(RawVec::from_vec(data.iter().map(|&v| v as i64).collect())),
                ),
            ],
        },
    }
}

fn palette_from_nbt(c: &Compound) -> Result<PaletteData> {
    if let Some(bits) = get_byte_opt(c, "bits") {
        let data = get_long_array(c, "data")?
            .into_iter()
            .map(|v| v as u64)
            .collect();
        if field(c, "palette").is_none() {
            return Ok(PaletteData::Direct {
                bits_per_entry: bits,
                data,
            });
        }
        let palette = get_int_array(c, "palette")?;
        Ok(PaletteData::Indirect {
            bits_per_entry: bits,
            palette,
            data,
        })
    } else {
        Ok(PaletteData::SingleValue(get_int(c, "single")?))
    }
}

// ---- heightmaps & light ---------------------------------------------------

fn heightmaps_to_nbt(h: &ChunkHeightmaps) -> Compound {
    Compound {
        tags: vec![(
            "motion_blocking".into(),
            Tag::LongArray(RawVec::from_vec(h.motion_blocking.clone())),
        )],
    }
}

fn heightmaps_from_nbt(c: &Compound) -> Result<ChunkHeightmaps> {
    Ok(ChunkHeightmaps {
        motion_blocking: get_long_array(c, "motion_blocking").unwrap_or_else(|_| vec![0i64; 37]),
    })
}

fn light_to_nbt(l: &LightData) -> Compound {
    Compound {
        tags: vec![
            ("sky_mask".into(), u64_long_array(&l.sky_light_mask)),
            ("block_mask".into(), u64_long_array(&l.block_light_mask)),
            (
                "empty_sky_mask".into(),
                u64_long_array(&l.empty_sky_light_mask),
            ),
            (
                "empty_block_mask".into(),
                u64_long_array(&l.empty_block_light_mask),
            ),
            (
                "sky_arrays".into(),
                Tag::List(byte_array_list(&l.sky_light_arrays)),
            ),
            (
                "block_arrays".into(),
                Tag::List(byte_array_list(&l.block_light_arrays)),
            ),
        ],
    }
}

fn light_from_nbt(c: &Compound) -> Result<LightData> {
    Ok(LightData {
        sky_light_mask: long_array_u64(c, "sky_mask")?,
        block_light_mask: long_array_u64(c, "block_mask")?,
        empty_sky_light_mask: long_array_u64(c, "empty_sky_mask")?,
        empty_block_light_mask: long_array_u64(c, "empty_block_mask")?,
        sky_light_arrays: byte_array_list_from(c, "sky_arrays"),
        block_light_arrays: byte_array_list_from(c, "block_arrays"),
    })
}

// ---- small NBT helpers ----------------------------------------------------

fn corrupt(msg: &str) -> PersistenceError {
    PersistenceError::Corrupt(msg.to_string())
}

fn field<'a>(c: &'a Compound, key: &str) -> Option<&'a Tag> {
    c.tags
        .iter()
        .find(|(n, _)| n.decode().map(|s| s == key).unwrap_or(false))
        .map(|(_, t)| t)
}

fn get_int(c: &Compound, key: &str) -> Result<i32> {
    match field(c, key) {
        Some(Tag::Int(v)) => Ok(*v),
        _ => Err(corrupt(key)),
    }
}

fn get_string(c: &Compound, key: &str) -> Result<String> {
    match field(c, key) {
        Some(Tag::String(v)) => v.decode().map(|s| s.into_owned()).map_err(|_| corrupt(key)),
        _ => Err(corrupt(key)),
    }
}

fn get_short(c: &Compound, key: &str) -> Result<i16> {
    match field(c, key) {
        Some(Tag::Short(v)) => Ok(*v),
        _ => Err(corrupt(key)),
    }
}

fn get_byte_opt(c: &Compound, key: &str) -> Option<u8> {
    match field(c, key) {
        Some(Tag::Byte(v)) => Some(*v),
        _ => None,
    }
}

fn get_int_array(c: &Compound, key: &str) -> Result<Vec<i32>> {
    match field(c, key) {
        Some(Tag::IntArray(v)) => Ok(v.to_vec()),
        _ => Err(corrupt(key)),
    }
}

fn get_long_array(c: &Compound, key: &str) -> Result<Vec<i64>> {
    match field(c, key) {
        Some(Tag::LongArray(v)) => Ok(v.to_vec()),
        _ => Err(corrupt(key)),
    }
}

fn u64_long_array(v: &[u64]) -> Tag {
    Tag::LongArray(RawVec::from_vec(v.iter().map(|&x| x as i64).collect()))
}

fn long_array_u64(c: &Compound, key: &str) -> Result<Vec<u64>> {
    Ok(get_long_array(c, key)?
        .into_iter()
        .map(|x| x as u64)
        .collect())
}

fn compound_list(compounds: Vec<Compound>) -> List {
    if compounds.is_empty() {
        List::Empty
    } else {
        List::Compound(compounds)
    }
}

fn byte_array_list(arrays: &[Vec<u8>]) -> List {
    if arrays.is_empty() {
        List::Empty
    } else {
        List::ByteArray(arrays.to_vec())
    }
}

fn byte_array_list_from(c: &Compound, key: &str) -> Vec<Vec<u8>> {
    match field(c, key) {
        Some(Tag::List(List::ByteArray(v))) => v.clone(),
        _ => Vec::new(),
    }
}

fn dim_from_protocol_id(id: i32) -> Result<DimensionId> {
    match id {
        0 => Ok(DimensionId::Overworld),
        1 => Ok(DimensionId::Nether),
        2 => Ok(DimensionId::End),
        other => Err(PersistenceError::Corrupt(format!(
            "unknown dimension id {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voidmc_protocol::clientbound::chunk::blocks;

    fn sample_chunk() -> ChunkData {
        let mut sections: Vec<ChunkSection> = (0..24).map(|_| ChunkSection::empty()).collect();
        // Single-value (air) -> promote to indirect [air, oak] in one section.
        sections[8].set_block_state(3, 7, 11, 99);
        // A filled (single-value) section.
        sections[4] = ChunkSection::filled(blocks::STONE, 1);
        // Force palette growth past 16 entries in another section.
        for i in 0..20u8 {
            let (x, z) = (i % 16, i / 16);
            sections[10].set_block_state(x, 1, z, 200 + i as i32);
        }
        ChunkData::new(
            sections,
            ChunkHeightmaps::flat(70),
            LightData::full_sky_light(),
        )
    }

    fn assert_sections_eq(a: &ChunkData, b: &ChunkData) {
        assert_eq!(a.sections.len(), b.sections.len());
        for (sa, sb) in a.sections.iter().zip(b.sections.iter()) {
            assert_eq!(sa.block_count, sb.block_count);
            for y in 0..16u8 {
                for z in 0..16u8 {
                    for x in 0..16u8 {
                        assert_eq!(
                            sa.get_block_state(x, y, z),
                            sb.get_block_state(x, y, z),
                            "block mismatch at {x},{y},{z}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn round_trip_with_light() {
        let chunk = sample_chunk();
        let bytes = serialize_chunk(DimensionId::Overworld, 3, -5, &chunk, true).unwrap();
        let loaded = deserialize_chunk(&bytes).unwrap();
        assert_eq!(loaded.dimension, DimensionId::Overworld);
        assert_eq!((loaded.x, loaded.z), (3, -5));
        assert_sections_eq(&chunk, &loaded.data);
        assert_eq!(
            chunk.heightmaps.motion_blocking,
            loaded.data.heightmaps.motion_blocking
        );
        assert_eq!(
            chunk.light.sky_light_arrays,
            loaded.data.light.sky_light_arrays
        );
    }

    #[test]
    fn round_trip_without_light_regenerates() {
        let chunk = sample_chunk();
        let bytes = serialize_chunk(DimensionId::Nether, 0, 0, &chunk, false).unwrap();
        let loaded = deserialize_chunk(&bytes).unwrap();
        assert_eq!(loaded.dimension, DimensionId::Nether);
        assert_sections_eq(&chunk, &loaded.data);
        // Light regenerated as full sky, not the original empty-ish data.
        assert!(!loaded.data.light.sky_light_arrays.is_empty());
    }

    #[test]
    fn block_entities_round_trip_with_anvil_shaped_entries() {
        use voidmc::{Banner, DyeColor, Sign, SignSide, Skull};
        use voidmc_data::v26_1_2::blocks;

        let mut chunk = sample_chunk();
        let sign_pos = BlockPosition {
            x: 50,
            y: 70,
            z: -65,
        };
        let head_pos = BlockPosition {
            x: 63,
            y: -60,
            z: -65,
        };
        let banner_pos = BlockPosition {
            x: 48,
            y: 0,
            z: -80,
        };
        let banner = Banner::new(DyeColor::Red);
        chunk.set_block(2, 70, 15, blocks::OAK_SIGN).unwrap();
        chunk.set_block(15, -60, 15, blocks::PLAYER_HEAD).unwrap();
        chunk.set_block(0, 0, 0, banner.block_state()).unwrap();
        let chunk_pos = ChunkPos::new(3, -5);
        chunk
            .set_block_entity(
                chunk_pos,
                sign_pos,
                Sign::lines(["persist", "me", "", ""]).back(SignSide::default().glowing()),
            )
            .unwrap();
        chunk
            .set_block_entity(chunk_pos, head_pos, Skull::player("Notch"))
            .unwrap();
        chunk
            .set_block_entity(chunk_pos, banner_pos, banner)
            .unwrap();

        let bytes = serialize_chunk(DimensionId::Overworld, 3, -5, &chunk, false).unwrap();
        let loaded = deserialize_chunk(&bytes).unwrap();
        let mut expected: Vec<_> = chunk.block_entities(chunk_pos).collect();
        let mut got: Vec<_> = loaded.data.block_entities(chunk_pos).collect();
        expected.sort_by_key(|(p, _)| (p.x, p.y, p.z));
        got.sort_by_key(|(p, _)| (p.x, p.y, p.z));
        assert_eq!(got, expected);
        assert!(!loaded.data.has_pending_block_entities());

        let mut cursor = bytes.as_slice();
        let nbt = Nbt::read(&mut cursor).unwrap();
        let Some(Tag::List(List::Compound(entries))) = field(&nbt.compound, "block_entities")
        else {
            panic!("block_entities list missing");
        };
        let sign = entries
            .iter()
            .find(|c| get_string(c, "id").unwrap() == "minecraft:sign")
            .unwrap();
        assert_eq!(get_int(sign, "x").unwrap(), 50);
        assert_eq!(get_int(sign, "y").unwrap(), 70);
        assert_eq!(get_int(sign, "z").unwrap(), -65);
        assert!(field(sign, "front_text").is_some());
    }

    #[test]
    fn raw_payload_keys_named_like_metadata_survive() {
        use ussr_nbt::owned::Tag;
        use voidmc_data::v26_1_2::blocks;

        let mut chunk = sample_chunk();
        chunk.set_block(1, 5, 1, blocks::CHEST).unwrap();
        let pos = BlockPosition { x: 1, y: 5, z: 1 };
        let raw = BlockEntity::raw(
            BlockEntityKind::chest(),
            Compound {
                tags: vec![
                    ("id".into(), Tag::String("custom".into())),
                    ("x".into(), Tag::Int(99)),
                ],
            },
        );
        chunk
            .set_block_entity(ChunkPos::new(0, 0), pos, raw.clone())
            .unwrap();
        let bytes = serialize_chunk(DimensionId::Overworld, 0, 0, &chunk, false).unwrap();
        let loaded = deserialize_chunk(&bytes).unwrap();
        assert_eq!(
            loaded
                .data
                .block_entities(ChunkPos::new(0, 0))
                .collect::<Vec<_>>(),
            vec![(pos, &raw)]
        );
    }

    #[test]
    fn persisted_entry_on_a_block_that_cannot_host_it_is_dropped() {
        let mut nbt = chunk_to_nbt(DimensionId::Overworld, 0, 0, &sample_chunk(), false);
        let sign_on_stone = Compound {
            tags: vec![
                ("id".into(), Tag::String("minecraft:sign".into())),
                ("x".into(), Tag::Int(0)),
                ("y".into(), Tag::Int(0)),
                ("z".into(), Tag::Int(0)),
            ],
        };
        nbt.compound
            .tags
            .retain(|(n, _)| n.decode().unwrap() != "block_entities");
        nbt.compound.tags.push((
            "block_entities".into(),
            Tag::List(List::Compound(vec![sign_on_stone])),
        ));
        let mut bytes = Vec::new();
        nbt.write(&mut bytes).unwrap();
        let loaded = deserialize_chunk(&bytes).unwrap();
        assert_eq!(loaded.data.block_entities(ChunkPos::new(0, 0)).count(), 0);
    }

    #[test]
    fn chunks_without_block_entities_still_load() {
        let chunk = sample_chunk();
        let bytes = serialize_chunk(DimensionId::Overworld, 0, 0, &chunk, false).unwrap();
        let loaded = deserialize_chunk(&bytes).unwrap();
        assert_eq!(loaded.data.block_entities(ChunkPos::new(0, 0)).count(), 0);
    }

    #[test]
    fn unknown_block_entity_type_is_corrupt() {
        let mut nbt = chunk_to_nbt(DimensionId::Overworld, 0, 0, &sample_chunk(), false);
        let bogus = Compound {
            tags: vec![
                ("id".into(), Tag::String("minecraft:not_real".into())),
                ("x".into(), Tag::Int(0)),
                ("y".into(), Tag::Int(0)),
                ("z".into(), Tag::Int(0)),
            ],
        };
        nbt.compound
            .tags
            .retain(|(n, _)| n.decode().unwrap() != "block_entities");
        nbt.compound.tags.push((
            "block_entities".into(),
            Tag::List(List::Compound(vec![bogus])),
        ));
        let mut bytes = Vec::new();
        nbt.write(&mut bytes).unwrap();
        assert!(deserialize_chunk(&bytes).is_err());
    }

    #[test]
    fn high_bit_longs_survive_round_trip() {
        // A data array entry with the top bit set must survive the u64<->i64 cast.
        let mut chunk = sample_chunk();
        if let PaletteData::Indirect { data, .. } = &mut chunk.sections[10].block_state {
            data[0] = 0x8000_0000_0000_0001u64;
        }
        let bytes = serialize_chunk(DimensionId::Overworld, 1, 1, &chunk, true).unwrap();
        let loaded = deserialize_chunk(&bytes).unwrap();
        if let PaletteData::Indirect { data, .. } = &loaded.data.sections[10].block_state {
            assert_eq!(data[0], 0x8000_0000_0000_0001u64);
        } else {
            panic!("expected indirect palette");
        }
    }
}
