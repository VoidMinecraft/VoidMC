use bevy_ecs::prelude::*;
use void_protocol::serverbound;

use crate::components::{ConnectionState, ProtocolVersion};

pub fn handle_handshake_packet(
    world: &mut World,
    _client_id: u32,
    entity: Entity,
    packet: serverbound::HandshakePacket,
) {
    match &packet {
        serverbound::HandshakePacket::Handshake(handshake) => {
            world.entity_mut(entity).insert((
                ProtocolVersion(handshake.protocol_version),
                ConnectionState(handshake.next_state),
            ));
        }
    }
}
