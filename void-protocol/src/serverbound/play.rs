mod confirm_teleportation;
mod set_player_pos;
mod tick_end;

pub use confirm_teleportation::*;
pub use set_player_pos::*;
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
}
