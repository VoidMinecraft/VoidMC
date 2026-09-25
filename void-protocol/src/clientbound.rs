mod configuration;
mod login;
mod play;
mod status;

pub use configuration::*;
pub use login::*;
pub use play::*;
pub use status::*;

#[derive(Debug, Clone)]
pub enum ClientboundPacket {
    Status(StatusPacket),
    Login(LoginPacket),
    Configuration(ConfigurationPacket),
    Play(PlayPacket),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_structs_convert_into_clientbound() {
        let packet: ClientboundPacket = KeepAlive { keep_alive_id: 7 }.into();
        assert!(matches!(
            packet,
            ClientboundPacket::Play(PlayPacket::KeepAlive(KeepAlive { keep_alive_id: 7 }))
        ));

        let packet: ClientboundPacket = FinishConfiguration {}.into();
        assert!(matches!(
            packet,
            ClientboundPacket::Configuration(ConfigurationPacket::FinishConfiguration(_))
        ));

        let packet: ClientboundPacket = UpdateTags { registries: vec![] }.into();
        assert!(matches!(
            packet,
            ClientboundPacket::Configuration(ConfigurationPacket::UpdateTags(_))
        ));

        let packet: ClientboundPacket = SetPassengers {
            entity_id: 1,
            passengers: vec![],
        }
        .into();
        assert!(matches!(
            packet,
            ClientboundPacket::Play(PlayPacket::SetPassengers(_))
        ));
    }
}
