//! A single sectorized region file (`.vrm`), modeled on Minecraft's `.mca`.
//!
//! Layout (4096-byte sectors):
//! - sector 0: 1024 location entries (`u32 = offset << 8 | sector_count`)
//! - sector 1: 1024 timestamps (`u32`)
//! - sectors 2..: chunk payloads, each `[u32 len][u8 compression][zlib data]`,
//!   padded to a sector boundary. `len` counts the compression byte + data.

use std::fs::File;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::error::{PersistenceError, Result};

const SECTOR: usize = 4096;
const HEADER_SECTORS: usize = 2;
const ENTRIES: usize = 1024;
const COMPRESSION_ZLIB: u8 = 2;

pub struct RegionFile {
    file: File,
    locations: Vec<u32>,
    timestamps: Vec<u32>,
    used: Vec<bool>,
}

impl RegionFile {
    /// Opens (creating if needed) the region file at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        let mut locations = vec![0u32; ENTRIES];
        let mut timestamps = vec![0u32; ENTRIES];

        let len = file.metadata()?.len() as usize;
        if len < SECTOR * HEADER_SECTORS {
            file.set_len((SECTOR * HEADER_SECTORS) as u64)?;
        } else {
            let mut buf = vec![0u8; SECTOR];
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut buf)?;
            for (i, slot) in locations.iter_mut().enumerate() {
                *slot = u32::from_be_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
            }
            file.read_exact(&mut buf)?;
            for (i, slot) in timestamps.iter_mut().enumerate() {
                *slot = u32::from_be_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
            }
        }

        let total_sectors =
            ((file.metadata()?.len() as usize).div_ceil(SECTOR)).max(HEADER_SECTORS);
        let mut used = vec![false; total_sectors];
        for u in used.iter_mut().take(HEADER_SECTORS) {
            *u = true;
        }
        for &loc in &locations {
            if loc != 0 {
                let off = (loc >> 8) as usize;
                let cnt = (loc & 0xFF) as usize;
                if off + cnt > used.len() {
                    used.resize(off + cnt, false);
                }
                for u in used.iter_mut().skip(off).take(cnt) {
                    *u = true;
                }
            }
        }

        Ok(Self {
            file,
            locations,
            timestamps,
            used,
        })
    }

    fn index(cx: i32, cz: i32) -> usize {
        ((cx & 31) + (cz & 31) * 32) as usize
    }

    /// Reads and decompresses the chunk payload at `(cx, cz)`, if present.
    pub fn read_chunk(&mut self, cx: i32, cz: i32) -> Result<Option<Vec<u8>>> {
        let loc = self.locations[Self::index(cx, cz)];
        if loc == 0 {
            return Ok(None);
        }
        let off = (loc >> 8) as u64;
        self.file.seek(SeekFrom::Start(off * SECTOR as u64))?;

        let mut len_buf = [0u8; 4];
        self.file.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len == 0 {
            return Ok(None);
        }

        let mut comp = [0u8; 1];
        self.file.read_exact(&mut comp)?;
        let mut payload = vec![0u8; len - 1];
        self.file.read_exact(&mut payload)?;

        match comp[0] {
            COMPRESSION_ZLIB => {
                let mut decoder = ZlibDecoder::new(&payload[..]);
                let mut out = Vec::new();
                decoder.read_to_end(&mut out)?;
                Ok(Some(out))
            }
            other => Err(PersistenceError::Corrupt(format!(
                "unknown compression scheme {other}"
            ))),
        }
    }

    /// Compresses and writes the chunk payload at `(cx, cz)`.
    pub fn write_chunk(
        &mut self,
        cx: i32,
        cz: i32,
        uncompressed: &[u8],
        timestamp: u32,
    ) -> Result<()> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(uncompressed)?;
        let compressed = encoder.finish()?;

        let total = 4 + 1 + compressed.len();
        let sectors_needed = total.div_ceil(SECTOR);
        if sectors_needed > 255 {
            return Err(PersistenceError::Corrupt(format!(
                "chunk too large: {sectors_needed} sectors"
            )));
        }

        let idx = Self::index(cx, cz);
        let loc = self.locations[idx];

        let off = if loc != 0 && (loc & 0xFF) as usize >= sectors_needed {
            (loc >> 8) as usize
        } else {
            if loc != 0 {
                let old_off = (loc >> 8) as usize;
                let old_cnt = (loc & 0xFF) as usize;
                for u in self.used.iter_mut().skip(old_off).take(old_cnt) {
                    *u = false;
                }
            }
            self.allocate(sectors_needed)
        };

        if off + sectors_needed > self.used.len() {
            self.used.resize(off + sectors_needed, false);
        }
        for u in self.used.iter_mut().skip(off).take(sectors_needed) {
            *u = true;
        }

        let mut block = Vec::with_capacity(sectors_needed * SECTOR);
        block.extend_from_slice(&((compressed.len() + 1) as u32).to_be_bytes());
        block.push(COMPRESSION_ZLIB);
        block.extend_from_slice(&compressed);
        block.resize(sectors_needed * SECTOR, 0);

        self.file.seek(SeekFrom::Start((off * SECTOR) as u64))?;
        self.file.write_all(&block)?;

        self.locations[idx] = ((off as u32) << 8) | (sectors_needed as u32);
        self.timestamps[idx] = timestamp;
        self.write_header_entry(idx)?;
        Ok(())
    }

    /// Finds a contiguous free run of `count` sectors, or returns the sector
    /// just past the current end (the file grows on write).
    fn allocate(&mut self, count: usize) -> usize {
        let mut run_start: Option<usize> = None;
        let mut run_len = 0usize;
        for s in HEADER_SECTORS..self.used.len() {
            if self.used[s] {
                run_start = None;
                run_len = 0;
            } else {
                if run_start.is_none() {
                    run_start = Some(s);
                    run_len = 0;
                }
                run_len += 1;
                if run_len == count {
                    return run_start.unwrap();
                }
            }
        }
        self.used.len().max(HEADER_SECTORS)
    }

    fn write_header_entry(&mut self, idx: usize) -> Result<()> {
        self.file.seek(SeekFrom::Start((idx * 4) as u64))?;
        self.file.write_all(&self.locations[idx].to_be_bytes())?;
        self.file.seek(SeekFrom::Start((SECTOR + idx * 4) as u64))?;
        self.file.write_all(&self.timestamps[idx].to_be_bytes())?;
        Ok(())
    }

    /// Flushes buffered writes to durable storage.
    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        self.file.sync_all()?;
        Ok(())
    }
}
