use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use serde_json::json;
use sha1::{Digest, Sha1};
use uuid::Uuid;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::glyph::Image;

/// Resource pack format of Minecraft 26.1.2.
const PACK_FORMAT: u32 = 84;
const TOOLTIP_FRAMES: [&str; 2] = [
    "assets/minecraft/textures/gui/sprites/tooltip/background.png",
    "assets/minecraft/textures/gui/sprites/tooltip/frame.png",
];

/// A resource pack assembled in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePack {
    description: String,
    files: BTreeMap<String, Vec<u8>>,
}

impl ResourcePack {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            files: BTreeMap::new(),
        }
    }

    pub fn add(&mut self, path: impl Into<String>, bytes: Vec<u8>) {
        self.files.insert(path.into(), bytes);
    }

    pub fn contains(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    /// Blanks the tooltip box, for [`crate::Cursor`]. It applies to every tooltip
    /// of the client while the pack is loaded, item tooltips included.
    pub fn hide_tooltip_frames(&mut self) {
        let blank = Image::new(16, 16).to_png();
        for path in TOOLTIP_FRAMES {
            self.add(path, blank.clone());
        }
    }

    pub fn build(&self) -> BuiltPack {
        let meta = json!({ "pack": {
            "description": self.description,
            "min_format": PACK_FORMAT,
            "max_format": PACK_FORMAT,
        }});
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        // The default timestamp is fixed, so equal content gives an equal hash.
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let meta = meta.to_string().into_bytes();
        let files = std::iter::once(("pack.mcmeta", &meta)).chain(
            self.files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes)),
        );
        for (path, bytes) in files {
            zip.start_file(path, options).expect("the path is new");
            zip.write_all(bytes).expect("writing to a Vec cannot fail");
        }
        let bytes = zip
            .finish()
            .expect("writing to a Vec cannot fail")
            .into_inner();
        BuiltPack::new(bytes)
    }
}

/// The archive to send to clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltPack {
    bytes: Vec<u8>,
    sha1: [u8; 20],
}

impl BuiltPack {
    fn new(bytes: Vec<u8>) -> Self {
        let sha1 = Sha1::digest(&bytes).into();
        Self { bytes, sha1 }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Lowercase hex, as the client expects. It lets clients cache the pack.
    pub fn hash(&self) -> String {
        self.sha1.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Stable for equal content, and different when the pack changes.
    pub fn id(&self) -> Uuid {
        let mut id = [0; 16];
        id.copy_from_slice(&self.sha1[..16]);
        Uuid::from_bytes(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> ResourcePack {
        let mut pack = ResourcePack::new("test");
        pack.add("assets/a.txt", b"hello".to_vec());
        pack
    }

    #[test]
    fn building_twice_gives_the_same_archive() {
        assert_eq!(pack().build(), pack().build());
    }

    #[test]
    fn changing_a_file_changes_the_id_and_hash() {
        let mut other = pack();
        other.add("assets/a.txt", b"bye".to_vec());
        assert_ne!(pack().build().hash(), other.build().hash());
        assert_ne!(pack().build().id(), other.build().id());
    }

    #[test]
    fn the_hash_is_forty_hex_characters() {
        let hash = pack().build().hash();
        assert_eq!(hash.len(), 40);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_archive_holds_the_metadata_and_files() {
        let built = pack().build();
        let mut archive = zip::ZipArchive::new(Cursor::new(built.bytes())).unwrap();
        assert!(archive.by_name("pack.mcmeta").is_ok());
        assert!(archive.by_name("assets/a.txt").is_ok());
    }

    #[test]
    fn hiding_tooltips_replaces_both_sprites() {
        let mut pack = pack();
        pack.hide_tooltip_frames();
        assert!(TOOLTIP_FRAMES.iter().all(|path| pack.contains(path)));
    }
}
