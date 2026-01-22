use bevy_app::{App, PostUpdate, PreUpdate, ScheduleRunnerPlugin, TaskPoolPlugin, Update};
use bevy_ecs::prelude::*;
use crossbeam_channel::{Receiver, Sender};
use std::time::Duration;
use tracing_subscriber::prelude::*;
use void::{IncomingPacket, OutgoingPacket, Server};
use void_protocol::{
    State,
    serverbound::{HandshakePacket, ServerboundPacket, StatusPacket},
};

#[derive(Resource)]
pub struct NetworkChannels {
    pub incoming: Receiver<IncomingPacket>,
    pub outgoing: Sender<OutgoingPacket>,
}

#[derive(Message, Debug)]
pub struct Handshake {
    pub client_id: u32,
    pub protocol_version: i32,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: State,
}

#[derive(Message, Debug)]
pub struct StatusRequest {
    pub client_id: u32,
}

pub fn ingest_network_packets(network_channels: Res<NetworkChannels>, mut commands: Commands) {
    // Read incoming packets
    while let Ok(incoming_packet) = network_channels.incoming.try_recv() {
        tracing::debug!(
            "Received packet from client {}: {:?}",
            incoming_packet.client_id,
            incoming_packet.packet
        );

        // TODO: Extract this into a method in another crate that does the Protocol -> ECS message conversion
        match incoming_packet.packet {
            ServerboundPacket::Handshake(packet) => match packet {
                HandshakePacket::Handshake(packet) => {
                    tracing::info!("Received handshake packet: {:?}", packet);
                    commands.queue(move |w: &mut World| {
                        w.write_message(Handshake {
                            client_id: incoming_packet.client_id,
                            protocol_version: packet.protocol_version,
                            server_address: packet.server_address,
                            server_port: packet.server_port,
                            next_state: packet.next_state,
                        });
                    })
                }
            },
            ServerboundPacket::Status(packet) => match packet {
                StatusPacket::StatusRequest(packet) => {
                    tracing::info!("Received status request packet: {:?}", packet);
                    commands.queue(move |w: &mut World| {
                        w.write_message(StatusRequest {
                            client_id: incoming_packet.client_id,
                        });
                    })
                }
                _ => { /* Handle other status packets */ }
            },
            _ => { /* Handle other packet types */ }
        }
    }
}

pub fn handle_handshakes(mut commands: Commands, mut messages: MessageReader<Handshake>) {
    for msg in messages.read() {
        tracing::info!("New handshake received: {:?}", msg);
        // Handle new player connections here
    }
}

// May need to run after handshakes since packet can be sent after handshake directly and ingested in same frame
pub fn handle_status_requests(
    mut commands: Commands,
    mut messages: MessageReader<StatusRequest>,
    mut network_channels: ResMut<NetworkChannels>,
) {
    for msg in messages.read() {
        tracing::info!("New status request received: {:?}", msg);
        // Handle status requests here, e.g., send a status response back
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging()?;

    let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded::<IncomingPacket>();
    let (outgoing_tx, outgoing_rx) = crossbeam_channel::unbounded::<OutgoingPacket>();

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
            server.run(incoming_tx).await;
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
        .add_message::<Handshake>()
        .add_message::<StatusRequest>()
        .add_systems(PreUpdate, ingest_network_packets)
        .add_systems(Update, (handle_handshakes, handle_status_requests))
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
