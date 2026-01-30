use bevy_app::{App, PreUpdate, ScheduleRunnerPlugin, TaskPoolPlugin};
use bevy_ecs::prelude::*;
use flume::{Receiver, Sender};
use std::{collections::HashMap, time::Duration};
use tracing_subscriber::prelude::*;
use void::{IncomingPacket, OutgoingPacket, Server};
use void_net::socket::Packet;
use void_protocol::{
    clientbound::{self, Description, Status},
    serverbound,
};

#[derive(Resource)]
pub struct NetworkChannels {
    pub incoming: Receiver<IncomingPacket>,
    pub outgoing: Sender<OutgoingPacket>,
}

#[derive(Component)]
pub struct Client;

#[derive(Component)]
pub struct State(pub void_protocol::State);

#[derive(Component)]
pub struct ProtocolVersion(pub i32);

#[derive(Resource)]
pub struct ClientToEntityMap(HashMap<u32, Entity>);

pub fn send_packet(world: &mut World, packet: OutgoingPacket) -> std::io::Result<()> {
    world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
        channels
            .outgoing
            .send(packet)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    })
}

pub fn handle_handshake_packet(
    world: &mut World,
    client_entity: Entity,
    packet: serverbound::HandshakePacket,
) -> std::io::Result<()> {
    match packet {
        serverbound::HandshakePacket::Handshake(handshake) => {
            // Store the protocol version in a component
            world
                .entity_mut(client_entity)
                .insert(ProtocolVersion(handshake.protocol_version));

            // Overwrite the state component to the next state
            world
                .entity_mut(client_entity)
                .insert(State(handshake.next_state));
        }
    }

    Ok(())
}

pub fn handle_status_packet(
    world: &mut World,
    client_id: u32,
    client_entity: Entity,
    packet: serverbound::StatusPacket,
) -> std::io::Result<()> {
    match packet {
        serverbound::StatusPacket::StatusRequest(_) => send_packet(
            world,
            OutgoingPacket {
                client_id,
                packet: clientbound::ClientboundPacket::Status(
                    clientbound::StatusPacket::StatusResponse(clientbound::StatusResponse {
                        status: Status {
                            version: clientbound::Version {
                                name: "Void Server".to_string(),
                                protocol: world
                                    .get::<ProtocolVersion>(client_entity)
                                    .expect("Client must have a ProtocolVersion component at this point")
                                    .0,
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
                    }),
                ),
            },
        ),
        serverbound::StatusPacket::PingRequest(ping) => send_packet(
            world,
            OutgoingPacket {
                client_id,
                packet: clientbound::ClientboundPacket::Status(
                    clientbound::StatusPacket::PingResponse(clientbound::PingResponse {
                        timestamp: ping.timestamp,
                    }),
                ),
            },
        ),
    }
}

pub fn handle_packet(
    world: &mut World,
    client_id: u32,
    client_entity: Entity,
    packet: Packet,
) -> std::io::Result<()> {
    let state = world
        .get::<State>(client_entity)
        .expect("Client must have a State component");

    match state.0 {
        void_protocol::State::Handshake => handle_handshake_packet(
            world,
            client_entity,
            packet.decode::<serverbound::HandshakePacket>()?,
        ),
        void_protocol::State::Status => handle_status_packet(
            world,
            client_id,
            client_entity,
            packet.decode::<serverbound::StatusPacket>()?,
        ),
        _ => unimplemented!(),
    }
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

        let client_entity =
            world.resource_scope(|world, mut client_to_entity_map: Mut<ClientToEntityMap>| {
                client_to_entity_map
                    .0
                    .entry(incoming_packet.client_id)
                    .or_insert_with(|| {
                        world
                            .spawn((Client, State(void_protocol::State::Handshake)))
                            .id()
                    })
                    .clone()
            });

        if let Err(e) = handle_packet(
            world,
            incoming_packet.client_id,
            client_entity,
            incoming_packet.packet,
        ) {
            tracing::error!(
                "Failed to handle packet from client {}: {}",
                incoming_packet.client_id,
                e
            );
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
        .insert_resource(ClientToEntityMap(HashMap::new()))
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
