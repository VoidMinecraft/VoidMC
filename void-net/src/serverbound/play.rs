mod set_player_pos;
mod tick_end;

pub use set_player_pos::SetPlayerPos;
pub use tick_end::TickEnd;

use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, StatePacket};

#[derive(Debug)]
pub enum PlayPacket {
    SetPlayerPos(SetPlayerPos),
    TickEnd(TickEnd),
}

impl Packet for PlayPacket {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        match self {
            PlayPacket::SetPlayerPos(packet) => {
                encoder.encode_vari32(SetPlayerPos::ID)?;
                packet.encode(encoder)
            }
            PlayPacket::TickEnd(packet) => {
                encoder.encode_vari32(TickEnd::ID)?;
                packet.encode(encoder)
            }
        }
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let id = decoder.decode_vari32()?;

        match id {
            SetPlayerPos::ID => {
                let packet = SetPlayerPos::decode(decoder)?;
                Ok(PlayPacket::SetPlayerPos(packet))
            }
            TickEnd::ID => {
                let packet = TickEnd::decode(decoder)?;
                Ok(PlayPacket::TickEnd(packet))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid packet ID: {}", id),
            )),
        }
    }
}

impl StatePacket for PlayPacket {}
