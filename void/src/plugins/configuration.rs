use std::collections::HashSet;

use bevy_app::{App, Plugin};
use bevy_ecs::{
    entity::Entity,
    observer::On,
    system::{Commands, Query, Res},
};
use voidmc_protocol::{
    State, clientbound,
    serverbound::{ClientInformation, FinishConfigurationAcknowledged, KnownPacks, PluginMessage},
};

use crate::{
    CommandRegistry, RegistryDataStore, ServerConfigResource, WorldGen,
    components::{
        ClientSettings, ConnectionState, CurrentChunkPos, EffectiveViewDistance, KeepAliveState,
        LoadedChunks, MinecraftEntityId, PlayerDimension, PlayerUuid, Position, PreviousPosition,
        Rotation, TeleportState,
    },
    events::PlayerJoinEvent,
    network::PacketEvent,
    players::Players,
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
    players: Players,
    registries: Res<RegistryDataStore>,
) {
    registries.mark_sent();
    for registry in &registries.registries {
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
    command_registry: Res<CommandRegistry>,
    config: Res<ServerConfigResource>,
    world_gen: Res<WorldGen>,
    players: Players,
    chunk_index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    uuids: Query<&PlayerUuid>,
    entity_ids: Query<&MinecraftEntityId>,
) {
    // Determine spawn position
    let spawn_y = config.spawn_y.unwrap_or_else(|| {
        world_gen
            .0
            .surface_height_at(config.spawn_x.floor() as i32, config.spawn_z.floor() as i32)
            as f64
            + 1.0
    });
    let spawn_chunk = ChunkPos::from_block(config.spawn_x, config.spawn_z);

    let stable_id = uuids
        .get(event.entity)
        .ok()
        .filter(|_| config.stable_player_ids)
        .map(|uuid| MinecraftEntityId::of_player(uuid.0));
    let minecraft_entity_id = player_entity_id(stable_id, entity_ids.iter().copied());

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
        EffectiveViewDistance(config.initial_chunk_radius),
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

    // Send Commands packet (command tree for tab completion)
    players.send(event.entity, command_registry.build_command_tree());

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
                players.send(event.entity, pkt);
                loaded.insert(pos);
            }
        }

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

    if !teleport_sent {
        send_teleport(
            &players,
            event.entity,
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

/// An id already in the world gives way to a fresh one: two uuids can fold to the same value.
fn player_entity_id(
    stable: Option<MinecraftEntityId>,
    mut taken: impl Iterator<Item = MinecraftEntityId>,
) -> MinecraftEntityId {
    stable
        .filter(|id| !taken.any(|other| other == *id))
        .unwrap_or_else(MinecraftEntityId::allocate)
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
    use uuid::Uuid;

    use super::*;

    #[test]
    fn a_player_keeps_its_id_from_one_run_to_the_next() {
        let uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        let id = MinecraftEntityId::of_player(uuid);
        assert_eq!(id, MinecraftEntityId::of_player(uuid));
        assert!(id.0 >= 0x4000_0000);
        assert_eq!(player_entity_id(Some(id), std::iter::empty()), id);
    }

    #[test]
    fn a_stable_id_gives_way_when_it_is_taken() {
        let id = MinecraftEntityId::of_player(Uuid::nil());
        assert_ne!(player_entity_id(Some(id), std::iter::once(id)), id);
        assert!(player_entity_id(None, std::iter::empty()).0 < 0x4000_0000);
    }
}
