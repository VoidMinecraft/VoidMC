use std::collections::HashMap;

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::*;
use flume::{Receiver, Sender};
use void_net::socket::Packet;
use void_protocol::serverbound;

use crate::components::{Client, ClientId, ConnectionState};
use crate::events::{
    ConfigurationPacketEvent, HandshakePacketEvent, LoginPacketEvent, PlayPacketEvent,
    StatusPacketEvent,
};

pub struct IncomingPacket {
    pub client_id: u32,
    pub packet: Packet,
}

pub struct OutgoingPacket {
    pub client_id: u32,
    pub packet: void_protocol::clientbound::ClientboundPacket,
}

pub struct NetworkPlugin {
    incoming_rx: Receiver<IncomingPacket>,
    outgoing_tx: Sender<OutgoingPacket>,
    disconnect_rx: Receiver<u32>,
}

impl NetworkPlugin {
    pub fn new(
        incoming_rx: Receiver<IncomingPacket>,
        outgoing_tx: Sender<OutgoingPacket>,
        disconnect_rx: Receiver<u32>,
    ) -> Self {
        Self {
            incoming_rx,
            outgoing_tx,
            disconnect_rx,
        }
    }
}

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NetworkChannels {
            incoming: self.incoming_rx.clone(),
            outgoing: self.outgoing_tx.clone(),
            disconnect: self.disconnect_rx.clone(),
        })
        .insert_resource(ClientToEntityMap(HashMap::new()))
        .add_message::<HandshakePacketEvent>()
        .add_message::<StatusPacketEvent>()
        .add_message::<LoginPacketEvent>()
        .add_message::<ConfigurationPacketEvent>()
        .add_message::<PlayPacketEvent>()
        .add_systems(PreUpdate, ingest_network_packets);
    }
}

#[derive(Resource)]
pub struct NetworkChannels {
    pub incoming: Receiver<IncomingPacket>,
    pub outgoing: Sender<OutgoingPacket>,
    pub disconnect: Receiver<u32>,
}

#[derive(Resource)]
pub struct ClientToEntityMap(pub HashMap<u32, Entity>);

pub fn ingest_network_packets(world: &mut World) {
    // Batch-drain all packets from channel
    let packets: Vec<IncomingPacket> =
        world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
            let mut packets = Vec::new();
            while let Ok(packet) = channels.incoming.try_recv() {
                packets.push(packet);
            }
            packets
        });

    for incoming_packet in packets {
        let client_entity =
            world.resource_scope(|world, mut client_to_entity_map: Mut<ClientToEntityMap>| {
                client_to_entity_map
                    .0
                    .entry(incoming_packet.client_id)
                    .or_insert_with(|| {
                        world
                            .spawn((
                                Client,
                                ClientId(incoming_packet.client_id),
                                ConnectionState(void_protocol::State::Handshake),
                            ))
                            .id()
                    })
                    .clone()
            });

        if let Err(e) = dispatch_packet(
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

fn dispatch_packet(
    world: &mut World,
    client_id: u32,
    entity: Entity,
    packet: Packet,
) -> std::io::Result<()> {
    let state = world
        .get::<ConnectionState>(entity)
        .expect("Client must have a ConnectionState component");

    match state.0 {
        void_protocol::State::Handshake => {
            let decoded = packet.decode::<serverbound::HandshakePacket>()?;
            // Apply state transition immediately so subsequent packets in the
            // same batch are decoded with the correct state.
            let serverbound::HandshakePacket::Handshake(ref hs) = decoded;
            world
                .entity_mut(entity)
                .insert(ConnectionState(hs.next_state));
            world.write_message(HandshakePacketEvent {
                client_id,
                entity,
                packet: decoded,
            });
        }
        void_protocol::State::Status => {
            let decoded = packet.decode::<serverbound::StatusPacket>()?;
            world.write_message(StatusPacketEvent {
                client_id,
                entity,
                packet: decoded,
            });
        }
        void_protocol::State::Login => {
            let decoded = packet.decode::<serverbound::LoginPacket>()?;
            // LoginAcknowledged transitions to Configuration immediately.
            if matches!(decoded, serverbound::LoginPacket::LoginAcknowledged(_)) {
                world
                    .entity_mut(entity)
                    .insert(ConnectionState(void_protocol::State::Configuration));
            }
            world.write_message(LoginPacketEvent {
                client_id,
                entity,
                packet: decoded,
            });
        }
        void_protocol::State::Configuration => {
            let decoded = packet.decode::<serverbound::ConfigurationPacket>()?;
            // FinishConfigurationAcknowledged transitions to Play immediately.
            if matches!(
                decoded,
                serverbound::ConfigurationPacket::FinishConfigurationAcknowledged(_)
            ) {
                world
                    .entity_mut(entity)
                    .insert(ConnectionState(void_protocol::State::Play));
            }
            world.write_message(ConfigurationPacketEvent {
                client_id,
                entity,
                packet: decoded,
            });
        }
        void_protocol::State::Play => {
            let decoded = packet.decode::<serverbound::PlayPacket>()?;
            world.write_message(PlayPacketEvent {
                client_id,
                entity,
                packet: decoded,
            });
        }
        _ => {
            tracing::warn!("Unhandled protocol state: {:?}", state.0);
        }
    }

    Ok(())
}
