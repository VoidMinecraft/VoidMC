use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

#[derive(Debug, Clone, Copy, TryFromPrimitive, IntoPrimitive)]
#[repr(i32)]
pub enum ChatMode {
    Enabled = 0x0,
    CommandOnly = 0x1,
    Hidden = 0x2,
}

#[derive(Debug, Clone, Copy, TryFromPrimitive, IntoPrimitive)]
#[repr(i32)]
pub enum MainHand {
    Left = 0x0,
    Right = 0x1,
}

#[derive(Debug, Clone, Copy, TryFromPrimitive, IntoPrimitive)]
#[repr(i32)]
pub enum ParticleStatus {
    All = 0x0,
    Decreased = 0x1,
    Minimal = 0x2,
}

#[derive(Debug)]
pub struct ClientInformation {
    pub locale: String,
    pub view_distance: u8,
    pub chat_mode: ChatMode,
    pub chat_colors: bool,
    pub skin_parts: u8,
    pub main_hand: MainHand,
    pub enable_text_filtering: bool,
    pub allow_server_listings: bool,
    pub particle_status: ParticleStatus,
}

impl Packet for ClientInformation {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_str(&self.locale)?;
        encoder.encode_u8(self.view_distance)?;
        encoder.encode_vari32(self.chat_mode.into())?;
        encoder.encode_u8(self.chat_colors.into())?;
        encoder.encode_u8(self.skin_parts)?;
        encoder.encode_vari32(self.main_hand.into())?;
        encoder.encode_u8(self.enable_text_filtering.into())?;
        encoder.encode_u8(self.allow_server_listings.into())?;
        encoder.encode_vari32(self.particle_status.into())
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let locale = decoder.decode_str()?;
        let view_distance = decoder.decode_u8()?;
        let chat_mode: ChatMode = decoder.decode_vari32()?.try_into().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid chat_mode field in ClientInformation packet",
            )
        })?;
        let chat_colors = decoder.decode_u8()? != 0;
        let skin_parts = decoder.decode_u8()?;
        let main_hand: MainHand = decoder.decode_vari32()?.try_into().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid main_hand field in ClientInformation packet",
            )
        })?;
        let enable_text_filtering = decoder.decode_u8()? != 0;
        let allow_server_listings = decoder.decode_u8()? != 0;
        let particle_status: ParticleStatus =
            decoder.decode_vari32()?.try_into().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid particle_status field in ClientInformation packet",
                )
            })?;

        Ok(Self {
            locale,
            view_distance,
            chat_mode,
            chat_colors,
            skin_parts,
            main_hand,
            enable_text_filtering,
            allow_server_listings,
            particle_status,
        })
    }
}

impl PacketId for ClientInformation {
    const ID: i32 = 0x00;
}
