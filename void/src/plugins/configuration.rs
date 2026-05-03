use std::collections::HashSet;

use bevy_app::{App, Plugin};
use bevy_ecs::{
    observer::On,
    system::{Commands, Query, Res, ResMut},
};
use void_protocol::{
    State, clientbound,
    serverbound::{ClientInformation, FinishConfigurationAcknowledged, KnownPacks, PluginMessage},
};

use crate::{
    CommandRegistry, RegistryDataStore, ServerConfigResource, WorldGen,
    components::{
        ClientSettings, ConnectionState, CurrentChunkPos, EffectiveViewDistance, EntityIdCounter,
        KeepAliveState, LoadedChunks, MinecraftEntityId, PlayerDimension, Position,
        PreviousPosition, Rotation, TeleportState,
    },
    events::PlayerJoinEvent,
    network::{NetworkChannels, OutgoingPacket, PacketEvent},
    world::{ChunkData, ChunkIndex, ChunkPos, ChunkPosition, DimensionId},
};

/// Plugin handling the configuration phase of the Minecraft protocol.
pub struct ConfigurationPlugin;

impl Plugin for ConfigurationPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_client_information);
        app.add_observer(handle_plugin_message);
        app.add_observer(handle_known_packs);
        app.add_observer(handle_finish_configuration);
    }
}

fn handle_client_information(event: On<PacketEvent<ClientInformation>>, mut commands: Commands) {
    commands.entity(event.entity).insert(ClientSettings {
        locale: event.packet.locale.clone(),
        view_distance: event.packet.view_distance,
    });
}

fn handle_plugin_message(_event: On<PacketEvent<PluginMessage>>) {}

fn handle_known_packs(
    event: On<PacketEvent<KnownPacks>>,
    channels: Res<NetworkChannels>,
    registries: Res<RegistryDataStore>,
) {
    for registry in &registries.registries {
        let _ = channels.outgoing.send(crate::network::OutgoingPacket {
            client_id: event.client_id,
            packet: clientbound::ClientboundPacket::Configuration(
                clientbound::ConfigurationPacket::RegistryData(registry.clone()),
            ),
        });
    }

    let _ = channels.outgoing.send(crate::network::OutgoingPacket {
        client_id: event.client_id,
        packet: clientbound::ClientboundPacket::ManualConfiguration(
            clientbound::ManualConfigurationPacket::UpdateTags(build_update_tags(&registries)),
        ),
    });

    let _ = channels.outgoing.send(crate::network::OutgoingPacket {
        client_id: event.client_id,
        packet: clientbound::ClientboundPacket::Configuration(
            clientbound::ConfigurationPacket::FinishConfiguration(
                clientbound::FinishConfiguration {},
            ),
        ),
    });
}

/// Translates `void_data::tagged_registries()` into the wire format. Tag
/// values are mapped to numeric indices using the order of entries in the
/// registry as it was just sent.
fn build_update_tags(registries: &RegistryDataStore) -> clientbound::UpdateTags {
    let version = void_data::Version::V26_1_2;
    let mut out = Vec::new();

    for (registry_id, tags) in void_data::tagged_registries(version) {
        let Some(registry) = registries.get_registry(registry_id) else {
            continue;
        };

        let mut tag_entries = Vec::new();
        for (tag_id, entry_ids) in *tags {
            let mut indices = Vec::with_capacity(entry_ids.len());
            for entry_id in *entry_ids {
                if let Some(idx) = registry
                    .entries
                    .iter()
                    .position(|e| e.entry_id == *entry_id)
                {
                    indices.push(idx as i32);
                }
            }
            tag_entries.push(clientbound::TagEntry {
                tag_id: tag_id.to_string(),
                entries: indices,
            });
        }

        out.push(clientbound::RegistryTags {
            registry_id: registry_id.to_string(),
            tags: tag_entries,
        });
    }

    clientbound::UpdateTags { registries: out }
}

fn handle_finish_configuration(
    event: On<PacketEvent<FinishConfigurationAcknowledged>>,
    mut commands: Commands,
    mut entity_id_counter: ResMut<EntityIdCounter>,
    command_registry: Res<CommandRegistry>,
    config: Res<ServerConfigResource>,
    world_gen: Res<WorldGen>,
    channels: Res<NetworkChannels>,
    chunk_index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
) {
    // Allocate entity ID
    let minecraft_entity_id = entity_id_counter.0;
    entity_id_counter.0 += 1;

    // Determine spawn position
    let spawn_y = config.spawn_y.unwrap_or_else(|| {
        world_gen
            .0
            .surface_height_at(config.spawn_x.floor() as i32, config.spawn_z.floor() as i32)
            as f64
            + 1.0
    });
    let spawn_chunk = ChunkPos::from_block(config.spawn_x, config.spawn_z);

    // Update entity with new components
    commands.entity(event.entity).insert((
        ConnectionState(State::Play),
        MinecraftEntityId(minecraft_entity_id),
        Position {
            x: config.spawn_x,
            y: spawn_y,
            z: config.spawn_z,
        },
        PreviousPosition {
            x: config.spawn_x,
            y: spawn_y,
            z: config.spawn_z,
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
        CurrentChunkPos(spawn_chunk),
        EffectiveViewDistance(config.initial_chunk_radius),
        LoadedChunks(HashSet::new()),
        PlayerDimension(DimensionId::Overworld),
    ));

    // Send login success packet
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: event.client_id,
        packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::Login(
            clientbound::Login {
                entity_id: minecraft_entity_id,
                is_hardcore: config.hardcore,
                dimension_names: vec![
                    "minecraft:overworld".to_string(),
                    "minecraft:the_nether".to_string(),
                    "minecraft:the_end".to_string(),
                ],
                max_players: config.max_players,
                view_distance: config.view_distance,
                simulation_distance: config.simulation_distance,
                reduced_debug_info: false,
                enable_respawn_screen: true,
                do_limited_crafting: false,
                dimension_type: 0,
                dimension_name: "minecraft:overworld".to_string(),
                hashed_seed: 0,
                game_mode: config.game_mode,
                previous_game_mode: -1,
                is_debug: false,
                is_flat: false,
                last_death_location: None,
                portal_cooldown: 0,
                sea_level: 63,
                enforces_secure_chat: false,
            },
        )),
    });

    // Send Commands packet (command tree for tab completion)
    let command_tree = command_registry.build_command_tree();
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: event.client_id,
        packet: clientbound::ClientboundPacket::ManualPlay(
            clientbound::ManualPlayPacket::Commands(command_tree),
        ),
    });

    // Send GameEvent (Start waiting for level chunks)
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: event.client_id,
        packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::GameEvent(
            clientbound::GameEvent {
                event: clientbound::GameEventType::StartWaitingForLevelChunks,
                value: 0.0,
            },
        )),
    });

    // Send SetCenterChunk
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: event.client_id,
        packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SetCenterChunk(
            clientbound::SetCenterChunk {
                chunk_x: spawn_chunk.x,
                chunk_z: spawn_chunk.z,
            },
        )),
    });

    // Send initial chunks around spawn
    let early_teleport_threshold = 9;
    let mut loaded = HashSet::new();
    let mut teleport_sent = false;

    for pos in spawn_chunk.chunks_in_radius(config.initial_chunk_radius) {
        let key = (DimensionId::Overworld, pos);
        let chunk_entity = chunk_index.0.get(&key).copied();

        if let Some(chunk_entity) = chunk_entity {
            if let Ok((chunk_pos, chunk_data)) = chunks.get(chunk_entity) {
                let pkt = chunk_data.to_packet(chunk_pos.0.x, chunk_pos.0.z);
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::ManualPlay(
                        clientbound::ManualPlayPacket::ChunkDataAndLight(pkt),
                    ),
                });
                loaded.insert(pos);
            }
        }

        // After the first 9 chunks (center 3×3), send the teleport
        if !teleport_sent && loaded.len() >= early_teleport_threshold {
            send_teleport(
                &channels.outgoing,
                event.client_id,
                config.spawn_x,
                spawn_y,
                config.spawn_z,
            );
            teleport_sent = true;
        }
    }

    if !teleport_sent {
        send_teleport(
            &channels.outgoing,
            event.client_id,
            config.spawn_x,
            spawn_y,
            config.spawn_z,
        );
    }

    // Update LoadedChunks component with the chunks we've sent
    commands.entity(event.entity).insert(LoadedChunks(loaded));
    commands.trigger(PlayerJoinEvent {
        client_id: event.client_id,
        entity: event.entity,
    });
}

fn send_teleport(sender: &flume::Sender<OutgoingPacket>, client_id: u32, x: f64, y: f64, z: f64) {
    let _ = sender.send(OutgoingPacket {
        client_id,
        packet: clientbound::ClientboundPacket::Play(
            clientbound::PlayPacket::SynchronizePlayerPosition(
                clientbound::SynchronizePlayerPosition {
                    teleport_id: 0,
                    x,
                    y,
                    z,
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
}
