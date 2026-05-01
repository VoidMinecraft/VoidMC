use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Res, world::World};
use void_protocol::{
    clientbound,
    serverbound::{PingRequest, StatusRequest},
};

use crate::{
    ServerConfigResource,
    components::ProtocolVersion,
    network::{NetworkChannels, OutgoingPacket, PacketEvent},
};

/// Plugin handling the status state of the Minecraft protocol, where clients can query server information without fully logging in.
pub struct StatusPlugin;

impl Plugin for StatusPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(
            |event: On<PacketEvent<PingRequest>>, channels: Res<NetworkChannels>| {
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Status(
                        clientbound::StatusPacket::PingResponse(clientbound::PingResponse {
                            timestamp: event.packet.timestamp,
                        }),
                    ),
                });
            },
        );

        app.add_observer(|event: On<PacketEvent<StatusRequest>>, world: &World, channels: Res<NetworkChannels>, config: Res<ServerConfigResource>| {
                let protocol_version = world.get::<ProtocolVersion>(event.entity).map(|v| v.0).expect("Client should have a ProtocolVersion component by the time they send a StatusRequest");
                let max_players = config.max_players;
                let motd = config.motd.clone();

                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Status(
                        clientbound::StatusPacket::StatusResponse(clientbound::StatusResponse {
                            status: clientbound::Status {
                                version: clientbound::Version {
                                    name: "Void Server".to_string(),
                                    protocol: protocol_version,
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
            },
        );
    }
}
