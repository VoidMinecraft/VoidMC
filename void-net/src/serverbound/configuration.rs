mod client_information;
mod finish_configuration_acknowledged;
mod known_packs;

pub use client_information::{ChatMode, ClientInformation, MainHand, ParticleStatus};
pub use finish_configuration_acknowledged::FinishConfigurationAcknowledged;
pub use known_packs::{KnownPack, KnownPacks};

use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, StatePacket};

#[derive(Debug)]
pub enum ConfigurationPacket {
    KnownPacks(KnownPacks),
    ClientInformation(ClientInformation),
    FinishConfigurationAcknowledged(FinishConfigurationAcknowledged),
}

impl Packet for ConfigurationPacket {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        match self {
            ConfigurationPacket::KnownPacks(packet) => {
                encoder.encode_vari32(KnownPacks::ID)?;
                packet.encode(encoder)
            }
            ConfigurationPacket::ClientInformation(packet) => {
                encoder.encode_vari32(ClientInformation::ID)?;
                packet.encode(encoder)
            }
            ConfigurationPacket::FinishConfigurationAcknowledged(packet) => {
                encoder.encode_vari32(FinishConfigurationAcknowledged::ID)?;
                packet.encode(encoder)
            }
        }
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let id = decoder.decode_vari32()?;

        match id {
            KnownPacks::ID => {
                let packet = KnownPacks::decode(decoder)?;
                Ok(ConfigurationPacket::KnownPacks(packet))
            }
            ClientInformation::ID => {
                let packet = ClientInformation::decode(decoder)?;
                Ok(ConfigurationPacket::ClientInformation(packet))
            }
            FinishConfigurationAcknowledged::ID => {
                let packet = FinishConfigurationAcknowledged::decode(decoder)?;
                Ok(ConfigurationPacket::FinishConfigurationAcknowledged(packet))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unknown packet ID: {}", id),
            )),
        }
    }
}

impl StatePacket for ConfigurationPacket {}
