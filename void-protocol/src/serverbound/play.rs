mod confirm_teleportation;
mod player_loaded;
mod set_player_pos;
mod set_player_pos_and_rot;
mod tick_end;

pub use confirm_teleportation::*;
pub use player_loaded::*;
pub use set_player_pos::*;
pub use set_player_pos_and_rot::*;
pub use tick_end::*;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum PlayPacket {
    #[codec(packet_id = 0x00)]
    ConfirmTeleportation(ConfirmTeleportation),
    #[codec(packet_id = 0x0B)]
    TickEnd(TickEnd),
    #[codec(packet_id = 0x1C)]
    SetPlayerPos(SetPlayerPos),
    #[codec(packet_id = 0x1D)]
    SetPlayerPosAndRot(SetPlayerPosAndRot),
    #[codec(packet_id = 0x2A)]
    PlayerLoaded(PlayerLoaded),
}
