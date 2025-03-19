mod game_event;
mod login;
mod synchronize_player_position;

pub use game_event::GameEvent;
pub use login::Login;
pub use synchronize_player_position::SynchronizePlayerPosition;

use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, StatePacket};

#[derive(Debug)]
pub enum PlayPacket {
    Login(Login),
    SynchronizePlayerPosition(SynchronizePlayerPosition),
    GameEvent(GameEvent),
}

impl Packet for PlayPacket {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        match self {
            PlayPacket::Login(packet) => {
                encoder.encode_vari32(Login::ID)?;
                packet.encode(encoder)
            }
            PlayPacket::SynchronizePlayerPosition(packet) => {
                encoder.encode_vari32(SynchronizePlayerPosition::ID)?;
                packet.encode(encoder)
            }
            PlayPacket::GameEvent(packet) => {
                encoder.encode_vari32(GameEvent::ID)?;
                packet.encode(encoder)
            }
        }
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let id = decoder.decode_vari32()?;

        match id {
            Login::ID => {
                let packet = Login::decode(decoder)?;
                Ok(PlayPacket::Login(packet))
            }
            SynchronizePlayerPosition::ID => {
                let packet = SynchronizePlayerPosition::decode(decoder)?;
                Ok(PlayPacket::SynchronizePlayerPosition(packet))
            }
            GameEvent::ID => {
                let packet = GameEvent::decode(decoder)?;
                Ok(PlayPacket::GameEvent(packet))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid packet ID: {}", id),
            )),
        }
    }
}

impl StatePacket for PlayPacket {}
