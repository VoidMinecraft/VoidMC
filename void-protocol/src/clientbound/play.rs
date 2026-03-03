pub mod chunk;
mod game_event;
mod keep_alive;
mod login;
mod ping;
mod player_info_remove;
mod player_info_update;
mod remove_entities;
mod set_center_chunk;
mod set_head_rotation;
mod spawn_entity;
mod synchronize_player_position;
mod unload_chunk;
mod update_entity_position;
mod update_entity_position_and_rotation;
mod update_entity_rotation;

pub use chunk::*;
pub use game_event::*;
pub use keep_alive::*;
pub use login::*;
pub use ping::*;
pub use player_info_remove::*;
pub use player_info_update::*;
pub use remove_entities::*;
pub use set_center_chunk::*;
pub use set_head_rotation::*;
pub use spawn_entity::*;
pub use synchronize_player_position::*;
pub use unload_chunk::*;
pub use update_entity_position::*;
pub use update_entity_position_and_rotation::*;
pub use update_entity_rotation::*;
use void_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
#[codec(tagged)]
pub enum PlayPacket {
    #[codec(packet_id = 0x01)]
    SpawnEntity(SpawnEntity),
    #[codec(packet_id = 0x22)]
    UnloadChunk(UnloadChunk),
    #[codec(packet_id = 0x23)]
    GameEvent(GameEvent),
    #[codec(packet_id = 0x27)]
    KeepAlive(KeepAlive),
    #[codec(packet_id = 0x2C)]
    Login(Login),
    #[codec(packet_id = 0x2F)]
    UpdateEntityPosition(UpdateEntityPosition),
    #[codec(packet_id = 0x30)]
    UpdateEntityPositionAndRotation(UpdateEntityPositionAndRotation),
    #[codec(packet_id = 0x32)]
    UpdateEntityRotation(UpdateEntityRotation),
    #[codec(packet_id = 0x37)]
    Ping(Ping),
    #[codec(packet_id = 0x42)]
    SynchronizePlayerPosition(SynchronizePlayerPosition),
    #[codec(packet_id = 0x4D)]
    SetHeadRotation(SetHeadRotation),
    #[codec(packet_id = 0x58)]
    SetCenterChunk(SetCenterChunk),
}

/// Packets with manual Encode impls that can't be in the tagged enum.
/// These are encoded directly with their packet ID prepended.
#[derive(Debug, Clone)]
pub enum ManualPlayPacket {
    PlayerInfoUpdate(PlayerInfoUpdate),
    PlayerInfoRemove(PlayerInfoRemove),
    RemoveEntities(RemoveEntities),
    ChunkDataAndLight(ChunkDataAndLight),
}

impl Encode for ManualPlayPacket {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            ManualPlayPacket::PlayerInfoUpdate(packet) => {
                void_codec::VarI32(0x40).encode(buf);
                packet.encode(buf);
            }
            ManualPlayPacket::PlayerInfoRemove(packet) => {
                void_codec::VarI32(0x3F).encode(buf);
                packet.encode(buf);
            }
            ManualPlayPacket::RemoveEntities(packet) => {
                void_codec::VarI32(0x47).encode(buf);
                packet.encode(buf);
            }
            ManualPlayPacket::ChunkDataAndLight(packet) => {
                void_codec::VarI32(0x28).encode(buf);
                packet.encode(buf);
            }
        }
    }
}
