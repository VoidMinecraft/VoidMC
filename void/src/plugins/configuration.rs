use std::collections::HashSet;

use bevy_app::{App, Plugin};
use bevy_ecs::{
    entity::Entity,
    observer::On,
    system::{Commands, Query, Res},
    world::World,
};
use voidmc_protocol::{
    State, clientbound,
    serverbound::{ClientInformation, FinishConfigurationAcknowledged, KnownPacks, PluginMessage},
    types::BlockPosition,
};

use crate::{
    RegistryDataStore, ServerConfigResource, WorldGen,
    components::{
        ChunkStreamBacklog, ClientSettings, ConnectionState, CurrentChunkPos,
        EffectiveViewDistance, KeepAliveState, LoadedChunks, MinecraftEntityId, PlayerDimension,
        Position, PreviousPosition, Rotation, TeleportState,
    },
    events::PlayerJoinEvent,
    network::PacketEvent,
    players::Players,
    systems::chunk::effective_view_distance,
    world::{
        ChunkData, ChunkDimension, ChunkIndex, ChunkLoaderResource, ChunkPos, ChunkPosition,
        DimensionId, load_or_generate,
    },
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

/// Client Information is valid in the configuration and the play state; this
/// one observer stores it from either.
fn handle_client_information(event: On<PacketEvent<ClientInformation>>, mut commands: Commands) {
    commands.entity(event.entity).insert(ClientSettings {
        locale: event.packet.locale.clone(),
        view_distance: event.packet.view_distance,
        chat_mode: event.packet.chat_mode,
    });
}

fn handle_plugin_message(_event: On<PacketEvent<PluginMessage>>) {}

fn handle_known_packs(
    event: On<PacketEvent<KnownPacks>>,
    players: Players,
    registries: Res<RegistryDataStore>,
) {
    registries.mark_sent();
    for registry in registries.registries() {
        players.send(event.entity, registry.clone());
    }

    players.send(event.entity, build_update_tags(&registries));
    players.send(event.entity, clientbound::FinishConfiguration {});
}

/// Translates `voidmc_data::tagged_registries()` into the wire format. Tag
/// values are mapped to numeric indices using the order of entries in the
/// registry as it was just sent.
fn build_update_tags(registries: &RegistryDataStore) -> clientbound::UpdateTags {
    let version = voidmc_data::Version::V26_1_2;
    let mut out = Vec::new();

    for (registry_id, tags) in voidmc_data::tagged_registries(version) {
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
    world: &World,
    config: Res<ServerConfigResource>,
    world_gen: Res<WorldGen>,
    players: Players,
    chunk_index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    loader: Option<Res<ChunkLoaderResource>>,
    registries: Option<Res<RegistryDataStore>>,
    settings: Query<&ClientSettings>,
) {
    // The join never sends chunks the player's effective view distance would
    // forget on the first streaming pass.
    let view_distance =
        effective_view_distance(settings.get(event.entity).ok(), config.view_distance);
    let join_radius = config.initial_chunk_radius.min(view_distance);
    // Determine spawn position
    let spawn_y = config.spawn_y.unwrap_or_else(|| {
        world_gen
            .0
            .surface_height_at(config.spawn_x.floor() as i32, config.spawn_z.floor() as i32)
            as f64
    });
    let spawn_chunk = ChunkPos::from_block(config.spawn_x, config.spawn_z);

    let minecraft_entity_id = MinecraftEntityId::allocate();

    // Update entity with new components
    commands.entity(event.entity).insert((
        ConnectionState(State::Play),
        minecraft_entity_id,
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
        KeepAliveState::default(),
        CurrentChunkPos(spawn_chunk),
        EffectiveViewDistance(join_radius),
        LoadedChunks(HashSet::new()),
        PlayerDimension(DimensionId::Overworld),
        crate::inventory::Inventory::new(),
    ));

    // Send login success packet
    players.send(
        event.entity,
        clientbound::Login {
            entity_id: minecraft_entity_id.0,
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
    );

    // The player's permission level, then the commands it may run (the tree
    // drives the client's tab completion).
    super::permissions::send_permissions(world, event.entity, minecraft_entity_id.0);

    // The world spawn point, the last level information vanilla sends before
    // the client starts waiting for chunks.
    players.send(
        event.entity,
        clientbound::SetDefaultSpawnPosition {
            dimension: "minecraft:overworld".to_string(),
            position: BlockPosition {
                x: config.spawn_x.floor() as i32,
                y: spawn_y.floor() as i16,
                z: config.spawn_z.floor() as i32,
            },
            yaw: 0.0,
            pitch: 0.0,
        },
    );

    // Send GameEvent (Start waiting for level chunks)
    players.send(
        event.entity,
        clientbound::GameEvent {
            event: clientbound::GameEventType::StartWaitingForLevelChunks,
            value: 0.0,
        },
    );

    // Send SetCenterChunk
    players.send(
        event.entity,
        clientbound::SetCenterChunk {
            chunk_x: spawn_chunk.x,
            chunk_z: spawn_chunk.z,
        },
    );

    // Send initial chunks around spawn, in one batch
    players.send(event.entity, clientbound::ChunkBatchStart {});
    let early_teleport_threshold = 9;
    let mut loaded = HashSet::new();
    let mut teleport_sent = false;

    // The client leaves its loading screen once it holds the chunk it stands
    // in, and chunks are streamed only to players that left it. That chunk is
    // always sent: when it is not loaded (a spawn moved after startup, or no
    // spawn area), it is loaded or generated here as streaming would.
    let spawn_key = (DimensionId::Overworld, spawn_chunk);
    let missing_spawn_chunk = (!chunk_index.0.contains_key(&spawn_key)).then(|| {
        let mut chunk_data = load_or_generate(
            loader.as_deref(),
            &world_gen,
            DimensionId::Overworld,
            &spawn_chunk,
        );
        if let Some(registries) = &registries {
            chunk_data.normalize_biomes(registries.biome_count());
        }
        chunk_data
    });

    let join_chunks = join_chunks(spawn_chunk, join_radius, view_distance);
    let join_chunk_count = join_chunks.len();
    for pos in join_chunks {
        let key = (DimensionId::Overworld, pos);
        let packet = match chunk_index.0.get(&key) {
            Some(&chunk_entity) => chunks
                .get(chunk_entity)
                .ok()
                .map(|(chunk_pos, chunk_data)| chunk_data.to_packet(chunk_pos.0.x, chunk_pos.0.z)),
            None if pos == spawn_chunk => missing_spawn_chunk
                .as_ref()
                .map(|chunk_data| chunk_data.to_packet(pos.x, pos.z)),
            None => None,
        };
        // Nearest-first: a chunk that is not ready yet (outside the spawn
        // area, or still pending spawn) holds back every farther one, which
        // chunk streaming then sends in order.
        let Some(packet) = packet else {
            break;
        };
        players.send(event.entity, packet);
        loaded.insert(pos);

        // After the first 9 chunks (center 3×3), send the teleport
        if !teleport_sent && loaded.len() >= early_teleport_threshold {
            send_teleport(
                &players,
                event.entity,
                config.spawn_x,
                spawn_y,
                config.spawn_z,
            );
            teleport_sent = true;
        }
    }
    players.send(
        event.entity,
        clientbound::ChunkBatchFinished {
            batch_size: loaded.len() as i32,
        },
    );

    if !teleport_sent {
        send_teleport(
            &players,
            event.entity,
            config.spawn_x,
            spawn_y,
            config.spawn_z,
        );
    }

    if let Some(chunk_data) = missing_spawn_chunk {
        // Another player joining in the same tick may have added it first.
        commands.queue(move |world: &mut World| {
            if world.resource::<ChunkIndex>().0.contains_key(&spawn_key) {
                return;
            }
            let chunk_entity = world
                .spawn((
                    ChunkPosition(spawn_chunk),
                    chunk_data,
                    ChunkDimension(DimensionId::Overworld),
                ))
                .id();
            world
                .resource_mut::<ChunkIndex>()
                .0
                .insert(spawn_key, chunk_entity);
        });
    }

    // Chunks outside the pre-generated spawn area were held back. A backlog
    // makes chunk streaming generate them on its first pass, even when the
    // player's view distance equals the join radius and nothing else would
    // trigger it.
    let missing_join_chunks = loaded.len() < join_chunk_count;
    let mut player = commands.entity(event.entity);
    player.insert(LoadedChunks(loaded));
    if missing_join_chunks {
        player.insert(ChunkStreamBacklog::default());
    }
    commands.trigger(PlayerJoinEvent {
        client_id: event.client_id,
        entity: event.entity,
    });
}

/// The chunks the join sends, nearest-first (squared Euclidean distance from
/// `spawn_chunk`, the order of [`ChunkPos::chunks_in_radius`]).
///
/// When the join radius covers the whole effective view distance, that is the
/// whole square. Otherwise chunk streaming sends the rest of the view square
/// right after the join, so the join stops before the first chunk outside its
/// square: it sends the chunks of the square strictly nearer than
/// `join_radius + 1`, and every chunk streaming sends next is at least as far
/// as the farthest chunk of the join. The whole sequence from the join on is
/// then nearest-first.
fn join_chunks(spawn_chunk: ChunkPos, join_radius: i32, view_distance: i32) -> Vec<ChunkPos> {
    let join_radius = join_radius.max(0);
    let mut chunks = spawn_chunk.chunks_in_radius(join_radius);
    if join_radius < view_distance {
        let bound = (join_radius + 1) * (join_radius + 1);
        chunks.retain(|pos| {
            let dx = pos.x - spawn_chunk.x;
            let dz = pos.z - spawn_chunk.z;
            dx * dx + dz * dz < bound
        });
    }
    chunks
}

fn send_teleport(players: &Players, player: Entity, x: f64, y: f64, z: f64) {
    players.send(
        player,
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
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn squared_distance(a: ChunkPos, b: ChunkPos) -> i32 {
        (a.x - b.x).pow(2) + (a.z - b.z).pow(2)
    }

    /// The chunks the join sends followed by the chunks the first streaming
    /// pass sends (the view square nearest-first, minus what the client holds).
    fn join_then_stream(spawn: ChunkPos, initial: i32, view_distance: i32) -> Vec<ChunkPos> {
        let join_radius = initial.min(view_distance);
        let mut sent = join_chunks(spawn, join_radius, view_distance);
        let held: HashSet<ChunkPos> = sent.iter().copied().collect();
        sent.extend(
            spawn
                .chunks_in_radius(view_distance)
                .into_iter()
                .filter(|pos| !held.contains(pos)),
        );
        sent
    }

    #[test]
    fn the_join_sends_its_whole_square_when_it_covers_the_view_distance() {
        let spawn = ChunkPos::new(3, -2);
        for (initial, view_distance) in [(4, 4), (4, 3), (3, 2), (0, 0)] {
            let join_radius = initial.min(view_distance);
            assert_eq!(
                join_chunks(spawn, join_radius, view_distance),
                spawn.chunks_in_radius(join_radius),
                "initial {initial}, view distance {view_distance}"
            );
        }
    }

    #[test]
    fn a_view_distance_beyond_the_join_radius_cuts_the_corners_of_the_join() {
        let spawn = ChunkPos::new(0, 0);
        let chunks = join_chunks(spawn, 4, 10);
        // Every chunk strictly nearer than (5, 0), all of them in the square.
        let expected = spawn
            .chunks_in_radius(4)
            .into_iter()
            .filter(|pos| squared_distance(*pos, spawn) < 25)
            .count();
        assert_eq!(chunks.len(), expected);
        assert!(chunks.contains(&ChunkPos::new(4, 2)));
        assert!(!chunks.contains(&ChunkPos::new(4, 3)));
        assert!(!chunks.contains(&ChunkPos::new(4, 4)));
        assert_eq!(chunks[0], spawn, "the spawn chunk comes first");
    }

    #[test]
    fn a_join_radius_of_one_still_sends_the_centre_three_by_three() {
        let spawn = ChunkPos::new(-7, 5);
        assert_eq!(join_chunks(spawn, 1, 10).len(), 9);
        assert_eq!(join_chunks(spawn, 0, 10), vec![spawn]);
        assert_eq!(join_chunks(spawn, -1, 10), vec![spawn]);
    }

    #[test]
    fn chunks_are_nearest_first_from_the_join_through_the_first_stream() {
        let spawn = ChunkPos::new(2, -3);
        for initial in 0..=6 {
            for view_distance in 0..=12 {
                let sent = join_then_stream(spawn, initial, view_distance);
                let unique: HashSet<ChunkPos> = sent.iter().copied().collect();
                assert_eq!(unique.len(), sent.len(), "a chunk was sent twice");
                assert_eq!(
                    unique,
                    spawn.chunks_in_radius(view_distance).into_iter().collect(),
                    "initial {initial}, view distance {view_distance}"
                );
                for pair in sent.windows(2) {
                    assert!(
                        squared_distance(pair[0], spawn) <= squared_distance(pair[1], spawn),
                        "initial {initial}, view distance {view_distance}: {:?} sent before \
                         the nearer {:?}",
                        pair[0],
                        pair[1]
                    );
                }
            }
        }
    }
}
