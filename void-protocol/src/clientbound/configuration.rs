mod finish_configuration;
mod known_packs;
mod registry_data;
mod update_tags;

pub use finish_configuration::*;
pub use known_packs::*;
pub use registry_data::*;
pub use update_tags::*;
use voidmc_codec::Encode;

#[derive(Debug, Clone, Encode)]
#[codec(tagged, wrap = crate::clientbound::ClientboundPacket::Configuration)]
pub enum ConfigurationPacket {
    #[codec(packet_id = 0x03)]
    FinishConfiguration(FinishConfiguration),
    #[codec(packet_id = 0x07)]
    RegistryData(RegistryData),
    #[codec(packet_id = 0x0D)]
    UpdateTags(UpdateTags),
    #[codec(packet_id = 0x0E)]
    KnownPacks(KnownPacks),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_tags_is_a_configuration_packet() {
        let packet = ConfigurationPacket::UpdateTags(UpdateTags { registries: vec![] });
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        assert_eq!(bytes, [0x0D, 0x00]);
    }
}
