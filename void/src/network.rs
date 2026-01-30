use std::collections::HashMap;

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::*;
use flume::{Receiver, Sender};
use void_net::socket::Packet;
use void_protocol::{clientbound, serverbound};

pub struct IncomingPacket {
    pub client_id: u32, // who sent it
    pub packet: Packet,
}

pub struct OutgoingPacket {
    pub client_id: u32, // to whom to send (should be improved for broadcasting)
    pub packet: void_protocol::clientbound::ClientboundPacket,
}

pub struct NetworkPlugin {
    incoming_rx: Receiver<IncomingPacket>,
    outgoing_tx: Sender<OutgoingPacket>,
}

impl NetworkPlugin {
    pub fn new(incoming_rx: Receiver<IncomingPacket>, outgoing_tx: Sender<OutgoingPacket>) -> Self {
        Self {
            incoming_rx,
            outgoing_tx,
        }
    }
}

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NetworkChannels {
            incoming: self.incoming_rx.clone(),
            outgoing: self.outgoing_tx.clone(),
        })
        .insert_resource(ClientToEntityMap(HashMap::new()))
        .add_systems(PreUpdate, ingest_network_packets);
    }
}

#[derive(Resource)]
pub struct NetworkChannels {
    pub incoming: Receiver<IncomingPacket>,
    pub outgoing: Sender<OutgoingPacket>,
}

#[derive(Resource)]
pub struct ClientToEntityMap(HashMap<u32, Entity>);

#[derive(Component)]
pub struct Client;

#[derive(Component)]
pub struct State(pub void_protocol::State);

#[derive(Component)]
pub struct ProtocolVersion(pub i32);

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

pub fn send_packet(world: &mut World, packet: OutgoingPacket) -> std::io::Result<()> {
    world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
        channels
            .outgoing
            .send(packet)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    })
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

pub fn handle_handshake_packet(
    world: &mut World,
    client_entity: Entity,
    packet: serverbound::HandshakePacket,
) -> std::io::Result<()> {
    match packet {
        serverbound::HandshakePacket::Handshake(handshake) => {
            handle_handshake_packet_handshake(world, client_entity, handshake)
        }
    }
}

pub fn handle_handshake_packet_handshake(
    world: &mut World,
    client_entity: Entity,
    packet: serverbound::Handshake,
) -> std::io::Result<()> {
    // Store the protocol version in a component
    world
        .entity_mut(client_entity)
        .insert(ProtocolVersion(packet.protocol_version));

    // Overwrite the state component to the next state
    world
        .entity_mut(client_entity)
        .insert(State(packet.next_state));

    Ok(())
}

pub fn handle_status_packet(
    world: &mut World,
    client_id: u32,
    client_entity: Entity,
    packet: serverbound::StatusPacket,
) -> std::io::Result<()> {
    match packet {
        serverbound::StatusPacket::StatusRequest(packet) => {
            handle_status_packet_status(world, client_id, client_entity, packet)
        }
        serverbound::StatusPacket::PingRequest(packet) => {
            handle_status_packet_ping(world, client_id, packet)
        }
    }
}

pub fn handle_status_packet_status(
    world: &mut World,
    client_id: u32,
    client_entity: Entity,
    _packet: serverbound::StatusRequest,
) -> std::io::Result<()> {
    send_packet(
        world,
        OutgoingPacket {
            client_id,
            packet: clientbound::ClientboundPacket::Status(
                clientbound::StatusPacket::StatusResponse(clientbound::StatusResponse {
                    status: clientbound::Status {
                        version: clientbound::Version {
                            name: "Void Server".to_string(),
                            protocol: world
                                .get::<ProtocolVersion>(client_entity)
                                .expect(
                                    "Client must have a ProtocolVersion component at this point",
                                )
                                .0,
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
        },
    )
}

pub fn handle_status_packet_ping(
    world: &mut World,
    client_id: u32,
    packet: serverbound::PingRequest,
) -> std::io::Result<()> {
    send_packet(
        world,
        OutgoingPacket {
            client_id,
            packet: clientbound::ClientboundPacket::Status(
                clientbound::StatusPacket::PingResponse(clientbound::PingResponse {
                    timestamp: packet.timestamp,
                }),
            ),
        },
    )
}
