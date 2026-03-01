use bevy_ecs::prelude::*;
use void_protocol::{clientbound, serverbound};

use crate::components::{
    ClientSettings, ConnectionState, EntityIdCounter, KeepAliveState, MinecraftEntityId, Position,
    PreviousPosition, Rotation, TeleportState,
};
use crate::events::ConfigurationPacketEvent;
use crate::network::{NetworkChannels, OutgoingPacket};

#[derive(Resource)]
pub struct RegistryDataStore {
    pub registries: Vec<clientbound::RegistryData>,
}

pub fn handle_configuration(
    mut events: MessageReader<ConfigurationPacketEvent>,
    channels: Res<NetworkChannels>,
    registry_store: Res<RegistryDataStore>,
    mut commands: Commands,
    mut entity_id_counter: ResMut<EntityIdCounter>,
) {
    for event in events.read() {
        match &event.packet {
            serverbound::ConfigurationPacket::ClientInformation(info) => {
                tracing::debug!(
                    "Client {} settings: locale={}, view_distance={}",
                    event.client_id,
                    info.locale,
                    info.view_distance
                );

                commands.entity(event.entity).insert(ClientSettings {
                    locale: info.locale.clone(),
                    view_distance: info.view_distance,
                });
            }
            serverbound::ConfigurationPacket::PluginMessage(msg) => {
                tracing::debug!(
                    "Client {} plugin message: channel={}",
                    event.client_id,
                    msg.channel
                );
            }
            serverbound::ConfigurationPacket::KnownPacks(_) => {
                // Send all registry data entries
                for registry in &registry_store.registries {
                    let _ = channels.outgoing.send(OutgoingPacket {
                        client_id: event.client_id,
                        packet: clientbound::ClientboundPacket::Configuration(
                            clientbound::ConfigurationPacket::RegistryData(registry.clone()),
                        ),
                    });
                }

                // Send FinishConfiguration
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Configuration(
                        clientbound::ConfigurationPacket::FinishConfiguration(
                            clientbound::FinishConfiguration {},
                        ),
                    ),
                });
            }
            serverbound::ConfigurationPacket::FinishConfigurationAcknowledged(_) => {
                tracing::info!(
                    "Client {} finished configuration, transitioning to Play",
                    event.client_id
                );

                let minecraft_entity_id = entity_id_counter.0;
                entity_id_counter.0 += 1;

                let spawn_x = 0.0_f64;
                let spawn_y = 64.0_f64;
                let spawn_z = 0.0_f64;

                commands.entity(event.entity).insert((
                    ConnectionState(void_protocol::State::Play),
                    MinecraftEntityId(minecraft_entity_id),
                    Position {
                        x: spawn_x,
                        y: spawn_y,
                        z: spawn_z,
                    },
                    PreviousPosition {
                        x: spawn_x,
                        y: spawn_y,
                        z: spawn_z,
                    },
                    Rotation {
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                    TeleportState {
                        next_id: 1,
                        pending_id: Some(0),
                    },
                    KeepAliveState {
                        last_sent_id: 0,
                        awaiting_response: false,
                    },
                ));

                // Send Play Login packet
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Play(
                        clientbound::PlayPacket::Login(clientbound::Login {
                            entity_id: minecraft_entity_id,
                            is_hardcore: false,
                            dimension_names: vec![
                                "minecraft:overworld".to_string(),
                                "minecraft:the_nether".to_string(),
                                "minecraft:the_end".to_string(),
                            ],
                            max_players: 100,
                            view_distance: 10,
                            simulation_distance: 10,
                            reduced_debug_info: false,
                            enable_respawn_screen: true,
                            do_limited_crafting: false,
                            dimension_type: 0,
                            dimension_name: "minecraft:overworld".to_string(),
                            hashed_seed: 0,
                            game_mode: 1, // Creative
                            previous_game_mode: -1,
                            is_debug: false,
                            is_flat: true,
                            last_death_location: None,
                            portal_cooldown: 0,
                            sea_level: 63,
                            enforces_secure_chat: false,
                        }),
                    ),
                });

                // Send SynchronizePlayerPosition
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Play(
                        clientbound::PlayPacket::SynchronizePlayerPosition(
                            clientbound::SynchronizePlayerPosition {
                                teleport_id: 0,
                                x: spawn_x,
                                y: spawn_y,
                                z: spawn_z,
                                vx: 0.0,
                                vy: 0.0,
                                vz: 0.0,
                                yaw: 0.0,
                                pitch: 0.0,
                                flags: clientbound::TeleportFlags::empty(),
                            },
                        ),
                    ),
                });

                // Send GameEvent(StartWaitingForLevelChunks)
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Play(
                        clientbound::PlayPacket::GameEvent(clientbound::GameEvent {
                            event: clientbound::GameEventType::StartWaitingForLevelChunks,
                            value: 0.0,
                        }),
                    ),
                });
            }
        }
    }
}
