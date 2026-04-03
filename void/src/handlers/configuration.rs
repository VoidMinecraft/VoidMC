use std::collections::HashSet;

use bevy_ecs::prelude::*;
use void_protocol::{clientbound, serverbound};

use crate::commands::CommandRegistry;
use crate::components::{
    ClientSettings, ConnectionState, CurrentChunkPos, EffectiveViewDistance, EntityIdCounter,
    KeepAliveState, LoadedChunks, MinecraftEntityId, PlayerDimension, Position, PreviousPosition,
    Rotation, TeleportState,
};
use crate::config::ServerConfigResource;
use crate::events::PlayerJoinEvent;
use crate::network::{NetworkChannels, OutgoingPacket};
use crate::registry::RegistryDataStore;
use crate::world::generation::WorldGen;
use crate::world::{ChunkData, ChunkIndex, ChunkPos, ChunkPosition, DimensionId};

pub fn handle_configuration_packet(
    world: &mut World,
    client_id: u32,
    entity: Entity,
    packet: serverbound::ConfigurationPacket,
) {
    match &packet {
        serverbound::ConfigurationPacket::ClientInformation(info) => {
            tracing::debug!(
                "Client {} settings: locale={}, view_distance={}",
                client_id,
                info.locale,
                info.view_distance
            );

            world.entity_mut(entity).insert(ClientSettings {
                locale: info.locale.clone(),
                view_distance: info.view_distance,
            });
        }
        serverbound::ConfigurationPacket::PluginMessage(msg) => {
            tracing::debug!(
                "Client {} plugin message: channel={}",
                client_id,
                msg.channel
            );
        }
        serverbound::ConfigurationPacket::KnownPacks(_) => {
            let registries = world.resource::<RegistryDataStore>().registries.clone();
            let sender = world.resource::<NetworkChannels>().outgoing.clone();

            for registry in &registries {
                let _ = sender.send(OutgoingPacket {
                    client_id,
                    packet: clientbound::ClientboundPacket::Configuration(
                        clientbound::ConfigurationPacket::RegistryData(registry.clone()),
                    ),
                });
            }

            let _ = sender.send(OutgoingPacket {
                client_id,
                packet: clientbound::ClientboundPacket::Configuration(
                    clientbound::ConfigurationPacket::FinishConfiguration(
                        clientbound::FinishConfiguration {},
                    ),
                ),
            });
        }
        serverbound::ConfigurationPacket::FinishConfigurationAcknowledged(_) => {
            handle_finish_configuration(world, client_id, entity);
        }
    }
}

fn handle_finish_configuration(world: &mut World, client_id: u32, entity: Entity) {
    tracing::info!(
        "Client {} finished configuration, transitioning to Play",
        client_id
    );

    // Allocate entity ID
    let minecraft_entity_id = world.resource_scope(|_w, mut counter: Mut<EntityIdCounter>| {
        let id = counter.0;
        counter.0 += 1;
        id
    });

    // Read config values
    let (
        spawn_x,
        spawn_z,
        spawn_y_opt,
        hardcore,
        max_players,
        view_distance,
        simulation_distance,
        game_mode,
        initial_chunk_radius,
    ) = {
        let config = world.resource::<ServerConfigResource>();
        (
            config.spawn_x,
            config.spawn_z,
            config.spawn_y,
            config.hardcore,
            config.max_players,
            config.view_distance,
            config.simulation_distance,
            config.game_mode,
            config.initial_chunk_radius,
        )
    };

    let spawn_y = spawn_y_opt.unwrap_or_else(|| {
        let terrain_y = world
            .resource::<WorldGen>()
            .0
            .surface_height_at(spawn_x.floor() as i32, spawn_z.floor() as i32);
        (terrain_y + 1) as f64
    });

    let spawn_chunk = ChunkPos::from_block(spawn_x, spawn_z);

    // Insert all components directly — no deferred commands needed
    world.entity_mut(entity).insert((
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
        CurrentChunkPos(spawn_chunk),
        EffectiveViewDistance(initial_chunk_radius),
        LoadedChunks(HashSet::new()),
        PlayerDimension(DimensionId::Overworld),
    ));

    let sender = world.resource::<NetworkChannels>().outgoing.clone();

    // Send Play Login packet
    let _ = sender.send(OutgoingPacket {
        client_id,
        packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::Login(
            clientbound::Login {
                entity_id: minecraft_entity_id,
                is_hardcore: hardcore,
                dimension_names: vec![
                    "minecraft:overworld".to_string(),
                    "minecraft:the_nether".to_string(),
                    "minecraft:the_end".to_string(),
                ],
                max_players,
                view_distance,
                simulation_distance,
                reduced_debug_info: false,
                enable_respawn_screen: true,
                do_limited_crafting: false,
                dimension_type: 0,
                dimension_name: "minecraft:overworld".to_string(),
                hashed_seed: 0,
                game_mode,
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

    // Send Commands packet (command tree for tab-completion)
    let command_tree = world.resource::<CommandRegistry>().build_command_tree();
    let _ = sender.send(OutgoingPacket {
        client_id,
        packet: clientbound::ClientboundPacket::ManualPlay(
            clientbound::ManualPlayPacket::Commands(command_tree),
        ),
    });

    // Send GameEvent(StartWaitingForLevelChunks)
    let _ = sender.send(OutgoingPacket {
        client_id,
        packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::GameEvent(
            clientbound::GameEvent {
                event: clientbound::GameEventType::StartWaitingForLevelChunks,
                value: 0.0,
            },
        )),
    });

    // Send SetCenterChunk
    let _ = sender.send(OutgoingPacket {
        client_id,
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
    let mut query = world.query::<(&ChunkPosition, &ChunkData)>();

    for pos in spawn_chunk.chunks_in_radius(initial_chunk_radius) {
        let key = (DimensionId::Overworld, pos);
        let chunk_entity = {
            let chunk_index = world.resource::<ChunkIndex>();
            chunk_index.0.get(&key).copied()
        };
        if let Some(chunk_entity) = chunk_entity {
            if let Ok((chunk_pos, chunk_data)) = query.get(world, chunk_entity) {
                let pkt = chunk_data.to_packet(chunk_pos.0.x, chunk_pos.0.z);
                let _ = sender.send(OutgoingPacket {
                    client_id,
                    packet: clientbound::ClientboundPacket::ManualPlay(
                        clientbound::ManualPlayPacket::ChunkDataAndLight(pkt),
                    ),
                });
                loaded.insert(pos);
            }
        }

        // After the first 9 chunks (center 3×3), send the teleport
        if !teleport_sent && loaded.len() >= early_teleport_threshold {
            send_teleport(&sender, client_id, spawn_x, spawn_y, spawn_z);
            teleport_sent = true;
        }
    }

    if !teleport_sent {
        send_teleport(&sender, client_id, spawn_x, spawn_y, spawn_z);
    }

    tracing::info!(
        "Sent {} initial chunks to client {}",
        loaded.len(),
        client_id
    );

    // Update LoadedChunks directly — no deferred-commands issue
    world.get_mut::<LoadedChunks>(entity).unwrap().0 = loaded;

    // Trigger semantic event
    world.trigger(PlayerJoinEvent { client_id, entity });
    world.flush();
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
