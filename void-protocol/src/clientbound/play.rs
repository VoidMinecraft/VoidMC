mod block_changed_ack;
mod block_entity_data;
mod block_update;
mod boss_event;
pub mod chunk;
mod close_container;
mod command_suggestions_response;
pub mod commands;
mod disconnect;
pub mod entity_metadata;
mod game_event;
mod keep_alive;
mod level_particles;
mod login;
mod open_screen;
mod ping;
mod player_info_remove;
mod player_info_update;
mod remove_entities;
mod set_container_content;
mod set_container_slot;
mod set_cooldown;
mod set_cursor_item;
mod set_entity_data;
mod set_entity_motion;
mod set_head_rotation;
mod set_held_slot;
mod set_passengers;
mod sound;
mod spawn_entity;
mod synchronize_player_position;
mod system_chat;
mod teleport_entity;
mod unload_chunk;
mod update_entity_position;
mod update_entity_position_and_rotation;
mod update_entity_rotation;
mod world_border;

pub use block_changed_ack::*;
pub use block_entity_data::*;
pub use block_update::*;
pub use boss_event::*;
pub use chunk::*;
pub use close_container::*;
pub use command_suggestions_response::*;
pub use commands::*;
pub use disconnect::*;
pub use entity_metadata::{
    Billboard, DisplayTransform, ItemDisplayContext, MAX_TELEPORT_TICKS, pack_brightness,
};
pub use game_event::*;
pub use keep_alive::*;
pub use level_particles::*;
pub use login::*;
pub use open_screen::*;
pub use ping::*;
pub use player_info_remove::*;
pub use player_info_update::*;
pub use remove_entities::*;
pub use set_container_content::*;
pub use set_container_slot::*;
pub use set_cooldown::*;
pub use set_cursor_item::*;
pub use set_entity_data::*;
pub use set_entity_motion::*;
pub use set_head_rotation::*;
pub use set_held_slot::*;
pub use set_passengers::*;
pub use sound::*;
pub use spawn_entity::*;
pub use synchronize_player_position::*;
pub use system_chat::*;
pub use teleport_entity::*;
pub use unload_chunk::*;
pub use update_entity_position::*;
pub use update_entity_position_and_rotation::*;
pub use update_entity_rotation::*;
use voidmc_codec::{Decode, Encode};
pub use world_border::*;

#[derive(Debug, Clone, Encode, Decode)]
#[codec(tagged, wrap = crate::clientbound::ClientboundPacket::Play)]
pub enum PlayPacket {
    #[codec(packet_id = 0x01)]
    SpawnEntity(SpawnEntity),
    #[codec(packet_id = 0x04)]
    BlockChangedAck(BlockChangedAck),
    #[codec(packet_id = 0x06)]
    BlockEntityData(BlockEntityData),
    #[codec(packet_id = 0x08)]
    BlockUpdate(BlockUpdate),
    #[codec(packet_id = 0x09)]
    BossEvent(BossEvent),
    #[codec(packet_id = 0x0D)]
    ChunksBiomes(ChunksBiomes),
    #[codec(packet_id = 0x11)]
    CloseContainer(CloseContainer),
    #[codec(packet_id = 0x12)]
    SetContainerContent(SetContainerContent),
    #[codec(packet_id = 0x14)]
    SetContainerSlot(SetContainerSlot),
    #[codec(packet_id = 0x16)]
    SetCooldown(SetCooldown),
    #[codec(packet_id = 0x20)]
    Disconnect(Disconnect),
    #[codec(packet_id = 0x25)]
    UnloadChunk(UnloadChunk),
    #[codec(packet_id = 0x26)]
    GameEvent(GameEvent),
    #[codec(packet_id = 0x2B)]
    InitializeBorder(InitializeBorder),
    #[codec(packet_id = 0x2C)]
    KeepAlive(KeepAlive),
    #[codec(packet_id = 0x2F)]
    LevelParticles(LevelParticles),
    #[codec(packet_id = 0x31)]
    Login(Login),
    #[codec(packet_id = 0x35)]
    UpdateEntityPosition(UpdateEntityPosition),
    #[codec(packet_id = 0x36)]
    UpdateEntityPositionAndRotation(UpdateEntityPositionAndRotation),
    #[codec(packet_id = 0x38)]
    UpdateEntityRotation(UpdateEntityRotation),
    #[codec(packet_id = 0x3B)]
    OpenScreen(OpenScreen),
    #[codec(packet_id = 0x3D)]
    Ping(Ping),
    #[codec(packet_id = 0x48)]
    SynchronizePlayerPosition(SynchronizePlayerPosition),
    #[codec(packet_id = 0x53)]
    SetHeadRotation(SetHeadRotation),
    #[codec(packet_id = 0x65)]
    SetEntityMotion(SetEntityMotion),
    #[codec(packet_id = 0x58)]
    SetBorderCenter(SetBorderCenter),
    #[codec(packet_id = 0x59)]
    SetBorderLerpSize(SetBorderLerpSize),
    #[codec(packet_id = 0x5A)]
    SetBorderSize(SetBorderSize),
    #[codec(packet_id = 0x5B)]
    SetBorderWarningDelay(SetBorderWarningDelay),
    #[codec(packet_id = 0x5C)]
    SetBorderWarningDistance(SetBorderWarningDistance),
    #[codec(packet_id = 0x5E)]
    SetCenterChunk(SetCenterChunk),
    #[codec(packet_id = 0x60)]
    SetCursorItem(SetCursorItem),
    #[codec(packet_id = 0x63)]
    SetEntityData(SetEntityData),
    #[codec(packet_id = 0x69)]
    SetHeldSlot(SetHeldSlot),
    #[codec(packet_id = 0x74)]
    EntitySoundEffect(EntitySoundEffect),
    #[codec(packet_id = 0x75)]
    SoundEffect(SoundEffect),
    #[codec(packet_id = 0x77)]
    StopSound(StopSound),
    #[codec(packet_id = 0x79)]
    SystemChat(SystemChat),
    #[codec(packet_id = 0x7D)]
    TeleportEntity(TeleportEntity),
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
    SetPassengers(SetPassengers),
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
            ManualPlayPacket::SetPassengers(packet) => {
                voidmc_codec::VarI32(0x6B).encode(buf);
                packet.encode(buf);
            }
        }
    }
}
