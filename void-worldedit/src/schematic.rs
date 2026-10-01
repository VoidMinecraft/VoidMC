use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use ussr_nbt::endian::RawVec;
use ussr_nbt::mutf8::MString;
use ussr_nbt::owned::{Compound, Nbt, Tag};

use crate::block::{BlockState, VERSION};
use crate::clipboard::Clipboard;
use crate::math::BlockPos;

pub const DEFAULT_MAX_VOLUME: u64 = 64 * 1024 * 1024;
const MAX_INFLATED_BYTES: u64 = 1024 * 1024 * 1024;

/// A Sponge schematic (`.schem`) loaded into a clipboard. Versions 1–3 are
/// read, version 3 is written; block entities, entities and biomes are not
/// carried over.
#[derive(Debug, Clone)]
pub struct Schematic {
    pub clipboard: Clipboard,
    pub data_version: i32,
    /// Palette entries this server does not know, pasted as air.
    pub unknown_blocks: Vec<String>,
}

#[derive(Debug)]
pub enum SchematicError {
    Io(io::Error),
    Nbt(String),
    Missing(&'static str),
    Invalid(&'static str),
    TooLarge { volume: u64, limit: u64 },
}

impl fmt::Display for SchematicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchematicError::Io(error) => write!(f, "I/O error: {error}"),
            SchematicError::Nbt(error) => write!(f, "invalid NBT: {error}"),
            SchematicError::Missing(field) => write!(f, "missing field '{field}'"),
            SchematicError::Invalid(what) => write!(f, "invalid {what}"),
            SchematicError::TooLarge { volume, limit } => {
                write!(f, "schematic has {volume} blocks, the limit is {limit}")
            }
        }
    }
}

impl std::error::Error for SchematicError {}

impl From<io::Error> for SchematicError {
    fn from(error: io::Error) -> Self {
        SchematicError::Io(error)
    }
}

impl Schematic {
    pub fn read(reader: impl Read) -> Result<Self, SchematicError> {
        Self::read_with_limit(reader, DEFAULT_MAX_VOLUME)
    }

    pub fn read_with_limit(reader: impl Read, max_volume: u64) -> Result<Self, SchematicError> {
        let mut bytes = Vec::new();
        BufReader::new(reader).read_to_end(&mut bytes)?;
        if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut inflated = Vec::new();
            GzDecoder::new(bytes.as_slice())
                .take(MAX_INFLATED_BYTES)
                .read_to_end(&mut inflated)?;
            bytes = inflated;
        }
        let nbt =
            Nbt::read(&mut bytes.as_slice()).map_err(|e| SchematicError::Nbt(format!("{e:?}")))?;
        let root = match get(&nbt.compound, "Schematic") {
            Some(Tag::Compound(inner)) => inner,
            _ => &nbt.compound,
        };
        decode(root, max_volume)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, SchematicError> {
        Self::read(std::fs::File::open(path)?)
    }

    /// Writes `clipboard` as a gzip-compressed Sponge v3 schematic; keep
    /// entries become air.
    pub fn write(clipboard: &Clipboard, writer: impl Write) -> Result<(), SchematicError> {
        let size = clipboard.size();
        if [size.x, size.y, size.z]
            .iter()
            .any(|d| *d > i32::from(u16::MAX))
        {
            return Err(SchematicError::Invalid(
                "dimensions (each side must fit in 65535)",
            ));
        }
        let mut palette: Vec<BlockState> = Vec::new();
        let mut unique = |state: BlockState| match palette.iter().position(|s| *s == state) {
            Some(entry) => entry,
            None => {
                palette.push(state);
                palette.len() - 1
            }
        };
        let remap: Vec<usize> = clipboard.palette().iter().map(|s| unique(*s)).collect();
        let air_entry = unique(BlockState::AIR);
        let mut data = Vec::with_capacity(clipboard.volume());
        for index in 0..clipboard.volume() {
            let entry = clipboard
                .raw_entry(index)
                .map_or(air_entry, |entry| remap[usize::from(entry)]);
            write_varint(&mut data, entry as u32);
        }
        let palette_tag = Compound {
            tags: palette
                .iter()
                .enumerate()
                .map(|(entry, state)| (name(&state.to_string()), Tag::Int(entry as i32)))
                .collect(),
        };
        let offset = clipboard.offset();
        let date = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        let schematic = Compound {
            tags: vec![
                (name("Version"), Tag::Int(3)),
                (
                    name("DataVersion"),
                    Tag::Int(voidmc_data::world_version(VERSION)),
                ),
                (
                    name("Metadata"),
                    Tag::Compound(Compound {
                        tags: vec![(name("Date"), Tag::Long(date))],
                    }),
                ),
                (name("Width"), Tag::Short(size.x as u16 as i16)),
                (name("Height"), Tag::Short(size.y as u16 as i16)),
                (name("Length"), Tag::Short(size.z as u16 as i16)),
                (
                    name("Offset"),
                    Tag::IntArray(RawVec::from(vec![offset.x, offset.y, offset.z])),
                ),
                (
                    name("Blocks"),
                    Tag::Compound(Compound {
                        tags: vec![
                            (name("Palette"), Tag::Compound(palette_tag)),
                            (name("Data"), Tag::ByteArray(data)),
                        ],
                    }),
                ),
            ],
        };
        let root = Nbt {
            name: MString::new(),
            compound: Compound {
                tags: vec![(name("Schematic"), Tag::Compound(schematic))],
            },
        };
        let mut encoder = GzEncoder::new(writer, Compression::default());
        root.write(&mut encoder)?;
        encoder.finish()?;
        Ok(())
    }

    pub fn save(clipboard: &Clipboard, path: impl AsRef<Path>) -> Result<(), SchematicError> {
        let file = std::fs::File::create(path)?;
        Self::write(clipboard, io::BufWriter::new(file))
    }
}

fn decode(root: &Compound, max_volume: u64) -> Result<Schematic, SchematicError> {
    let version = int(root, "Version").unwrap_or(1);
    let dimension = |key: &'static str| match get(root, key) {
        Some(Tag::Short(value)) => Ok(i32::from(*value as u16)),
        _ => Err(SchematicError::Missing(key)),
    };
    let size = BlockPos::new(
        dimension("Width")?,
        dimension("Height")?,
        dimension("Length")?,
    );
    let volume = size.x as u64 * size.y as u64 * size.z as u64;
    if volume > max_volume {
        return Err(SchematicError::TooLarge {
            volume,
            limit: max_volume,
        });
    }
    if volume == 0 {
        return Err(SchematicError::Invalid("empty dimensions"));
    }

    let (palette_tag, data) = if version >= 3 {
        let Some(Tag::Compound(blocks)) = get(root, "Blocks") else {
            return Err(SchematicError::Missing("Blocks"));
        };
        (get(blocks, "Palette"), get(blocks, "Data"))
    } else {
        (get(root, "Palette"), get(root, "BlockData"))
    };
    let Some(Tag::Compound(palette_tag)) = palette_tag else {
        return Err(SchematicError::Missing("Palette"));
    };
    let Some(Tag::ByteArray(data)) = data else {
        return Err(SchematicError::Missing("Data"));
    };

    let mut unknown_blocks = Vec::new();
    let mut by_entry: Vec<Option<BlockState>> = Vec::new();
    for (key, value) in &palette_tag.tags {
        let Tag::Int(entry) = value else {
            return Err(SchematicError::Invalid("palette entry"));
        };
        let entry =
            usize::try_from(*entry).map_err(|_| SchematicError::Invalid("palette index"))?;
        if entry >= usize::from(u16::MAX) {
            return Err(SchematicError::Invalid("palette index"));
        }
        let key = key
            .decode()
            .map_err(|_| SchematicError::Invalid("palette name"))?;
        let state = BlockState::parse_lenient(&key).unwrap_or_else(|| {
            unknown_blocks.push(key.to_string());
            BlockState::AIR
        });
        if by_entry.len() <= entry {
            by_entry.resize(entry + 1, None);
        }
        by_entry[entry] = Some(state);
    }

    let mut entries = Vec::with_capacity(volume as usize);
    let mut cursor = data.as_slice();
    for _ in 0..volume {
        let entry = read_varint(&mut cursor).ok_or(SchematicError::Invalid("block data"))? as usize;
        if by_entry.get(entry).copied().flatten().is_none() {
            return Err(SchematicError::Invalid("block data palette index"));
        }
        entries.push(entry as u16);
    }
    let palette = by_entry
        .into_iter()
        .map(|state| state.unwrap_or(BlockState::AIR))
        .collect();
    let clipboard = Clipboard::from_parts(size, schematic_offset(root, version), palette, entries);
    unknown_blocks.sort();
    unknown_blocks.dedup();
    Ok(Schematic {
        clipboard,
        data_version: int(root, "DataVersion").unwrap_or(0),
        unknown_blocks,
    })
}

/// The offset from the paste origin to the minimum corner. v3 stores it in
/// `Offset`; v1/v2 store the world minimum there and WorldEdit adds the
/// relative offset as `Metadata.WEOffset{X,Y,Z}`.
fn schematic_offset(root: &Compound, version: i32) -> BlockPos {
    if version >= 3 {
        return match get(root, "Offset") {
            Some(Tag::IntArray(values)) => {
                let values = values.to_vec();
                match values.as_slice() {
                    [x, y, z] => BlockPos::new(*x, *y, *z).clamped(),
                    _ => BlockPos::ZERO,
                }
            }
            _ => BlockPos::ZERO,
        };
    }
    match get(root, "Metadata") {
        Some(Tag::Compound(metadata)) => BlockPos::new(
            int(metadata, "WEOffsetX").unwrap_or(0),
            int(metadata, "WEOffsetY").unwrap_or(0),
            int(metadata, "WEOffsetZ").unwrap_or(0),
        )
        .clamped(),
        _ => BlockPos::ZERO,
    }
}

fn name(text: &str) -> MString {
    MString::from_string(text.to_string())
}

fn get<'a>(compound: &'a Compound, key: &str) -> Option<&'a Tag> {
    compound
        .tags
        .iter()
        .find(|(name, _)| name.decode().is_ok_and(|n| n == key))
        .map(|(_, tag)| tag)
}

fn int(compound: &Compound, key: &str) -> Option<i32> {
    match get(compound, key)? {
        Tag::Int(value) => Some(*value),
        Tag::Short(value) => Some(i32::from(*value)),
        Tag::Byte(value) => Some(i32::from(*value)),
        _ => None,
    }
}

fn write_varint(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_varint(input: &mut &[u8]) -> Option<u32> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let (&byte, rest) = input.split_first()?;
        *input = rest;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extent::MemoryExtent;
    use crate::region::Cuboid;

    fn state(input: &str) -> BlockState {
        BlockState::parse(input).unwrap()
    }

    fn sample() -> Clipboard {
        let mut extent = MemoryExtent::default();
        let region = Cuboid::new(BlockPos::new(0, 0, 0), BlockPos::new(20, 3, 9));
        for pos in region.positions() {
            let block = match (pos.x + pos.y + pos.z) % 4 {
                0 => state("stone"),
                1 => state("oak_stairs[facing=east,half=top]"),
                2 => state("glass"),
                _ => BlockState::AIR,
            };
            extent.set(pos, block);
        }
        Clipboard::copy(&extent, &region, BlockPos::new(5, -2, 1))
    }

    #[test]
    fn v3_round_trip_preserves_blocks_and_offset() {
        let clipboard = sample();
        let mut bytes = Vec::new();
        Schematic::write(&clipboard, &mut bytes).unwrap();
        assert_eq!(&bytes[..2], &[0x1f, 0x8b]);
        let loaded = Schematic::read(bytes.as_slice()).unwrap();
        assert!(loaded.unknown_blocks.is_empty());
        assert_eq!(loaded.data_version, voidmc_data::world_version(VERSION));
        assert_eq!(loaded.clipboard.size(), clipboard.size());
        assert_eq!(loaded.clipboard.offset(), BlockPos::new(-5, 2, -1));
        let original: Vec<_> = clipboard.blocks().collect();
        let reloaded: Vec<_> = loaded.clipboard.blocks().collect();
        assert_eq!(original, reloaded);
    }

    #[test]
    fn v3_layout_matches_the_sponge_specification() {
        let mut clipboard = Clipboard::new(BlockPos::new(2, 1, 1), BlockPos::new(0, 0, 0));
        clipboard.set(BlockPos::new(0, 0, 0), state("stone"));
        let mut bytes = Vec::new();
        Schematic::write(&clipboard, &mut bytes).unwrap();
        let mut raw = Vec::new();
        GzDecoder::new(bytes.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        let nbt = Nbt::read(&mut raw.as_slice()).unwrap();
        let Some(Tag::Compound(schematic)) = get(&nbt.compound, "Schematic") else {
            panic!("v3 root must hold a Schematic compound");
        };
        assert_eq!(int(schematic, "Version"), Some(3));
        assert!(matches!(get(schematic, "Width"), Some(Tag::Short(2))));
        let Some(Tag::Compound(blocks)) = get(schematic, "Blocks") else {
            panic!("missing Blocks");
        };
        let Some(Tag::Compound(palette)) = get(blocks, "Palette") else {
            panic!("missing Palette");
        };
        assert_eq!(int(palette, "minecraft:stone"), Some(0));
        assert_eq!(int(palette, "minecraft:air"), Some(1));
        assert!(matches!(get(blocks, "Data"), Some(Tag::ByteArray(data)) if data == &vec![0, 1]));
    }

    fn v2(
        palette: Vec<(&str, i32)>,
        data: Vec<u8>,
        size: (i16, i16, i16),
        offset: Option<(i32, i32, i32)>,
    ) -> Vec<u8> {
        let mut tags = vec![
            (name("Version"), Tag::Int(2)),
            (name("DataVersion"), Tag::Int(3465)),
            (name("Width"), Tag::Short(size.0)),
            (name("Height"), Tag::Short(size.1)),
            (name("Length"), Tag::Short(size.2)),
            (
                name("Offset"),
                Tag::IntArray(RawVec::from(vec![100, 64, 100])),
            ),
            (name("PaletteMax"), Tag::Int(palette.len() as i32)),
            (
                name("Palette"),
                Tag::Compound(Compound {
                    tags: palette
                        .into_iter()
                        .map(|(key, entry)| (name(key), Tag::Int(entry)))
                        .collect(),
                }),
            ),
            (name("BlockData"), Tag::ByteArray(data)),
        ];
        if let Some((x, y, z)) = offset {
            tags.push((
                name("Metadata"),
                Tag::Compound(Compound {
                    tags: vec![
                        (name("WEOffsetX"), Tag::Int(x)),
                        (name("WEOffsetY"), Tag::Int(y)),
                        (name("WEOffsetZ"), Tag::Int(z)),
                    ],
                }),
            ));
        }
        let mut bytes = Vec::new();
        Nbt {
            name: name("Schematic"),
            compound: Compound { tags },
        }
        .write(&mut bytes)
        .unwrap();
        bytes
    }

    #[test]
    fn reads_worldedit_v2_files_with_unknown_blocks() {
        let bytes = v2(
            vec![
                ("minecraft:air", 0),
                ("minecraft:oak_log[axis=z]", 1),
                ("minecraft:grass_path", 2),
            ],
            vec![1, 0, 2, 1],
            (2, 1, 2),
            Some((-1, 0, -3)),
        );
        let loaded = Schematic::read(bytes.as_slice()).unwrap();
        assert_eq!(loaded.clipboard.offset(), BlockPos::new(-1, 0, -3));
        assert_eq!(
            loaded.unknown_blocks,
            vec!["minecraft:grass_path".to_string()]
        );
        assert_eq!(
            loaded.clipboard.get(BlockPos::new(0, 0, 0)),
            Some(state("oak_log[axis=z]"))
        );
        assert_eq!(
            loaded.clipboard.get(BlockPos::new(1, 0, 0)),
            Some(BlockState::AIR)
        );
        assert_eq!(
            loaded.clipboard.get(BlockPos::new(0, 0, 1)),
            Some(BlockState::AIR)
        );
        assert_eq!(
            loaded.clipboard.get(BlockPos::new(1, 0, 1)),
            Some(state("oak_log[axis=z]"))
        );
    }

    #[test]
    fn rewriting_a_file_with_unknown_blocks_keeps_palette_keys_unique() {
        let bytes = v2(
            vec![("minecraft:air", 0), ("minecraft:grass_path", 1)],
            vec![0, 1],
            (2, 1, 1),
            None,
        );
        let loaded = Schematic::read(bytes.as_slice()).unwrap();
        let mut written = Vec::new();
        Schematic::write(&loaded.clipboard, &mut written).unwrap();
        let mut raw = Vec::new();
        GzDecoder::new(written.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        let nbt = Nbt::read(&mut raw.as_slice()).unwrap();
        let Some(Tag::Compound(schematic)) = get(&nbt.compound, "Schematic") else {
            panic!("missing Schematic");
        };
        let Some(Tag::Compound(blocks)) = get(schematic, "Blocks") else {
            panic!("missing Blocks");
        };
        let Some(Tag::Compound(palette)) = get(blocks, "Palette") else {
            panic!("missing Palette");
        };
        assert_eq!(palette.tags.len(), 1);
        assert!(matches!(get(blocks, "Data"), Some(Tag::ByteArray(data)) if data == &vec![0, 0]));
    }

    #[test]
    fn rejects_truncated_data_and_oversized_volumes() {
        let truncated = v2(vec![("minecraft:stone", 0)], vec![0, 0], (2, 1, 2), None);
        assert!(matches!(
            Schematic::read(truncated.as_slice()),
            Err(SchematicError::Invalid(_))
        ));
        let bad_index = v2(vec![("minecraft:stone", 0)], vec![0, 5], (2, 1, 1), None);
        assert!(Schematic::read(bad_index.as_slice()).is_err());
        let huge = v2(vec![("minecraft:stone", 0)], vec![], (-1, -1, -1), None);
        assert!(matches!(
            Schematic::read_with_limit(huge.as_slice(), 1_000_000),
            Err(SchematicError::TooLarge { .. })
        ));
    }

    #[test]
    fn varints_round_trip() {
        for value in [0, 1, 127, 128, 300, 29_872, u32::MAX] {
            let mut out = Vec::new();
            write_varint(&mut out, value);
            assert_eq!(read_varint(&mut out.as_slice()), Some(value));
        }
    }
}
