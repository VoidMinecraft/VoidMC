use bevy_app::{App, PreUpdate, ScheduleRunnerPlugin, TaskPoolPlugin};
use bevy_ecs::{prelude::*, system::SystemState};
use flume::{Receiver, Sender};
use std::{collections::HashMap, time::Duration};
use tracing_subscriber::prelude::*;
use void::{IncomingPacket, OutgoingPacket, Server};
use void_protocol::{
    State,
    clientbound::{self, Description, Status},
    serverbound,
};

#[derive(Resource)]
pub struct NetworkChannels {
    pub incoming: Receiver<IncomingPacket>,
    pub outgoing: Sender<OutgoingPacket>,
}

#[derive(Clone, Copy)]
pub struct Player {
    entity: Entity,
    state: State,
    protocol_version: i32,
}

#[derive(Resource)]
pub struct InstanceData {
    pub players: HashMap<u32, Player>,
}

pub fn ingest_network_packets(world: &mut World) {
    loop {
        let Some(incoming_packet) =
            world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
                channels.incoming.try_recv().ok()
            })
        else {
            break;
        };

        let player = world.resource_scope(|world, mut instance_data: Mut<InstanceData>| {
            let player = instance_data
                .players
                .entry(incoming_packet.client_id)
                .or_insert_with(|| Player {
                    entity: world.spawn_empty().id(),
                    state: State::Handshake,
                    protocol_version: 0,
                });

            *player
        });

        match player.state {
            State::Handshake => match incoming_packet
                .packet
                .decode::<serverbound::HandshakePacket>()
            {
                Ok(packet) => match packet {
                    serverbound::HandshakePacket::Handshake(handshake) => {
                        world.resource_scope(|_world, mut instance_data: Mut<InstanceData>| {
                            let player = instance_data
                                .players
                                .get_mut(&incoming_packet.client_id)
                                .expect("Player must exist");

                            player.state = handshake.next_state;
                            player.protocol_version = handshake.protocol_version;
                        });
                    }
                },
                Err(_) => continue,
            },
            State::Status => match incoming_packet.packet.decode::<serverbound::StatusPacket>() {
                Ok(packet) => match packet {
                    serverbound::StatusPacket::StatusRequest(_) => {
                        world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
                            channels
                                .outgoing
                                .send(OutgoingPacket {
                                    client_id: incoming_packet.client_id,
                                    packet: clientbound::ClientboundPacket::Status(
                                        clientbound::StatusPacket::StatusResponse(
                                            clientbound::StatusResponse {
                                                status: Status {
                                                    version: clientbound::Version {
                                                        name: "Void Server".to_string(),
                                                        protocol: player.protocol_version,
                                                    },
                                                    players: clientbound::Players {
                                                        max: 100,
                                                        online: 0,
                                                        sample: vec![],
                                                    },
                                                    description: Description {
                                                        text: "Welcome to Void Server!".to_string(),
                                                    },
                                                    favicon: "".to_string(),
                                                    enforces_secure_chat: false,
                                                },
                                            },
                                        ),
                                    ),
                                })
                                .expect("Failed to send pong packet");
                        });
                    }
                    serverbound::StatusPacket::PingRequest(ping) => {
                        world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
                            channels
                                .outgoing
                                .send(OutgoingPacket {
                                    client_id: incoming_packet.client_id,
                                    packet: clientbound::ClientboundPacket::Status(
                                        clientbound::StatusPacket::PingResponse(
                                            clientbound::PingResponse {
                                                timestamp: ping.timestamp,
                                            },
                                        ),
                                    ),
                                })
                                .expect("Failed to send pong packet");
                        });
                    }
                },
                Err(_) => continue,
            },
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging()?;

    let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
    let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();

    // Start the server in a separate thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            let server = Server::new("127.0.0.1:25565")
                .await
                .expect("Failed to start server");
            server.run(incoming_tx, outgoing_rx).await;
        })
    });

    App::new()
        .add_plugins((
            TaskPoolPlugin::default(),
            ScheduleRunnerPlugin::run_loop(Duration::from_millis(1000 / 20)),
        ))
        // TODO: extract into a plugin
        .insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_resource(InstanceData {
            players: HashMap::new(),
        })
        .add_systems(PreUpdate, ingest_network_packets)
        .run();

    Ok(())
}

fn setup_logging() -> Result<(), Box<dyn std::error::Error>> {
    // Create logs directory if it doesn't exist
    std::fs::create_dir_all("logs")?;

    let file_appender = tracing_appender::rolling::daily("logs", "void.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_level(true)
        .with_file(true)
        .pretty()
        .with_writer(std::io::stderr);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_level(true)
        .with_file(true)
        .with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .with(console_layer)
        .with(file_layer)
        .init();

    Ok(())
}
