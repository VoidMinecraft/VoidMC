use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

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
pub const DEFAULT_MAX_MEMORY: u64 = 256 * 1024 * 1024;

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
        Self::read_with_limits(reader, max_volume, DEFAULT_MAX_MEMORY)
    }

    /// `max_memory` bounds everything one load allocates together:
    /// decompression state, the decoded blocks, the palette and the names of
    /// unknown blocks. It also caps the file and its decompressed size. Fields
    /// the clipboard does not use are skipped without being stored, and a
    /// file with data after its root compound is rejected.
    pub fn read_with_limits(
        reader: impl Read,
        max_volume: u64,
        max_memory: u64,
    ) -> Result<Self, SchematicError> {
        let mut memory = MemoryBudget {
            used: 0,
            limit: max_memory,
        };
        let mut reader = Capped {
            inner: reader,
            remaining: max_memory,
        };
        let mut magic = [0u8; 2];
        let len = read_up_to(&mut reader, &mut magic)?;
        let gzip = magic[..len] == [0x1f, 0x8b];
        let reader = (&magic[..len]).chain(reader);
        if gzip {
            memory.charge(GZIP_WORKING_SET)?;
            parse(GzDecoder::new(reader), max_volume, max_memory, memory)
        } else {
            parse(reader, max_volume, max_memory, memory)
        }
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

    /// Writes to a temporary file next to `path` and renames it into place,
    /// so readers and concurrent saves never see a partial file.
    pub fn save(clipboard: &Clipboard, path: impl AsRef<Path>) -> Result<(), SchematicError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = path.as_ref();
        let mut temp = path.as_os_str().to_owned();
        temp.push(format!(
            ".{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let temp = std::path::PathBuf::from(temp);
        let written = std::fs::File::create(&temp)
            .map_err(SchematicError::from)
            .and_then(|file| {
                let mut writer = io::BufWriter::new(file);
                Self::write(clipboard, &mut writer)?;
                writer
                    .into_inner()
                    .map_err(|e| e.into_error())?
                    .sync_all()?;
                Ok(())
            })
            .and_then(|()| Ok(std::fs::rename(&temp, path)?));
        if written.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        written
    }
}

const INPUT_BUFFER: usize = 64 * 1024;
const GZIP_WORKING_SET: usize = 384 * 1024;
const MAX_NBT_DEPTH: u16 = 512;

fn parse<R: Read>(
    reader: R,
    max_volume: u64,
    max_memory: u64,
    mut memory: MemoryBudget,
) -> Result<Schematic, SchematicError> {
    memory.charge(INPUT_BUFFER)?;
    let mut parser = Parser {
        input: Input {
            reader,
            buffer: vec![0; INPUT_BUFFER],
            start: 0,
            end: 0,
            remaining: max_memory,
        },
        memory,
        max_volume,
        name: Vec::new(),
    };
    if parser.input.byte()? != 10 {
        return Err(SchematicError::Nbt("root is not a compound".to_string()));
    }
    let name_len = parser.input.u16()?;
    parser.input.skip(u64::from(name_len))?;
    let root = parser.fields(0, true)?;
    if !parser.input.at_end()? {
        return Err(SchematicError::Nbt(
            "data after the root compound".to_string(),
        ));
    }
    let fields = match root.inner {
        Field::Value(inner) => *inner,
        _ => root,
    };
    decode(fields, max_volume, &mut parser.memory)
}

fn decode(
    fields: Fields,
    max_volume: u64,
    memory: &mut MemoryBudget,
) -> Result<Schematic, SchematicError> {
    let version = fields.version.value().unwrap_or(1);
    let [width, height, length] = fields.size;
    let dimension =
        |field: Field<i32>, key: &'static str| field.value().ok_or(SchematicError::Missing(key));
    let size = BlockPos::new(
        dimension(width, "Width")?,
        dimension(height, "Height")?,
        dimension(length, "Length")?,
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

    let (palette, entries) = if version >= 3 {
        let Some(blocks) = fields.blocks.value() else {
            return Err(SchematicError::Missing("Blocks"));
        };
        (blocks.palette, blocks.data)
    } else {
        (fields.palette, fields.block_data)
    };
    let Some(palette) = palette.value() else {
        return Err(SchematicError::Missing("Palette"));
    };
    let Some(mut entries) = entries.value() else {
        return Err(SchematicError::Missing("Data"));
    };
    if let Some(error) = palette.error {
        return Err(SchematicError::Invalid(error));
    }

    let volume = volume as usize;
    for entry in entries.iter().take(volume) {
        if palette
            .by_entry
            .get(usize::from(*entry))
            .copied()
            .flatten()
            .is_none()
        {
            return Err(SchematicError::Invalid("block data palette index"));
        }
    }
    if entries.len() < volume {
        return Err(SchematicError::Invalid("block data"));
    }
    entries.truncate(volume);
    if entries.capacity() > volume {
        let before = entries.capacity() * size_of::<u16>();
        memory.charge(volume * size_of::<u16>())?;
        entries.shrink_to_fit();
        memory.release(before);
    }
    memory.charge(palette.by_entry.len() * size_of::<BlockState>())?;
    let states = palette
        .by_entry
        .iter()
        .map(|state| state.unwrap_or(BlockState::AIR))
        .collect();
    let offset = if version >= 3 {
        fields.offset.value().unwrap_or(BlockPos::ZERO)
    } else {
        match fields.metadata.value() {
            Some([x, y, z]) => BlockPos::new(
                x.value().unwrap_or(0),
                y.value().unwrap_or(0),
                z.value().unwrap_or(0),
            )
            .clamped(),
            None => BlockPos::ZERO,
        }
    };
    let mut unknown_blocks = palette.unknown;
    unknown_blocks.sort_unstable();
    unknown_blocks.dedup();
    Ok(Schematic {
        clipboard: Clipboard::from_parts(size, offset, states, entries),
        data_version: fields.data_version.value().unwrap_or(0),
        unknown_blocks,
    })
}

/// The first occurrence of a key wins; `Other` is a first occurrence of the
/// wrong type, which hides any later one.
#[derive(Default)]
enum Field<T> {
    #[default]
    Absent,
    Other,
    Value(T),
}

impl<T> Field<T> {
    fn is_absent(&self) -> bool {
        matches!(self, Field::Absent)
    }

    fn value(self) -> Option<T> {
        match self {
            Field::Value(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Default)]
struct Fields {
    version: Field<i32>,
    data_version: Field<i32>,
    size: [Field<i32>; 3],
    offset: Field<BlockPos>,
    metadata: Field<[Field<i32>; 3]>,
    blocks: Field<Blocks>,
    palette: Field<Palette>,
    block_data: Field<Vec<u16>>,
    inner: Field<Box<Fields>>,
}

#[derive(Default)]
struct Blocks {
    palette: Field<Palette>,
    data: Field<Vec<u16>>,
}

#[derive(Default)]
struct Palette {
    by_entry: Vec<Option<BlockState>>,
    unknown: Vec<String>,
    error: Option<&'static str>,
}

impl Fields {
    fn version(&self) -> Option<i32> {
        match &self.version {
            Field::Absent => None,
            Field::Other => Some(1),
            Field::Value(version) => Some(*version),
        }
    }

    fn volume(&self) -> Option<u64> {
        self.size.iter().try_fold(1u64, |volume, side| match side {
            Field::Value(side) => Some(volume * *side as u64),
            _ => None,
        })
    }
}

/// Live bytes a load holds, each allocation rounded up the way malloc does
/// and growth counted with the old and new buffers both alive.
struct MemoryBudget {
    used: u64,
    limit: u64,
}

fn allocation_cost(bytes: usize) -> u64 {
    if bytes == 0 {
        return 0;
    }
    let quantum = match bytes {
        0..=1024 => 16,
        1025..=131_072 => 512,
        _ => 16 * 1024,
    };
    (bytes as u64).saturating_add(16).next_multiple_of(quantum)
}

impl MemoryBudget {
    fn exceeded(&self) -> SchematicError {
        SchematicError::Nbt(format!(
            "the schematic needs more than {} bytes of memory",
            self.limit
        ))
    }

    fn charge(&mut self, bytes: usize) -> Result<(), SchematicError> {
        let used = self.used.saturating_add(allocation_cost(bytes));
        if used > self.limit {
            return Err(self.exceeded());
        }
        self.used = used;
        Ok(())
    }

    fn release(&mut self, bytes: usize) {
        self.used = self.used.saturating_sub(allocation_cost(bytes));
    }

    fn reserve<T>(
        &mut self,
        vec: &mut Vec<T>,
        len: usize,
        max: usize,
    ) -> Result<(), SchematicError> {
        let old = vec.capacity();
        if len <= old {
            return Ok(());
        }
        let capacity = len.max(old.saturating_mul(2)).max(4).min(max.max(len));
        self.charge(capacity.saturating_mul(size_of::<T>()))?;
        vec.try_reserve_exact(capacity - vec.len())
            .map_err(|_| self.exceeded())?;
        self.release(old * size_of::<T>());
        Ok(())
    }
}

struct Capped<R> {
    inner: R,
    remaining: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return match self.inner.read(&mut [0])? {
                0 => Ok(0),
                _ => Err(io::ErrorKind::FileTooLarge.into()),
            };
        }
        let len = buf
            .len()
            .min(self.remaining.min(usize::MAX as u64) as usize);
        let read = self.inner.read(&mut buf[..len])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

fn file_too_large() -> SchematicError {
    SchematicError::Nbt("the file is larger than the memory limit".to_string())
}

fn truncated() -> SchematicError {
    SchematicError::Nbt("truncated or oversized length".to_string())
}

struct Input<R> {
    reader: R,
    buffer: Vec<u8>,
    start: usize,
    end: usize,
    remaining: u64,
}

impl<R: Read> Input<R> {
    fn fill(&mut self) -> Result<&[u8], SchematicError> {
        if self.start == self.end {
            let read = loop {
                match self.reader.read(&mut self.buffer) {
                    Ok(read) => break read,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) if error.kind() == io::ErrorKind::FileTooLarge => {
                        return Err(file_too_large());
                    }
                    Err(error) => return Err(SchematicError::Io(error)),
                }
            };
            if read as u64 > self.remaining {
                return Err(file_too_large());
            }
            self.remaining -= read as u64;
            self.start = 0;
            self.end = read;
        }
        Ok(&self.buffer[self.start..self.end])
    }

    fn available(&mut self) -> Result<&[u8], SchematicError> {
        let chunk = self.fill()?;
        if chunk.is_empty() {
            return Err(truncated());
        }
        Ok(chunk)
    }

    fn at_end(&mut self) -> Result<bool, SchematicError> {
        Ok(self.fill()?.is_empty())
    }

    #[inline]
    fn byte(&mut self) -> Result<u8, SchematicError> {
        if let Some(&byte) = self.buffer[..self.end].get(self.start) {
            self.start += 1;
            return Ok(byte);
        }
        let byte = self.available()?[0];
        self.start += 1;
        Ok(byte)
    }

    #[inline]
    fn read_into(&mut self, out: &mut [u8]) -> Result<(), SchematicError> {
        if let Some(bytes) = self.buffer[..self.end].get(self.start..self.start + out.len()) {
            out.copy_from_slice(bytes);
            self.start += out.len();
            return Ok(());
        }
        let mut filled = 0;
        while filled < out.len() {
            let chunk = self.available()?;
            let len = chunk.len().min(out.len() - filled);
            out[filled..filled + len].copy_from_slice(&chunk[..len]);
            self.start += len;
            filled += len;
        }
        Ok(())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], SchematicError> {
        let mut out = [0; N];
        self.read_into(&mut out)?;
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16, SchematicError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn i32(&mut self) -> Result<i32, SchematicError> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    #[inline]
    fn skip(&mut self, mut len: u64) -> Result<(), SchematicError> {
        if len <= (self.end - self.start) as u64 {
            self.start += len as usize;
            return Ok(());
        }
        while len > 0 {
            let step = (self.available()?.len() as u64).min(len);
            self.start += step as usize;
            len -= step;
        }
        Ok(())
    }
}

fn fixed_size(id: u8) -> Option<u64> {
    Some(match id {
        1 => 1,
        2 => 2,
        3 | 5 => 4,
        4 | 6 => 8,
        _ => return None,
    })
}

enum Key {
    Version,
    DataVersion,
    Size(usize),
    Offset,
    Metadata,
    Blocks,
    Palette,
    BlockData,
    Schematic,
    Other,
}

fn key(name: &[u8]) -> Key {
    match name {
        b"Version" => Key::Version,
        b"DataVersion" => Key::DataVersion,
        b"Width" => Key::Size(0),
        b"Height" => Key::Size(1),
        b"Length" => Key::Size(2),
        b"Offset" => Key::Offset,
        b"Metadata" => Key::Metadata,
        b"Blocks" => Key::Blocks,
        b"Palette" => Key::Palette,
        b"BlockData" => Key::BlockData,
        b"Schematic" => Key::Schematic,
        _ => Key::Other,
    }
}

/// Reads NBT straight from the (decompressed) stream. Nothing is allocated
/// from a length the file claims: skipped values are consumed in place, and
/// the block data is decoded into one `u16` per block as it streams by.
struct Parser<R> {
    input: Input<R>,
    memory: MemoryBudget,
    max_volume: u64,
    name: Vec<u8>,
}

impl<R: Read> Parser<R> {
    fn descend(&self, depth: u16) -> Result<(), SchematicError> {
        if depth >= MAX_NBT_DEPTH {
            return Err(SchematicError::Nbt("nested too deeply".to_string()));
        }
        Ok(())
    }

    fn read_name(&mut self) -> Result<(), SchematicError> {
        let len = usize::from(self.input.u16()?);
        self.memory.reserve(&mut self.name, len, len)?;
        self.name.resize(len, 0);
        self.input.read_into(&mut self.name)
    }

    fn fields(&mut self, depth: u16, top: bool) -> Result<Fields, SchematicError> {
        self.descend(depth)?;
        let mut fields = Fields::default();
        loop {
            let id = self.input.byte()?;
            if id == 0 {
                return Ok(fields);
            }
            self.read_name()?;
            let child = depth + 1;
            let modern = fields.version().is_none_or(|version| version >= 3);
            let legacy = fields.version().is_none_or(|version| version < 3);
            match key(&self.name) {
                Key::Version if fields.version.is_absent() => {
                    fields.version = self.int(id, child)?;
                }
                Key::DataVersion if fields.data_version.is_absent() => {
                    fields.data_version = self.int(id, child)?;
                }
                Key::Size(axis) if fields.size[axis].is_absent() => {
                    fields.size[axis] = if id == 2 {
                        Field::Value(i32::from(i16::from_be_bytes(self.input.array()?) as u16))
                    } else {
                        self.other(id, child)?
                    };
                }
                Key::Offset if fields.offset.is_absent() => {
                    fields.offset = self.offset(id, child)?;
                }
                Key::Metadata if fields.metadata.is_absent() => {
                    fields.metadata = if id == 10 {
                        Field::Value(self.metadata(child)?)
                    } else {
                        self.other(id, child)?
                    };
                }
                Key::Blocks if fields.blocks.is_absent() => {
                    fields.blocks = if id == 10 && modern {
                        Field::Value(self.blocks(child, fields.volume())?)
                    } else {
                        self.other(id, child)?
                    };
                }
                Key::Palette if fields.palette.is_absent() => {
                    fields.palette = if id == 10 && legacy {
                        Field::Value(self.palette(child)?)
                    } else {
                        self.other(id, child)?
                    };
                }
                Key::BlockData if fields.block_data.is_absent() => {
                    fields.block_data = if id == 7 && legacy {
                        Field::Value(self.block_data(fields.volume())?)
                    } else {
                        self.other(id, child)?
                    };
                }
                Key::Schematic if top && fields.inner.is_absent() => {
                    fields.inner = if id == 10 {
                        Field::Value(Box::new(self.fields(child, false)?))
                    } else {
                        self.other(id, child)?
                    };
                }
                _ => self.skip(id, child)?,
            }
        }
    }

    fn other<T>(&mut self, id: u8, depth: u16) -> Result<Field<T>, SchematicError> {
        self.skip(id, depth)?;
        Ok(Field::Other)
    }

    fn int(&mut self, id: u8, depth: u16) -> Result<Field<i32>, SchematicError> {
        Ok(Field::Value(match id {
            1 => i32::from(self.input.byte()?),
            2 => i32::from(i16::from_be_bytes(self.input.array()?)),
            3 => self.input.i32()?,
            _ => return self.other(id, depth),
        }))
    }

    fn offset(&mut self, id: u8, depth: u16) -> Result<Field<BlockPos>, SchematicError> {
        if id != 11 {
            return self.other(id, depth);
        }
        let len = self.input.i32()?;
        if len != 3 {
            self.input.skip(len.max(0) as u64 * 4)?;
            return Ok(Field::Other);
        }
        let (x, y, z) = (self.input.i32()?, self.input.i32()?, self.input.i32()?);
        Ok(Field::Value(BlockPos::new(x, y, z).clamped()))
    }

    fn metadata(&mut self, depth: u16) -> Result<[Field<i32>; 3], SchematicError> {
        self.descend(depth)?;
        let mut offset: [Field<i32>; 3] = Default::default();
        loop {
            let id = self.input.byte()?;
            if id == 0 {
                return Ok(offset);
            }
            self.read_name()?;
            let axis = match self.name.as_slice() {
                b"WEOffsetX" => Some(0),
                b"WEOffsetY" => Some(1),
                b"WEOffsetZ" => Some(2),
                _ => None,
            };
            match axis {
                Some(axis) if offset[axis].is_absent() => {
                    offset[axis] = self.int(id, depth + 1)?;
                }
                _ => self.skip(id, depth + 1)?,
            }
        }
    }

    fn blocks(&mut self, depth: u16, volume: Option<u64>) -> Result<Blocks, SchematicError> {
        self.descend(depth)?;
        let mut blocks = Blocks::default();
        loop {
            let id = self.input.byte()?;
            if id == 0 {
                return Ok(blocks);
            }
            self.read_name()?;
            match self.name.as_slice() {
                b"Palette" if blocks.palette.is_absent() => {
                    blocks.palette = if id == 10 {
                        Field::Value(self.palette(depth + 1)?)
                    } else {
                        self.other(id, depth + 1)?
                    };
                }
                b"Data" if blocks.data.is_absent() => {
                    blocks.data = if id == 7 {
                        Field::Value(self.block_data(volume)?)
                    } else {
                        self.other(id, depth + 1)?
                    };
                }
                _ => self.skip(id, depth + 1)?,
            }
        }
    }

    fn palette(&mut self, depth: u16) -> Result<Palette, SchematicError> {
        self.descend(depth)?;
        let mut palette = Palette::default();
        loop {
            let id = self.input.byte()?;
            if id == 0 {
                return Ok(palette);
            }
            self.read_name()?;
            if palette.error.is_some() || id != 3 {
                palette.error.get_or_insert("palette entry");
                self.skip(id, depth + 1)?;
                continue;
            }
            let entry = self.input.i32()?;
            let Some(entry) = usize::try_from(entry)
                .ok()
                .filter(|entry| *entry < usize::from(u16::MAX))
            else {
                palette.error = Some("palette index");
                continue;
            };
            let scratch = 2 * self.name.len() + 64;
            self.memory.charge(scratch)?;
            let Ok(key) = simd_cesu8::mutf8::decode(&self.name) else {
                self.memory.release(scratch);
                palette.error = Some("palette name");
                continue;
            };
            let state = match BlockState::parse_lenient(&key) {
                Some(state) => state,
                None => {
                    self.memory.charge(key.len())?;
                    let len = palette.unknown.len() + 1;
                    self.memory.reserve(&mut palette.unknown, len, usize::MAX)?;
                    palette.unknown.push(key.to_string());
                    BlockState::AIR
                }
            };
            self.memory.release(scratch);
            self.memory
                .reserve(&mut palette.by_entry, entry + 1, usize::from(u16::MAX))?;
            if palette.by_entry.len() <= entry {
                palette.by_entry.resize(entry + 1, None);
            }
            palette.by_entry[entry] = Some(state);
        }
    }

    /// Decodes the varint palette indices into at most `volume` (or, before
    /// the dimensions are known, `max_volume`) entries; an index too large
    /// for a palette becomes `u16::MAX`, which no palette entry can be.
    fn block_data(&mut self, volume: Option<u64>) -> Result<Vec<u16>, SchematicError> {
        let len = self.input.i32()?.max(0) as u64;
        let limit = match volume {
            Some(volume) if volume > self.max_volume => 0,
            Some(volume) => volume,
            None => self.max_volume,
        }
        .min(len) as usize;
        let mut entries: Vec<u16> = Vec::new();
        if volume.is_some() {
            self.memory.reserve(&mut entries, limit, limit)?;
        }
        let (mut value, mut shift, mut broken) = (0u32, 0u32, false);
        let mut left = len;
        while left > 0 {
            let chunk = self.input.available()?;
            let step = chunk.len().min(left.min(usize::MAX as u64) as usize);
            for &byte in &chunk[..step] {
                if broken || entries.len() >= limit {
                    break;
                }
                value |= u32::from(byte & 0x7f) << shift;
                if byte & 0x80 == 0 {
                    let len = entries.len() + 1;
                    self.memory.reserve(&mut entries, len, limit)?;
                    entries.push(u16::try_from(value).unwrap_or(u16::MAX));
                    value = 0;
                    shift = 0;
                } else {
                    shift += 7;
                    broken = shift >= 35;
                }
            }
            self.input.start += step;
            left -= step as u64;
        }
        Ok(entries)
    }

    fn skip(&mut self, id: u8, depth: u16) -> Result<(), SchematicError> {
        match id {
            1..=6 => self.input.skip(fixed_size(id).unwrap_or(0)),
            7 => {
                let len = self.input.i32()?;
                self.input.skip(len.max(0) as u64)
            }
            8 => {
                let len = self.input.u16()?;
                self.input.skip(u64::from(len))
            }
            9 => {
                self.descend(depth)?;
                let element = self.input.byte()?;
                let len = self.input.i32()?;
                if len <= 0 {
                    return Ok(());
                }
                match element {
                    1..=6 => self
                        .input
                        .skip(len as u64 * fixed_size(element).unwrap_or(0)),
                    7..=12 => {
                        for _ in 0..len {
                            self.skip(element, depth + 1)?;
                        }
                        Ok(())
                    }
                    _ => Err(SchematicError::Nbt(format!("invalid tag {element}"))),
                }
            }
            10 => {
                self.descend(depth)?;
                loop {
                    let id = self.input.byte()?;
                    if id == 0 {
                        return Ok(());
                    }
                    let len = self.input.u16()?;
                    self.input.skip(u64::from(len))?;
                    self.skip(id, depth + 1)?;
                }
            }
            11 => {
                let len = self.input.i32()?;
                self.input.skip(len.max(0) as u64 * 4)
            }
            12 => {
                let len = self.input.i32()?;
                self.input.skip(len.max(0) as u64 * 8)
            }
            _ => Err(SchematicError::Nbt(format!("invalid tag {id}"))),
        }
    }
}

fn name(text: &str) -> MString {
    MString::from_string(text.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extent::MemoryExtent;
    use crate::region::Cuboid;

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
    fn rejects_lengths_longer_than_the_file() {
        let list_bomb = [
            0x0a, 0x00, 0x00, 0x09, 0x00, 0x00, 0x0a, 0x7f, 0xff, 0xff, 0xff, 0x00, 0x00,
        ];
        assert_eq!(list_bomb.len(), 13);
        assert!(matches!(
            Schematic::read(list_bomb.as_slice()),
            Err(SchematicError::Nbt(_))
        ));
        let array_bomb = [
            0x0a, 0x00, 0x00, 0x0b, 0x00, 0x00, 0x7f, 0xff, 0xff, 0xff, 0x00, 0x00,
        ];
        assert!(matches!(
            Schematic::read(array_bomb.as_slice()),
            Err(SchematicError::Nbt(_))
        ));
        let mut nested = vec![0x0a, 0x00, 0x00, 0x09, 0x00, 0x00];
        for _ in 0..100_000 {
            nested.extend([0x09, 0x00, 0x00, 0x00, 0x01]);
        }
        nested.extend([0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        assert!(matches!(
            Schematic::read(nested.as_slice()),
            Err(SchematicError::Nbt(_))
        ));
    }

    #[test]
    fn saves_replace_the_file_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hut.schem");
        let first = sample();
        let second = first.rotated(1);
        let saves: Vec<_> = [first.clone(), second.clone(), first, second.clone()]
            .into_iter()
            .map(|clipboard| {
                let path = path.clone();
                std::thread::spawn(move || Schematic::save(&clipboard, path))
            })
            .collect();
        for save in saves {
            save.join().unwrap().unwrap();
        }
        let loaded = Schematic::load(&path).unwrap().clipboard;
        assert!(loaded == second || loaded == sample());
        Schematic::save(&second, &path).unwrap();
        assert_eq!(Schematic::load(&path).unwrap().clipboard, second);
        let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn rejects_schematics_that_would_outgrow_the_memory_limit() {
        let clipboard = Clipboard::new(BlockPos::new(128, 64, 128), BlockPos::ZERO);
        let mut gzip = Vec::new();
        Schematic::write(&clipboard, &mut gzip).unwrap();
        assert!(Schematic::read_with_limits(gzip.as_slice(), DEFAULT_MAX_VOLUME, 4 << 20).is_ok());
        let error =
            Schematic::read_with_limits(gzip.as_slice(), DEFAULT_MAX_VOLUME, 3 << 19).unwrap_err();
        assert!(error.to_string().contains("needs more than"), "{error}");

        let mut raw = vec![0x0a, 0x00, 0x00, 0x07, 0x00, 0x00];
        raw.extend((2u32 << 20).to_be_bytes());
        raw.resize(raw.len() + (2 << 20), 0);
        raw.push(0x00);
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&raw).unwrap();
        let gzip = gzip.finish().unwrap();
        let inflated =
            Schematic::read_with_limits(gzip.as_slice(), DEFAULT_MAX_VOLUME, 1 << 20).unwrap_err();
        assert!(inflated.to_string().contains("memory limit"), "{inflated}");
        let file =
            Schematic::read_with_limits(raw.as_slice(), DEFAULT_MAX_VOLUME, 1 << 20).unwrap_err();
        assert!(file.to_string().contains("memory limit"), "{file}");
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
