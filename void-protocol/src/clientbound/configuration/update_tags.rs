use voidmc_codec::{Encode, VarI32};

/// A single tag's flat list of registry indices.
#[derive(Debug, Clone)]
pub struct TagEntry {
    pub tag_id: String,
    pub entries: Vec<i32>,
}

/// Tags grouped by their target registry.
#[derive(Debug, Clone)]
pub struct RegistryTags {
    pub registry_id: String,
    pub tags: Vec<TagEntry>,
}

/// Clientbound `Update Tags` packet (configuration phase, 0x0D).
#[derive(Debug, Clone)]
pub struct UpdateTags {
    pub registries: Vec<RegistryTags>,
}

impl Encode for UpdateTags {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.registries.len() as i32).encode(buf);
        for reg in &self.registries {
            reg.registry_id.encode(buf);
            VarI32(reg.tags.len() as i32).encode(buf);
            for tag in &reg.tags {
                tag.tag_id.encode(buf);
                VarI32(tag.entries.len() as i32).encode(buf);
                for &id in &tag.entries {
                    VarI32(id).encode(buf);
                }
            }
        }
    }
}
