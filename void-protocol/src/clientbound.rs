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
    }
);

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
    }
}
