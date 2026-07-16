use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::{
    observer::On,
    prelude::With,
    system::{Query, Res},
};
use voidmc_protocol::{
    clientbound,
    serverbound::{PingRequest, StatusRequest},
};

use crate::{
    ServerConfigResource,
    components::PlayerReady,
    network::{NetworkChannels, OutgoingPacket, PacketEvent},
    server_status::{ServerStatusSnapshot, player_count, server_status},
};

/// Plugin handling the status state of the Minecraft protocol, where clients can query server information without fully logging in.
pub struct StatusPlugin;

impl Plugin for StatusPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, update_status_snapshot);

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

fn update_status_snapshot(
    snapshot: Option<Res<ServerStatusSnapshot>>,
    config: Option<Res<ServerConfigResource>>,
    ready_players: Query<(), With<PlayerReady>>,
) {
    if let (Some(snapshot), Some(config)) = (snapshot, config) {
        snapshot.update(
            config.max_players,
            ready_players.iter().count(),
            &config.motd,
        );
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

    #[test]
    fn status_snapshot_tracks_config_and_ready_players() {
        let mut app = App::new();
        let mut config = ServerConfig::default();
        let snapshot = ServerStatusSnapshot::new(&config);

        app.insert_resource(ServerConfigResource::from(&config))
            .insert_resource(snapshot.clone())
            .add_plugins(StatusPlugin);
        app.world_mut().spawn(PlayerReady);
        app.world_mut().spawn(PlayerReady);

        config.max_players = 40;
        config.motd = "Updated".to_string();
        {
            let mut resource = app.world_mut().resource_mut::<ServerConfigResource>();
            resource.max_players = config.max_players;
            resource.motd.clone_from(&config.motd);
        }
        app.update();

        let response = snapshot.response();
        assert_eq!(response.status.players.max, 40);
        assert_eq!(response.status.players.online, 2);
        assert_eq!(response.status.description.text, "Updated");
    }
}
