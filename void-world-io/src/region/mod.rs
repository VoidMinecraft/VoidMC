//! Default [`ChunkStore`] backed by sectorized region files on disk.

mod file;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use voidmc::{ChunkPos, DimensionId};

use crate::error::Result;
use crate::store::ChunkStore;
use file::RegionFile;

/// Stores chunks in `<root>/region/<dim>/r.<rx>.<rz>.vrm`, 32×32 chunks per file.
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
