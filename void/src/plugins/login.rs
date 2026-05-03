use bevy_app::{App, Plugin};
use bevy_ecs::{
    observer::On,
    system::{Commands, Res},
};
use void_protocol::{
    clientbound,
    serverbound::{LoginAcknowledged, LoginStart},
};

use crate::{
    components::{PlayerName, PlayerUuid},
    network::{NetworkChannels, PacketEvent},
};

/// Plugin handling the login state of the Minecraft protocol, where clients can authenticate and join the server.
pub struct LoginPlugin;

impl Plugin for LoginPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_login_start);
        app.add_observer(handle_login_acknowledged);
    }
}

fn handle_login_start(
    event: On<PacketEvent<LoginStart>>,
    mut commands: Commands,
    channels: Res<NetworkChannels>,
) {
    commands.entity(event.entity).insert((
        PlayerName(event.packet.name.clone()),
        PlayerUuid(event.packet.uuid),
    ));

    let _ = channels.outgoing.send(crate::network::OutgoingPacket {
        client_id: event.client_id,
        packet: clientbound::ClientboundPacket::Login(clientbound::LoginPacket::LoginSuccess(
            clientbound::LoginSuccess {
                uuid: event.packet.uuid,
                username: event.packet.name.clone(),
                properties: vec![],
            },
        )),
    });
}

fn handle_login_acknowledged(
    event: On<PacketEvent<LoginAcknowledged>>,
    mut commands: Commands,
    channels: Res<NetworkChannels>,
) {
    commands
        .entity(event.entity)
        .insert(crate::components::ConnectionState(
            void_protocol::State::Configuration,
        ));

    let _ = channels.outgoing.send(crate::network::OutgoingPacket {
        client_id: event.client_id,
        packet: clientbound::ClientboundPacket::Configuration(
            clientbound::ConfigurationPacket::KnownPacks(clientbound::KnownPacks {
                known_packs: vec![clientbound::KnownPack {
                    namespace: "minecraft".to_string(),
                    id: "core".to_string(),
                    version: "26.1.2".to_string(),
                }],
            }),
        ),
    });
}
