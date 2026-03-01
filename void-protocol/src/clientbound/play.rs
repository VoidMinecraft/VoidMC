mod game_event;
mod keep_alive;
mod login;
mod ping;
mod spawn_entity;
mod synchronize_player_position;
mod update_entity_position;
mod update_entity_position_and_rotation;

pub use game_event::*;
pub use keep_alive::*;
pub use login::*;
pub use ping::*;
pub use spawn_entity::*;
pub use synchronize_player_position::*;
pub use update_entity_position::*;
pub use update_entity_position_and_rotation::*;
use void_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
#[codec(tagged)]
pub enum PlayPacket {
    #[codec(packet_id = 0x01)]
    SpawnEntity(SpawnEntity),
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
    #[codec(packet_id = 0x37)]
    Ping(Ping),
    #[codec(packet_id = 0x42)]
    SynchronizePlayerPosition(SynchronizePlayerPosition),
}
