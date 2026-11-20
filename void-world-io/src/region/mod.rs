//! Default [`ChunkStore`] backed by sectorized region files on disk.

mod file;

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use voidmc::{ChunkPos, DimensionId};

use crate::error::Result;
use crate::store::ChunkStore;
use file::RegionFile;

/// Stores chunks in `<root>/region/<dim>/r.<rx>.<rz>.vrm`, 32×32 chunks per file.
/// A chunk that cannot be decoded is quarantined next to its region file, in
/// `c.<cx>.<cz>.<n>.corrupt` (see [`ChunkStore::quarantine`]).
pub struct RegionChunkStore {
    root: PathBuf,
    open: Mutex<HashMap<(DimensionId, i32, i32), RegionFile>>,
}

impl RegionChunkStore {
    /// Creates a store rooted at `root` (the world directory).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            open: Mutex::new(HashMap::new()),
        }
    }

    fn dim_dir(dimension: DimensionId) -> &'static str {
        dimension
            .name()
            .strip_prefix("minecraft:")
            .unwrap_or(dimension.name())
    }

    fn region_dir(&self, dimension: DimensionId) -> PathBuf {
        self.root.join("region").join(Self::dim_dir(dimension))
    }

    fn region_path(&self, dimension: DimensionId, rx: i32, rz: i32) -> PathBuf {
        self.region_dir(dimension).join(format!("r.{rx}.{rz}.vrm"))
    }
}

fn region_coords(pos: ChunkPos) -> (i32, i32) {
    (pos.x >> 5, pos.z >> 5)
}

fn now_secs() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

impl ChunkStore for RegionChunkStore {
    fn read(&self, dimension: DimensionId, pos: ChunkPos) -> Result<Option<Vec<u8>>> {
        let (rx, rz) = region_coords(pos);
        let path = self.region_path(dimension, rx, rz);
        let mut map = self.open.lock().expect("region store mutex poisoned");

        let key = (dimension, rx, rz);
        if !map.contains_key(&key) {
            // Don't create region files just to answer a read miss.
            if !path.exists() {
                return Ok(None);
            }
            map.insert(key, RegionFile::open(&path)?);
        }
        let region = map.get_mut(&key).unwrap();
        region.read_chunk(pos.x, pos.z)
    }

    fn write(&self, dimension: DimensionId, pos: ChunkPos, payload: &[u8]) -> Result<()> {
        let (rx, rz) = region_coords(pos);
        let key = (dimension, rx, rz);
        let mut map = self.open.lock().expect("region store mutex poisoned");

        if !map.contains_key(&key) {
            let dir = self.region_dir(dimension);
            std::fs::create_dir_all(&dir)?;
            let path = self.region_path(dimension, rx, rz);
            map.insert(key, RegionFile::open(&path)?);
        }
        let region = map.get_mut(&key).unwrap();
        region.write_chunk(pos.x, pos.z, payload, now_secs())
    }

    fn flush(&self) -> Result<()> {
        let mut map = self.open.lock().expect("region store mutex poisoned");
        for region in map.values_mut() {
            region.flush()?;
        }
        Ok(())
    }

    /// Copies the sectors of the chunk, as stored, to
    /// `<root>/region/<dim>/c.<cx>.<cz>.<n>.corrupt` next to its region
    /// file, with `n` the lowest number from 1 not taken yet. The file is
    /// created only if no file has that name, so an existing one is never
    /// overwritten, and it is synced to disk before this returns.
    fn quarantine(&self, dimension: DimensionId, pos: ChunkPos) -> Result<Option<PathBuf>> {
        let (rx, rz) = region_coords(pos);
        let path = self.region_path(dimension, rx, rz);
        let mut map = self.open.lock().expect("region store mutex poisoned");

        let key = (dimension, rx, rz);
        if !map.contains_key(&key) {
            if !path.exists() {
                return Ok(None);
            }
            map.insert(key, RegionFile::open(&path)?);
        }
        let region = map.get_mut(&key).unwrap();
        let Some(slot) = region.read_slot(pos.x, pos.z)? else {
            return Ok(None);
        };

        let dir = self.region_dir(dimension);
        for n in 1u32.. {
            let side = dir.join(format!("c.{}.{}.{n}.corrupt", pos.x, pos.z));
            match OpenOptions::new().write(true).create_new(true).open(&side) {
                Ok(mut file) => {
                    file.write_all(&slot)?;
                    file.sync_all()?;
                    return Ok(Some(side));
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err.into()),
            }
        }
        unreachable!("u32::MAX quarantine files for one chunk")
    }
}

impl Drop for RegionChunkStore {
    fn drop(&mut self) {
        // Best-effort durability if the explicit shutdown flush didn't run.
        let _ = self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(tag: u8) -> Vec<u8> {
        vec![tag; 100]
    }

    /// The region file of chunk (0, 0) in the overworld.
    fn region_0_0(root: &std::path::Path) -> PathBuf {
        root.join("region/overworld/r.0.0.vrm")
    }

    /// XORs the compressed bytes of the only chunk stored in `path`, which
    /// sits in the first sector after the header.
    fn damage_only_chunk(path: &std::path::Path) {
        let mut file = std::fs::read(path).unwrap();
        let start = 2 * 4096;
        let len = u32::from_be_bytes(file[start..start + 4].try_into().unwrap()) as usize;
        for byte in &mut file[start + 5..start + 4 + len] {
            *byte ^= 0x5A;
        }
        std::fs::write(path, file).unwrap();
    }

    #[test]
    fn damaged_compressed_data_reads_as_corrupt_not_as_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let pos = ChunkPos::new(0, 0);
        {
            let store = RegionChunkStore::new(dir.path());
            store
                .write(DimensionId::Overworld, pos, &payload(0x33))
                .unwrap();
        }
        damage_only_chunk(&region_0_0(dir.path()));

        let store = RegionChunkStore::new(dir.path());
        let result = store.read(DimensionId::Overworld, pos);
        assert!(
            matches!(result, Err(crate::PersistenceError::Corrupt(_))),
            "{result:?}"
        );
    }

    #[test]
    fn a_length_past_the_stored_sectors_reads_as_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let pos = ChunkPos::new(0, 0);
        {
            let store = RegionChunkStore::new(dir.path());
            store
                .write(DimensionId::Overworld, pos, &payload(0x33))
                .unwrap();
        }
        let path = region_0_0(dir.path());
        let mut file = std::fs::read(&path).unwrap();
        file[2 * 4096..2 * 4096 + 4].copy_from_slice(&0xFFFF_FFF0u32.to_be_bytes());
        std::fs::write(&path, file).unwrap();

        let store = RegionChunkStore::new(dir.path());
        let result = store.read(DimensionId::Overworld, pos);
        assert!(
            matches!(result, Err(crate::PersistenceError::Corrupt(_))),
            "{result:?}"
        );
    }

    #[test]
    fn quarantine_copies_the_stored_sectors_to_a_new_file_each_time() {
        let dir = tempfile::tempdir().unwrap();
        let store = RegionChunkStore::new(dir.path());
        let pos = ChunkPos::new(0, 0);
        store
            .write(DimensionId::Overworld, pos, &payload(0x33))
            .unwrap();
        store.flush().unwrap();
        damage_only_chunk(&region_0_0(dir.path()));
        let stored = std::fs::read(region_0_0(dir.path())).unwrap()[2 * 4096..3 * 4096].to_vec();

        // A file an operator left under the first name is not overwritten.
        let side_dir = dir.path().join("region/overworld");
        std::fs::write(side_dir.join("c.0.0.1.corrupt"), b"operator notes").unwrap();

        let first = store
            .quarantine(DimensionId::Overworld, pos)
            .unwrap()
            .expect("a copy");
        assert_eq!(first, side_dir.join("c.0.0.2.corrupt"));
        assert_eq!(std::fs::read(&first).unwrap(), stored);

        // A save over the slot, then a second failure of the same chunk.
        store
            .write(DimensionId::Overworld, pos, &payload(0x44))
            .unwrap();
        let rewritten = std::fs::read(region_0_0(dir.path())).unwrap()[2 * 4096..3 * 4096].to_vec();
        let second = store
            .quarantine(DimensionId::Overworld, pos)
            .unwrap()
            .expect("a copy");
        assert_eq!(second, side_dir.join("c.0.0.3.corrupt"));
        assert_eq!(std::fs::read(&second).unwrap(), rewritten);
        assert_eq!(std::fs::read(&first).unwrap(), stored);
        assert_eq!(
            std::fs::read(side_dir.join("c.0.0.1.corrupt")).unwrap(),
            b"operator notes"
        );
    }

    #[test]
    fn quarantine_of_a_chunk_not_stored_copies_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = RegionChunkStore::new(dir.path());
        assert_eq!(
            store
                .quarantine(DimensionId::Overworld, ChunkPos::new(0, 0))
                .unwrap(),
            None
        );
        store
            .write(DimensionId::Overworld, ChunkPos::new(1, 0), &payload(1))
            .unwrap();
        assert_eq!(
            store
                .quarantine(DimensionId::Overworld, ChunkPos::new(0, 0))
                .unwrap(),
            None
        );
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = RegionChunkStore::new(dir.path());
        let pos = ChunkPos::new(3, 7);
        store
            .write(DimensionId::Overworld, pos, &payload(0xAB))
            .unwrap();
        let got = store.read(DimensionId::Overworld, pos).unwrap();
        assert_eq!(got, Some(payload(0xAB)));
    }

    #[test]
    fn read_miss_returns_none_without_creating_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = RegionChunkStore::new(dir.path());
        let got = store
            .read(DimensionId::Overworld, ChunkPos::new(0, 0))
            .unwrap();
        assert_eq!(got, None);
        assert!(!dir.path().join("region").exists());
    }

    #[test]
    fn negative_coords_map_to_correct_slot() {
        let dir = tempfile::tempdir().unwrap();
        let store = RegionChunkStore::new(dir.path());
        for pos in [
            ChunkPos::new(-1, -1),
            ChunkPos::new(-32, -32),
            ChunkPos::new(-33, 5),
        ] {
            store
                .write(DimensionId::Overworld, pos, &payload(pos.x as u8))
                .unwrap();
            let got = store.read(DimensionId::Overworld, pos).unwrap();
            assert_eq!(got, Some(payload(pos.x as u8)), "pos {pos:?}");
        }
    }

    #[test]
    fn overwrite_with_larger_payload_reallocates() {
        let dir = tempfile::tempdir().unwrap();
        let store = RegionChunkStore::new(dir.path());
        let pos = ChunkPos::new(1, 1);
        store
            .write(DimensionId::Overworld, pos, &payload(1))
            .unwrap();
        // Much larger payload forces a multi-sector reallocation.
        let big = vec![0xCD; 9000];
        store.write(DimensionId::Overworld, pos, &big).unwrap();
        let got = store.read(DimensionId::Overworld, pos).unwrap();
        assert_eq!(got, Some(big));
    }

    #[test]
    fn reopen_from_disk_preserves_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let pos_a = ChunkPos::new(2, 2);
        let pos_b = ChunkPos::new(2, 3);
        {
            let store = RegionChunkStore::new(dir.path());
            store
                .write(DimensionId::Overworld, pos_a, &payload(0x11))
                .unwrap();
            store
                .write(DimensionId::Overworld, pos_b, &payload(0x22))
                .unwrap();
            store.flush().unwrap();
        }
        let store = RegionChunkStore::new(dir.path());
        assert_eq!(
            store.read(DimensionId::Overworld, pos_a).unwrap(),
            Some(payload(0x11))
        );
        assert_eq!(
            store.read(DimensionId::Overworld, pos_b).unwrap(),
            Some(payload(0x22))
        );
    }
}
