mod game_event;
mod login;
mod synchronize_player_position;

pub use game_event::*;
pub use login::*;
pub use synchronize_player_position::*;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum PlayPacket {
    #[codec(packet_id = 0x23)]
    GameEvent(GameEvent),
    #[codec(packet_id = 0x2C)]
    Login(Login),
    #[codec(packet_id = 0x42)]
    SynchronizePlayerPosition(SynchronizePlayerPosition),
}
