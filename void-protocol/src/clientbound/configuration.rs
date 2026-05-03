mod finish_configuration;
mod known_packs;
mod registry_data;
mod update_tags;

pub use finish_configuration::*;
pub use known_packs::*;
pub use registry_data::*;
pub use update_tags::*;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
#[codec(tagged)]
pub enum ConfigurationPacket {
    #[codec(packet_id = 0x03)]
    FinishConfiguration(FinishConfiguration),
    #[codec(packet_id = 0x07)]
    RegistryData(RegistryData),
    #[codec(packet_id = 0x0E)]
    KnownPacks(KnownPacks),
}

/// Packets in the configuration phase whose body has a manual `Encode` impl
/// (e.g. uses VarInt arrays) and therefore can't live inside the tagged enum.
#[derive(Debug, Clone)]
pub enum ManualConfigurationPacket {
    UpdateTags(UpdateTags),
}

impl Encode for ManualConfigurationPacket {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            ManualConfigurationPacket::UpdateTags(packet) => {
                voidmc_codec::VarI32(0x0D).encode(buf);
                packet.encode(buf);
            }
        }
    }
}
