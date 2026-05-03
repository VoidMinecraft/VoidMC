use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Commands};
use voidmc_protocol::serverbound::Handshake;

use crate::{
    components::{ConnectionState, ProtocolVersion},
    network::PacketEvent,
};

/// Plugin handling the initial handshake phase of the Minecraft protocol, where clients identify themselves and select a protocol version.
pub struct HandshakePlugin;

impl Plugin for HandshakePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(
            |event: On<PacketEvent<Handshake>>, mut commands: Commands| {
                commands.entity(event.entity).insert((
                    ProtocolVersion(event.packet.protocol_version),
                    ConnectionState(event.packet.next_state),
                ));
            },
        );
    }
}
