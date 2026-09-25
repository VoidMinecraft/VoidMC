mod block_changed_ack;
mod block_entity_data;
mod block_update;
mod boss_event;
pub mod chunk;
mod clear_titles;
mod close_container;
mod command_suggestions_response;
pub mod commands;
mod disconnect;
pub mod entity_metadata;
mod entity_position_sync;
mod game_event;
mod keep_alive;
mod level_particles;
mod login;
mod number_format;
mod open_screen;
mod ping;
mod player_abilities;
mod player_info_remove;
mod player_info_update;
mod remove_entities;
mod remove_mob_effect;
mod reset_score;
mod set_container_content;
mod set_container_slot;
mod set_cooldown;
mod set_cursor_item;
mod set_display_objective;
mod set_entity_data;
mod set_entity_motion;
mod set_head_rotation;
mod set_held_slot;
mod set_objective;
mod set_passengers;
mod set_player_team;
mod set_score;
mod set_subtitle_text;
mod set_tab_list_header_footer;
mod set_time;
mod set_title_text;
mod set_titles_animation;
mod sound;
mod spawn_entity;
mod synchronize_player_position;
mod system_chat;
mod teleport_entity;
mod unload_chunk;
mod update_advancements;
mod update_attributes;
mod update_entity_position;
mod update_entity_position_and_rotation;
mod update_entity_rotation;
mod update_mob_effect;
mod world_border;

pub use block_changed_ack::*;
pub use block_entity_data::*;
pub use block_update::*;
pub use boss_event::*;
pub use chunk::*;
pub use clear_titles::*;
pub use close_container::*;
pub use command_suggestions_response::*;
pub use commands::*;
pub use disconnect::*;
pub use entity_metadata::{
    Billboard, DisplayTransform, ItemDisplayContext, MAX_TELEPORT_TICKS, pack_brightness,
};
pub use entity_position_sync::*;
pub use game_event::*;
pub use keep_alive::*;
pub use level_particles::*;
pub use login::*;
pub use number_format::*;
pub use open_screen::*;
pub use ping::*;
pub use player_abilities::*;
pub use player_info_remove::*;
pub use player_info_update::*;
pub use remove_entities::*;
pub use remove_mob_effect::*;
pub use reset_score::*;
pub use set_container_content::*;
pub use set_container_slot::*;
pub use set_cooldown::*;
pub use set_cursor_item::*;
pub use set_display_objective::*;
pub use set_entity_data::*;
pub use set_entity_motion::*;
pub use set_head_rotation::*;
pub use set_held_slot::*;
pub use set_objective::*;
pub use set_passengers::*;
pub use set_player_team::*;
pub use set_score::*;
pub use set_subtitle_text::*;
pub use set_tab_list_header_footer::*;
pub use set_time::*;
pub use set_title_text::*;
pub use set_titles_animation::*;
pub use sound::*;
pub use spawn_entity::*;
pub use synchronize_player_position::*;
pub use system_chat::*;
pub use teleport_entity::*;
pub use unload_chunk::*;
pub use update_advancements::*;
pub use update_attributes::*;
pub use update_entity_position::*;
pub use update_entity_position_and_rotation::*;
pub use update_entity_rotation::*;
pub use update_mob_effect::*;
use voidmc_codec::Encode;
pub use world_border::*;

#[derive(Debug, Clone, Encode)]
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
    #[codec(packet_id = 0x0E)]
    ClearTitles(ClearTitles),
    #[codec(packet_id = 0x0F)]
    CommandSuggestionsResponse(CommandSuggestionsResponse),
    #[codec(packet_id = 0x10)]
    Commands(Commands),
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
    #[codec(packet_id = 0x23)]
    EntityPositionSync(EntityPositionSync),
    #[codec(packet_id = 0x25)]
    UnloadChunk(UnloadChunk),
    #[codec(packet_id = 0x26)]
    GameEvent(GameEvent),
    #[codec(packet_id = 0x2B)]
    InitializeBorder(InitializeBorder),
    #[codec(packet_id = 0x2C)]
    KeepAlive(KeepAlive),
    #[codec(packet_id = 0x2D)]
    ChunkDataAndLight(ChunkDataAndLight),
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
    #[codec(packet_id = 0x40)]
    PlayerAbilities(PlayerAbilities),
    #[codec(packet_id = 0x45)]
    PlayerInfoRemove(PlayerInfoRemove),
    #[codec(packet_id = 0x46)]
    PlayerInfoUpdate(PlayerInfoUpdate),
    #[codec(packet_id = 0x48)]
    SynchronizePlayerPosition(SynchronizePlayerPosition),
    #[codec(packet_id = 0x4D)]
    RemoveEntities(RemoveEntities),
    #[codec(packet_id = 0x4E)]
    RemoveMobEffect(RemoveMobEffect),
    #[codec(packet_id = 0x4F)]
    ResetScore(ResetScore),
    #[codec(packet_id = 0x53)]
    SetHeadRotation(SetHeadRotation),
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
    #[codec(packet_id = 0x62)]
    SetDisplayObjective(SetDisplayObjective),
    #[codec(packet_id = 0x63)]
    SetEntityData(SetEntityData),
    #[codec(packet_id = 0x65)]
    SetEntityMotion(SetEntityMotion),
    #[codec(packet_id = 0x69)]
    SetHeldSlot(SetHeldSlot),
    #[codec(packet_id = 0x6A)]
    SetObjective(SetObjective),
    #[codec(packet_id = 0x6B)]
    SetPassengers(SetPassengers),
    #[codec(packet_id = 0x6D)]
    SetPlayerTeam(SetPlayerTeam),
    #[codec(packet_id = 0x6E)]
    SetScore(SetScore),
    #[codec(packet_id = 0x70)]
    SetSubtitleText(SetSubtitleText),
    #[codec(packet_id = 0x71)]
    SetTime(SetTime),
    #[codec(packet_id = 0x72)]
    SetTitleText(SetTitleText),
    #[codec(packet_id = 0x73)]
    SetTitlesAnimation(SetTitlesAnimation),
    #[codec(packet_id = 0x74)]
    EntitySoundEffect(EntitySoundEffect),
    #[codec(packet_id = 0x75)]
    SoundEffect(SoundEffect),
    #[codec(packet_id = 0x77)]
    StopSound(StopSound),
    #[codec(packet_id = 0x79)]
    SystemChat(SystemChat),
    #[codec(packet_id = 0x7A)]
    SetTabListHeaderFooter(SetTabListHeaderFooter),
    #[codec(packet_id = 0x7D)]
    TeleportEntity(TeleportEntity),
    #[codec(packet_id = 0x82)]
    UpdateAdvancements(UpdateAdvancements),
    #[codec(packet_id = 0x83)]
    UpdateAttributes(UpdateAttributes),
    #[codec(packet_id = 0x84)]
    UpdateMobEffect(UpdateMobEffect),
}
