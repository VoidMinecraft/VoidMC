use uuid::Uuid;
use voidmc_codec::{Encode, VarI32};

#[derive(Debug, Clone)]
pub struct PlayerInfoEntry {
    pub uuid: Uuid,
    pub name: String,
    pub game_mode: i32,
    pub listed: bool,
}

/// PlayerInfoUpdate with actions: AddPlayer (0x01) | UpdateGameMode (0x04) | UpdateListed (0x08) = 0x0D
#[derive(Debug, Clone)]
pub struct PlayerInfoUpdate {
    pub entries: Vec<PlayerInfoEntry>,
}

impl Encode for PlayerInfoUpdate {
    fn encode(&self, buf: &mut Vec<u8>) {
        // Actions bitmask: AddPlayer | UpdateGameMode | UpdateListed
        buf.push(0x0D);
        // Player count
        VarI32(self.entries.len() as i32).encode(buf);
        for entry in &self.entries {
            // UUID
            entry.uuid.encode(buf);
            // AddPlayer action: name + properties count (0)
            entry.name.encode(buf);
            VarI32(0).encode(buf); // 0 properties
            // UpdateGameMode action: VarInt game_mode
            VarI32(entry.game_mode).encode(buf);
            // UpdateListed action: bool listed
            entry.listed.encode(buf);
        }
    }
}
