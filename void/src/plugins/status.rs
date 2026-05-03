use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Res};
use voidmc_protocol::{
    clientbound,
    serverbound::{PingRequest, StatusRequest},
};

use crate::{
    ServerConfigResource,
    network::{NetworkChannels, OutgoingPacket, PacketEvent},
};

/// Protocol version this server speaks (Minecraft 26.1.2).
const PROTOCOL_VERSION: i32 = 773;
const PROTOCOL_NAME: &str = "26.1.2";

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

        app.add_observer(
            |event: On<PacketEvent<StatusRequest>>,
             channels: Res<NetworkChannels>,
             config: Res<ServerConfigResource>| {
                let max_players = config.max_players;
                let motd = config.motd.clone();

                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Status(
                        clientbound::StatusPacket::StatusResponse(clientbound::StatusResponse {
                            status: clientbound::Status {
                                version: clientbound::Version {
                                    name: PROTOCOL_NAME.to_string(),
                                    protocol: PROTOCOL_VERSION,
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
