use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Commands};
use voidmc_protocol::{
    MINECRAFT_VERSION, clientbound,
    serverbound::{LoginAcknowledged, LoginStart},
};

use crate::{
    components::{PlayerName, PlayerUuid},
    network::PacketEvent,
    players::Players,
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
    players: Players,
) {
    commands.entity(event.entity).insert((
        PlayerName(event.packet.name.clone()),
        PlayerUuid(event.packet.uuid),
    ));

    players.send(
        event.entity,
        clientbound::LoginSuccess {
            uuid: event.packet.uuid,
            username: event.packet.name.clone(),
            properties: vec![],
        },
    );
}

fn handle_login_acknowledged(
    event: On<PacketEvent<LoginAcknowledged>>,
    mut commands: Commands,
    players: Players,
) {
    commands
        .entity(event.entity)
        .insert(crate::components::ConnectionState(
            voidmc_protocol::State::Configuration,
        ));

    players.send(
        event.entity,
        clientbound::KnownPacks {
            known_packs: vec![clientbound::KnownPack {
                namespace: "minecraft".to_string(),
                id: "core".to_string(),
                version: MINECRAFT_VERSION.to_string(),
            }],
        },
    );
}
