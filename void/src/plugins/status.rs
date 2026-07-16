use bevy_app::{App, Plugin};
use bevy_ecs::{observer::On, system::Res};
use voidmc_protocol::{
    MINECRAFT_VERSION, PROTOCOL_VERSION, clientbound,
    serverbound::{PingRequest, StatusRequest},
};

use crate::{
    ServerConfigResource,
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
                            status: server_status(max_players, motd),
                        }),
                    ),
                });
            },
        );
    }
}

fn server_status(max_players: i32, motd: String) -> clientbound::Status {
    clientbound::Status {
        version: clientbound::Version {
            name: MINECRAFT_VERSION.to_string(),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_advertises_minecraft_26_1_2_protocol() {
        let status = server_status(100, "VoidMC".to_string());

        assert_eq!(status.version.name, "26.1.2");
        assert_eq!(status.version.protocol, 775);
        assert_eq!(status.players.max, 100);
        assert_eq!(status.description.text, "VoidMC");
    }
}
