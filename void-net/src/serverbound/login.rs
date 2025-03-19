mod login_start;
mod login_acknowledged;

pub use login_start::LoginStart;
pub use login_acknowledged::LoginAcknowledged;

use crate::{Packet, PacketDecode, PacketEncode, PacketId, StatePacket};

#[derive(Debug)]
pub enum LoginPacket {
    LoginStart(LoginStart),
    LoginAcknowledged(LoginAcknowledged),
}

impl Packet for LoginPacket {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        match self {
            LoginPacket::LoginStart(packet) => {
                encoder.encode_vari32(LoginStart::ID)?;
                packet.encode(encoder)
            }
            LoginPacket::LoginAcknowledged(packet) => {
                encoder.encode_vari32(LoginAcknowledged::ID)?;
                packet.encode(encoder)
            }
        }
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let id = decoder.decode_vari32()?;

        match id {
            LoginStart::ID => Ok(LoginPacket::LoginStart(LoginStart::decode(decoder)?)),
            LoginAcknowledged::ID => Ok(LoginPacket::LoginAcknowledged(LoginAcknowledged::decode(decoder)?)),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unknown packet ID: {}", id),
            )),
        }
    }
}

impl StatePacket for LoginPacket {}