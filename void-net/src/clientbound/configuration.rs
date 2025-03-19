mod finish_configuration;
mod known_packs;
mod registry_data;

pub use finish_configuration::FinishConfiguration;
pub use known_packs::{KnownPack, KnownPacks};
pub use registry_data::{RegistryData, RegistryEntry};

use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId, StatePacket};

#[derive(Debug)]
pub enum ConfigurationPacket {
    KnownPacks(KnownPacks),
    RegistryData(RegistryData),
    FinishConfiguration(FinishConfiguration),
}

impl Packet for ConfigurationPacket {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        match self {
            ConfigurationPacket::KnownPacks(packet) => {
                encoder.encode_vari32(KnownPacks::ID)?;
                packet.encode(encoder)
            }
            ConfigurationPacket::RegistryData(packet) => {
                encoder.encode_vari32(RegistryData::ID)?;
                packet.encode(encoder)
            }
            ConfigurationPacket::FinishConfiguration(packet) => {
                encoder.encode_vari32(FinishConfiguration::ID)?;
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
            RegistryData::ID => {
                let packet = RegistryData::decode(decoder)?;
                Ok(ConfigurationPacket::RegistryData(packet))
            }
            FinishConfiguration::ID => {
                let packet = FinishConfiguration::decode(decoder)?;
                Ok(ConfigurationPacket::FinishConfiguration(packet))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid packet ID: {}", id),
            )),
        }
    }
}

impl StatePacket for ConfigurationPacket {}
