use bevy_ecs::prelude::*;
use void_protocol::{clientbound, serverbound};

use crate::components::ProtocolVersion;
use crate::config::ServerConfigResource;
use crate::network::{NetworkChannels, OutgoingPacket};

pub fn handle_status_packet(
    world: &mut World,
    client_id: u32,
    entity: Entity,
    packet: serverbound::StatusPacket,
) {
    let sender = world.resource::<NetworkChannels>().outgoing.clone();

    match &packet {
        serverbound::StatusPacket::StatusRequest(_) => {
            let protocol = world
                .get::<ProtocolVersion>(entity)
                .map(|v| v.0)
                .unwrap_or(0);
            let config = world.resource::<ServerConfigResource>();
            let max_players = config.max_players;
            let motd = config.motd.clone();

            let _ = sender.send(OutgoingPacket {
                client_id,
                packet: clientbound::ClientboundPacket::Status(
                    clientbound::StatusPacket::StatusResponse(clientbound::StatusResponse {
                        status: clientbound::Status {
                            version: clientbound::Version {
                                name: "Void Server".to_string(),
                                protocol,
                            },
                            players: clientbound::Players {
                                max: max_players,
                                online: 0,
                                sample: vec![],
                            },
                            description: clientbound::Description { text: motd },
                            favicon: "".to_string(),
                            enforces_secure_chat: false,
                        },
                    }),
                ),
            });
        }
        serverbound::StatusPacket::PingRequest(ping) => {
            let _ = sender.send(OutgoingPacket {
                client_id,
                packet: clientbound::ClientboundPacket::Status(
                    clientbound::StatusPacket::PingResponse(clientbound::PingResponse {
                        timestamp: ping.timestamp,
                    }),
                ),
            });
        }
    }
}
