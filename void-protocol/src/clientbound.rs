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

macro_rules! into_clientbound {
    ($($state:ident :: $inner:ident :: $variant:ident),* $(,)?) => {
        $(
            impl From<$variant> for ClientboundPacket {
                fn from(packet: $variant) -> Self {
                    ClientboundPacket::$state($inner::$variant(packet))
                }
            }
        )*
    };
}

impl From<StatusPacket> for ClientboundPacket {
    fn from(packet: StatusPacket) -> Self {
        ClientboundPacket::Status(packet)
    }
}

impl From<LoginPacket> for ClientboundPacket {
    fn from(packet: LoginPacket) -> Self {
        ClientboundPacket::Login(packet)
    }
}

impl From<ConfigurationPacket> for ClientboundPacket {
    fn from(packet: ConfigurationPacket) -> Self {
        ClientboundPacket::Configuration(packet)
    }
}

impl From<ManualConfigurationPacket> for ClientboundPacket {
    fn from(packet: ManualConfigurationPacket) -> Self {
        ClientboundPacket::ManualConfiguration(packet)
    }
}

impl From<PlayPacket> for ClientboundPacket {
    fn from(packet: PlayPacket) -> Self {
        ClientboundPacket::Play(packet)
    }
}

impl From<ManualPlayPacket> for ClientboundPacket {
    fn from(packet: ManualPlayPacket) -> Self {
        ClientboundPacket::ManualPlay(packet)
    }
}

into_clientbound! {
    Status::StatusPacket::StatusResponse,
    Status::StatusPacket::PingResponse,
    Login::LoginPacket::LoginSuccess,
    Configuration::ConfigurationPacket::FinishConfiguration,
    Configuration::ConfigurationPacket::RegistryData,
    Configuration::ConfigurationPacket::KnownPacks,
    ManualConfiguration::ManualConfigurationPacket::UpdateTags,
    Play::PlayPacket::SpawnEntity,
    Play::PlayPacket::BlockChangedAck,
    Play::PlayPacket::BlockUpdate,
    Play::PlayPacket::SetContainerContent,
    Play::PlayPacket::SetContainerSlot,
    Play::PlayPacket::Disconnect,
    Play::PlayPacket::UnloadChunk,
    Play::PlayPacket::GameEvent,
    Play::PlayPacket::KeepAlive,
    Play::PlayPacket::Login,
    Play::PlayPacket::UpdateEntityPosition,
    Play::PlayPacket::UpdateEntityPositionAndRotation,
    Play::PlayPacket::UpdateEntityRotation,
    Play::PlayPacket::Ping,
    Play::PlayPacket::SynchronizePlayerPosition,
    Play::PlayPacket::SetHeadRotation,
    Play::PlayPacket::SetEntityMotion,
    Play::PlayPacket::SetCenterChunk,
    Play::PlayPacket::SetCursorItem,
    Play::PlayPacket::SetEntityData,
    Play::PlayPacket::SetHeldSlot,
    Play::PlayPacket::SystemChat,
    Play::PlayPacket::TeleportEntity,
    ManualPlay::ManualPlayPacket::PlayerInfoUpdate,
    ManualPlay::ManualPlayPacket::PlayerInfoRemove,
    ManualPlay::ManualPlayPacket::RemoveEntities,
    ManualPlay::ManualPlayPacket::ChunkDataAndLight,
    ManualPlay::ManualPlayPacket::Commands,
    ManualPlay::ManualPlayPacket::CommandSuggestionsResponse,
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
    }
}
