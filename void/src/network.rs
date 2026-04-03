use std::collections::HashMap;

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::*;
use flume::{Receiver, Sender};
use void_net::socket::Packet;
use void_protocol::serverbound;

use crate::components::{Client, ClientId, ConnectionState, PlayerReady};
use crate::events::PlayerQuitEvent;

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
    kick_tx: Sender<u32>,
}

impl NetworkPlugin {
    pub fn new(
        incoming_rx: Receiver<IncomingPacket>,
        outgoing_tx: Sender<OutgoingPacket>,
        disconnect_rx: Receiver<u32>,
        kick_tx: Sender<u32>,
    ) -> Self {
        Self {
            incoming_rx,
            outgoing_tx,
            disconnect_rx,
            kick_tx,
        }
    }
}

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NetworkChannels {
            incoming: self.incoming_rx.clone(),
            outgoing: self.outgoing_tx.clone(),
            disconnect: self.disconnect_rx.clone(),
            kick: self.kick_tx.clone(),
        })
        .insert_resource(ClientToEntityMap(HashMap::new()))
        .add_systems(PreUpdate, ingest_network_packets);
    }
}

#[derive(Resource)]
pub struct NetworkChannels {
    pub incoming: Receiver<IncomingPacket>,
    pub outgoing: Sender<OutgoingPacket>,
    pub disconnect: Receiver<u32>,
    pub kick: Sender<u32>,
}

#[derive(Resource)]
pub struct ClientToEntityMap(pub HashMap<u32, Entity>);

pub fn ingest_network_packets(world: &mut World) {
    // TODO: This batch-draining approach is simple but may lead to increased latency under high load.
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

    // Drain disconnect channel and handle disconnects
    let disconnected: Vec<u32> = world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
        let mut disc = Vec::new();
        while let Ok(client_id) = channels.disconnect.try_recv() {
            disc.push(client_id);
        }
        disc
    });

    for disc_client_id in disconnected {
        let entity = {
            let mut map = world.resource_mut::<ClientToEntityMap>();
            match map.0.remove(&disc_client_id) {
                Some(e) => e,
                None => continue,
            }
        };

        // Trigger quit event (observer will broadcast to other players)
        let is_ready = world.get::<PlayerReady>(entity).is_some();
        if is_ready {
            world.trigger(PlayerQuitEvent {
                client_id: disc_client_id,
                entity,
            });
            world.flush();
        }

        world.despawn(entity);
    }
}

#[derive(Debug, Event)]
pub struct PacketEvent<T> {
    pub client_id: u32,
    pub entity: Entity,
    pub packet: T,
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
            let packet = packet.decode::<serverbound::HandshakePacket>()?;
            tracing::debug!("[Client {}][Handshake] Received {:?}", client_id, packet);

            match packet {
                serverbound::HandshakePacket::Handshake(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
            }
        }
        void_protocol::State::Status => {
            let packet = packet.decode::<serverbound::StatusPacket>()?;
            tracing::debug!("[Client {}][Status] Received {:?}", client_id, packet);

            match packet {
                serverbound::StatusPacket::PingRequest(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::StatusPacket::StatusRequest(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
            }
        }
        void_protocol::State::Login => {
            let packet = packet.decode::<serverbound::LoginPacket>()?;
            tracing::debug!("[Client {}][Login] Received {:?}", client_id, packet);

            match packet {
                serverbound::LoginPacket::LoginStart(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::LoginPacket::LoginAcknowledged(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
            }
        }
        void_protocol::State::Configuration => {
            let packet = packet.decode::<serverbound::ConfigurationPacket>()?;
            tracing::debug!(
                "[Client {}][Configuration] Received {:?}",
                client_id,
                packet
            );

            match packet {
                serverbound::ConfigurationPacket::ClientInformation(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::ConfigurationPacket::PluginMessage(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::ConfigurationPacket::KnownPacks(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::ConfigurationPacket::FinishConfigurationAcknowledged(packet) => world
                    .trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    }),
            }
        }
        void_protocol::State::Play => {
            let packet = packet.decode::<serverbound::PlayPacket>()?;
            tracing::debug!("[Client {}][Play] Received {:?}", client_id, packet);

            match packet {
                serverbound::PlayPacket::ChatCommand(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::ChatMessage(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::ChatCommandUnsigned(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::PlayPacket::ClientInformation(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::CloseContainer(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::CommandSuggestionsRequest(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::PlayPacket::ConfirmTeleportation(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::PlayPacket::Interact(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::KeepAlive(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::PlayerAbilities(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::PlayerAction(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::PlayerCommand(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::PlayerLoaded(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::Pong(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SetHeldItem(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SetPlayerPos(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SetPlayerPosAndRot(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SetPlayerRotation(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SignedChatCommand(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SwingArm(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::TickEnd(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::UseItem(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::UseItemOn(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
            }
        }
        _ => {
            tracing::warn!("Unhandled protocol state: {:?}", state.0);
        }
    }

    // Ensure all events are processed and any resulting state changes are applied before processing the next packet
    world.flush();

    Ok(())
}
