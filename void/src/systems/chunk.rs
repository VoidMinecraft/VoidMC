use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound;

use crate::components::{
    ClientSettings, CurrentChunkPos, EffectiveViewDistance, LoadedChunks, PlayerDimension,
    PlayerReady, Position,
};
use crate::config::ServerConfigResource;
use crate::players::Players;
use crate::world::{
    ChunkData, ChunkDimension, ChunkIndex, ChunkLoaderResource, ChunkPos, ChunkPosition,
    generation::WorldGen, load_or_generate,
};

/// Streams chunks to players as they move through the world.
#[instrument(
    level = "info",
    skip(
        players,
        chunk_index,
        chunk_query,
        viewers,
        commands,
        world_gen,
        loader,
        config
    )
)]
pub fn stream_chunks(
    players: Players,
    mut chunk_index: ResMut<ChunkIndex>,
    chunk_query: Query<(&ChunkPosition, &ChunkData)>,
    mut viewers: Query<
        (
            Entity,
            &Position,
            &mut CurrentChunkPos,
            &mut EffectiveViewDistance,
            &mut LoadedChunks,
            &PlayerDimension,
            Option<&ClientSettings>,
        ),
        With<PlayerReady>,
    >,
    mut commands: Commands,
    world_gen: Res<WorldGen>,
    loader: Option<Res<ChunkLoaderResource>>,
    config: Res<ServerConfigResource>,
) {
    let max_chunk_generations = config.max_chunk_generations_per_tick;
    let mut generated_this_tick = 0usize;
    let mut throttled = false;

    for (
        player,
        position,
        mut current_chunk,
        mut effective_vd,
        mut loaded_chunks,
        dimension,
        settings,
    ) in viewers.iter_mut()
    {
        let new_chunk = ChunkPos::from_block(position.x, position.z);

        let view_distance = settings
            .map(|s| s.view_distance as i32)
            .unwrap_or(config.view_distance)
            .min(config.view_distance);

        // Skip if player hasn't moved to a new chunk AND view distance is unchanged
        let chunk_changed = new_chunk != current_chunk.0;
        let vd_changed = view_distance != effective_vd.0;
        if !chunk_changed && !vd_changed {
            continue;
        }

        // Update tracked state
        current_chunk.0 = new_chunk;
        effective_vd.0 = view_distance;

        // Send SetCenterChunk when the chunk position changed
        if chunk_changed {
            players.send(
                player,
                clientbound::SetCenterChunk {
                    chunk_x: new_chunk.x,
                    chunk_z: new_chunk.z,
                },
            );
        }

        let desired_sorted = new_chunk.chunks_in_radius(view_distance);
        let desired_set: std::collections::HashSet<ChunkPos> =
            desired_sorted.iter().copied().collect();

        // Unload chunks no longer in range
        let to_unload: Vec<ChunkPos> = loaded_chunks
            .0
            .iter()
            .filter(|pos| !desired_set.contains(pos))
            .copied()
            .collect();

        for pos in &to_unload {
            players.send(
                player,
                clientbound::UnloadChunk {
                    chunk_x: pos.x,
                    chunk_z: pos.z,
                },
            );
            loaded_chunks.0.remove(pos);
        }

        // Load new chunks in range (nearest-first order preserved)
        let dim_id = dimension.0;
        for pos in &desired_sorted {
            if loaded_chunks.0.contains(pos) {
                continue;
            }

            let key = (dim_id, *pos);

            // Generate chunk on-demand if not in index
            if !chunk_index.0.contains_key(&key) {
                if max_chunk_generations > 0 && generated_this_tick >= max_chunk_generations {
                    throttled = true;
                    continue;
                }

                let chunk_data = load_or_generate(loader.as_deref(), &world_gen, dim_id, pos);
                let packet = chunk_data.to_packet(pos.x, pos.z);
                let entity = commands
                    .spawn((ChunkPosition(*pos), chunk_data, ChunkDimension(dim_id)))
                    .id();
                chunk_index.0.insert(key, entity);
                generated_this_tick += 1;

                players.send(player, packet);
                loaded_chunks.0.insert(*pos);
                continue;
            }

            // Chunk exists in index — query its data
            if let Some(&chunk_entity) = chunk_index.0.get(&key) {
                if let Ok((chunk_pos, chunk_data)) = chunk_query.get(chunk_entity) {
                    let packet = chunk_data.to_packet(chunk_pos.0.x, chunk_pos.0.z);
                    players.send(player, packet);
                    loaded_chunks.0.insert(*pos);
                }
            }
        }
    }

    if throttled {
        tracing::warn!(
            generated_this_tick,
            max_chunk_generations_per_tick = max_chunk_generations,
            "Chunk generation throttled"
        );
    }
}
