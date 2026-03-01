mod finish_configuration;
mod known_packs;
mod registry_data;

pub use finish_configuration::*;
pub use known_packs::*;
pub use registry_data::*;
use void_codec::{Decode, Encode};

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
