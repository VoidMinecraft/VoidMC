use bevy_ecs::prelude::*;
use void_protocol::serverbound;

use crate::components::{ConnectionState, ProtocolVersion};
use crate::events::HandshakePacketEvent;

pub fn handle_handshake(mut events: MessageReader<HandshakePacketEvent>, mut commands: Commands) {
    for event in events.read() {
        match &event.packet {
            serverbound::HandshakePacket::Handshake(handshake) => {
                commands.entity(event.entity).insert((
                    ProtocolVersion(handshake.protocol_version),
                    ConnectionState(handshake.next_state),
                ));
            }
        }
    }
}
