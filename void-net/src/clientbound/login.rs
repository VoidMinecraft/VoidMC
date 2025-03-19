mod login_success;

pub use login_success::LoginSuccess;

use crate::{Packet, PacketDecode, PacketEncode, PacketId, StatePacket};

#[derive(Debug)]
pub enum LoginPacket {
    LoginSuccess(LoginSuccess),
}

impl Packet for LoginPacket {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        match self {
            LoginPacket::LoginSuccess(packet) => {
                encoder.encode_vari32(LoginSuccess::ID)?;
                packet.encode(encoder)
            }
        }
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let id = decoder.decode_vari32()?;

        match id {
            LoginSuccess::ID => Ok(LoginPacket::LoginSuccess(LoginSuccess::decode(decoder)?)),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid packet ID: {}", id),
            )),
        }
    }
}

impl StatePacket for LoginPacket {}
