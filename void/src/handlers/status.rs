use bevy_ecs::prelude::*;
use void_protocol::{clientbound, serverbound};

use crate::components::ProtocolVersion;
use crate::events::StatusPacketEvent;
use crate::network::{NetworkChannels, OutgoingPacket};

pub fn handle_status(
    mut events: MessageReader<StatusPacketEvent>,
    channels: Res<NetworkChannels>,
    query: Query<&ProtocolVersion>,
) {
    for event in events.read() {
        match &event.packet {
            serverbound::StatusPacket::StatusRequest(_) => {
                let protocol = query
                    .get(event.entity)
                    .map(|v| v.0)
                    .unwrap_or(0);

                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Status(
                        clientbound::StatusPacket::StatusResponse(clientbound::StatusResponse {
                            status: clientbound::Status {
                                version: clientbound::Version {
                                    name: "Void Server".to_string(),
                                    protocol,
                                },
                                players: clientbound::Players {
                                    max: 100,
                                    online: 0,
                                    sample: vec![],
                                },
                                description: clientbound::Description {
                                    text: "Welcome to Void Server!".to_string(),
                                },
                                favicon: "".to_string(),
                                enforces_secure_chat: false,
                            },
                        }),
                    ),
                });
            }
            serverbound::StatusPacket::PingRequest(ping) => {
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Status(
                        clientbound::StatusPacket::PingResponse(clientbound::PingResponse {
                            timestamp: ping.timestamp,
                        }),
                    ),
                });
            }
        }
    }
}
