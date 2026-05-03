pub mod chunk;
mod command_suggestions_response;
pub mod commands;
mod disconnect;
mod game_event;
mod keep_alive;
mod login;
mod ping;
mod player_info_remove;
mod player_info_update;
mod remove_entities;
mod set_head_rotation;
mod spawn_entity;
mod synchronize_player_position;
mod system_chat;
mod unload_chunk;
mod update_entity_position;
mod update_entity_position_and_rotation;
mod update_entity_rotation;

pub use chunk::*;
pub use command_suggestions_response::*;
pub use commands::*;
pub use disconnect::*;
pub use game_event::*;
pub use keep_alive::*;
pub use login::*;
pub use ping::*;
pub use player_info_remove::*;
pub use player_info_update::*;
pub use remove_entities::*;
pub use set_head_rotation::*;
pub use spawn_entity::*;
pub use synchronize_player_position::*;
pub use system_chat::*;
pub use unload_chunk::*;
pub use update_entity_position::*;
pub use update_entity_position_and_rotation::*;
pub use update_entity_rotation::*;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
#[codec(tagged)]
pub enum PlayPacket {
    #[codec(packet_id = 0x01)]
    SpawnEntity(SpawnEntity),
    #[codec(packet_id = 0x20)]
    Disconnect(Disconnect),
    #[codec(packet_id = 0x25)]
    UnloadChunk(UnloadChunk),
    #[codec(packet_id = 0x26)]
    GameEvent(GameEvent),
    #[codec(packet_id = 0x2C)]
    KeepAlive(KeepAlive),
    #[codec(packet_id = 0x31)]
    Login(Login),
    #[codec(packet_id = 0x35)]
    UpdateEntityPosition(UpdateEntityPosition),
    #[codec(packet_id = 0x36)]
    UpdateEntityPositionAndRotation(UpdateEntityPositionAndRotation),
    #[codec(packet_id = 0x38)]
    UpdateEntityRotation(UpdateEntityRotation),
    #[codec(packet_id = 0x3D)]
    Ping(Ping),
    #[codec(packet_id = 0x48)]
    SynchronizePlayerPosition(SynchronizePlayerPosition),
    #[codec(packet_id = 0x53)]
    SetHeadRotation(SetHeadRotation),
    #[codec(packet_id = 0x5E)]
    SetCenterChunk(SetCenterChunk),
    #[codec(packet_id = 0x79)]
    SystemChat(SystemChat),
}

/// Packets with manual Encode impls that can't be in the tagged enum.
/// These are encoded directly with their packet ID prepended.
#[derive(Debug, Clone)]
pub enum ManualPlayPacket {
    PlayerInfoUpdate(PlayerInfoUpdate),
    PlayerInfoRemove(PlayerInfoRemove),
    RemoveEntities(RemoveEntities),
    ChunkDataAndLight(ChunkDataAndLight),
    Commands(Commands),
    CommandSuggestionsResponse(CommandSuggestionsResponse),
}

impl Encode for ManualPlayPacket {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            ManualPlayPacket::PlayerInfoUpdate(packet) => {
                voidmc_codec::VarI32(0x46).encode(buf);
                packet.encode(buf);
            }
            ManualPlayPacket::PlayerInfoRemove(packet) => {
                voidmc_codec::VarI32(0x45).encode(buf);
                packet.encode(buf);
            }
            ManualPlayPacket::RemoveEntities(packet) => {
                voidmc_codec::VarI32(0x4D).encode(buf);
                packet.encode(buf);
            }
            ManualPlayPacket::ChunkDataAndLight(packet) => {
                voidmc_codec::VarI32(0x2D).encode(buf);
                packet.encode(buf);
            }
            ManualPlayPacket::Commands(packet) => {
                voidmc_codec::VarI32(0x10).encode(buf);
                packet.encode(buf);
            }
            ManualPlayPacket::CommandSuggestionsResponse(packet) => {
                voidmc_codec::VarI32(0x0F).encode(buf);
                packet.encode(buf);
            }
        }
    }
}
