use voidmc_codec::{Encode, PacketName};

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
    ManualConfiguration(ManualConfigurationPacket),
    Play(PlayPacket),
    ManualPlay(ManualPlayPacket),
}

impl ClientboundPacket {
    /// The connection state this packet belongs to, whichever way it is encoded.
    pub fn state_name(&self) -> &'static str {
        match self {
            Self::Status(_) => "Status",
            Self::Login(_) => "Login",
            Self::Configuration(_) | Self::ManualConfiguration(_) => "Configuration",
            Self::Play(_) | Self::ManualPlay(_) => "Play",
        }
    }
}

impl PacketName for ClientboundPacket {
    fn packet_name(&self) -> &'static str {
        match self {
            Self::Status(packet) => packet.packet_name(),
            Self::Login(packet) => packet.packet_name(),
            Self::Configuration(packet) => packet.packet_name(),
            Self::ManualConfiguration(packet) => packet.packet_name(),
            Self::Play(packet) => packet.packet_name(),
            Self::ManualPlay(packet) => packet.packet_name(),
        }
    }
}

impl Encode for ClientboundPacket {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Status(packet) => packet.encode(buf),
            Self::Login(packet) => packet.encode(buf),
            Self::Configuration(packet) => packet.encode(buf),
            Self::ManualConfiguration(packet) => packet.encode(buf),
            Self::Play(packet) => packet.encode(buf),
            Self::ManualPlay(packet) => packet.encode(buf),
        }
    }
}

macro_rules! manual_into_clientbound {
    ($wrap:ident, $inner:ident { $($variant:ident),* $(,)? }) => {
        impl From<$inner> for ClientboundPacket {
            fn from(packet: $inner) -> Self {
                ClientboundPacket::$wrap(packet)
            }
        }
        $(
            impl From<$variant> for ClientboundPacket {
                fn from(packet: $variant) -> Self {
                    ClientboundPacket::$wrap($inner::$variant(packet))
                }
            }
        )*
    };
}

manual_into_clientbound!(
    ManualConfiguration,
    ManualConfigurationPacket { UpdateTags }
);
manual_into_clientbound!(
    ManualPlay,
    ManualPlayPacket {
        PlayerInfoUpdate,
        PlayerInfoRemove,
        RemoveEntities,
        ChunkDataAndLight,
        Commands,
        CommandSuggestionsResponse,
        SetPassengers,
        UpdateAdvancements,
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packet_is_named_by_its_state_and_its_variant() {
        let tagged: ClientboundPacket = KeepAlive { keep_alive_id: 7 }.into();
        assert_eq!(
            (tagged.state_name(), tagged.packet_name()),
            ("Play", "KeepAlive")
        );

        let manual: ClientboundPacket = RemoveEntities { entity_ids: vec![] }.into();
        assert_eq!(
            (manual.state_name(), manual.packet_name()),
            ("Play", "RemoveEntities")
        );
    }

    #[test]
    fn encoding_the_wrapper_matches_encoding_the_packet() {
        let packet = KeepAlive { keep_alive_id: 7 };
        let mut alone = Vec::new();
        PlayPacket::KeepAlive(packet.clone()).encode(&mut alone);
        let mut wrapped = Vec::new();
        ClientboundPacket::from(packet).encode(&mut wrapped);
        assert_eq!(alone, wrapped);
    }

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
    }
}
