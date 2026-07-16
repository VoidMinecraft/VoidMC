use bevy_app::{App, Plugin};
use bevy_ecs::{
    observer::On,
    prelude::With,
    system::{Query, Res},
};
use voidmc_protocol::{
    MINECRAFT_VERSION, PROTOCOL_VERSION, clientbound,
    serverbound::{PingRequest, StatusRequest},
};

use crate::{
    ServerConfigResource,
    components::PlayerReady,
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
             config: Res<ServerConfigResource>,
             ready_players: Query<(), With<PlayerReady>>| {
                let max_players = config.max_players;
                let motd = config.motd.clone();
                let online_players = player_count(ready_players.iter().count());

                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Status(
                        clientbound::StatusPacket::StatusResponse(clientbound::StatusResponse {
                            status: server_status(max_players, online_players, motd),
                        }),
                    ),
                });
            },
        );
    }
}

fn player_count(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

fn server_status(max_players: i32, online_players: i32, motd: String) -> clientbound::Status {
    clientbound::Status {
        version: clientbound::Version {
            name: MINECRAFT_VERSION.to_string(),
            protocol: PROTOCOL_VERSION,
        },
        players: clientbound::Players {
            max: max_players,
            online: online_players,
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
    use crate::{
        ServerConfig,
        network::{IncomingPacket, NetworkChannels},
    };

    #[test]
    fn status_advertises_minecraft_26_1_2_protocol() {
        let status = server_status(100, 3, "VoidMC".to_string());

        assert_eq!(status.version.name, "26.1.2");
        assert_eq!(status.version.protocol, 775);
        assert_eq!(status.players.max, 100);
        assert_eq!(status.players.online, 3);
        assert_eq!(status.description.text, "VoidMC");
    }

    #[test]
    fn player_count_saturates_at_protocol_limit() {
        assert_eq!(player_count(0), 0);
        assert_eq!(player_count(42), 42);
        assert_eq!(player_count(i32::MAX as usize + 1), i32::MAX);
    }

    #[test]
    fn status_request_counts_ready_players() {
        let mut app = App::new();
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let config = ServerConfig::default();

        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_resource(ServerConfigResource::from(&config))
        .add_plugins(StatusPlugin);

        app.world_mut().spawn(PlayerReady);
        app.world_mut().spawn(PlayerReady);
        app.world_mut().spawn_empty();
        let requester = app.world_mut().spawn_empty().id();
        app.world_mut().trigger(PacketEvent {
            client_id: 7,
            entity: requester,
            packet: StatusRequest {},
        });

        let outgoing = outgoing_rx.recv().expect("status response");
        let clientbound::ClientboundPacket::Status(clientbound::StatusPacket::StatusResponse(
            response,
        )) = outgoing.packet
        else {
            panic!("expected status response");
        };

        assert_eq!(outgoing.client_id, 7);
        assert_eq!(response.status.players.online, 2);
    }
}
