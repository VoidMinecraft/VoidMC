mod client_information;
mod finish_configuration_acknowledged;
mod known_packs;
mod plugin_message;

pub use client_information::{ChatMode, ClientInformation, MainHand, ParticleStatus};
pub use finish_configuration_acknowledged::FinishConfigurationAcknowledged;
pub use known_packs::{KnownPack, KnownPacks};
pub use plugin_message::PluginMessage;
use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum ConfigurationPacket {
    #[codec(packet_id = 0x00)]
    ClientInformation(ClientInformation),
    #[codec(packet_id = 0x02)]
    PluginMessage(PluginMessage),
    #[codec(packet_id = 0x03)]
    FinishConfigurationAcknowledged(FinishConfigurationAcknowledged),
    #[codec(packet_id = 0x07)]
    KnownPacks(KnownPacks),
}
