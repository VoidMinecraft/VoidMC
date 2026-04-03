use bevy_ecs::prelude::*;
use void_protocol::clientbound;

use crate::components::{
    ClientId, ClientSettings, CurrentChunkPos, EffectiveViewDistance, LoadedChunks,
    PlayerDimension, PlayerReady, Position,
};
use crate::config::ServerConfigResource;
use crate::network::{NetworkChannels, OutgoingPacket};
use crate::world::{
    ChunkData, ChunkDimension, ChunkIndex, ChunkPos, ChunkPosition, generation::WorldGen,
};

/// Streams chunks to players as they move through the world.
pub fn stream_chunks(
    channels: Res<NetworkChannels>,
    mut chunk_index: ResMut<ChunkIndex>,
    chunk_query: Query<(&ChunkPosition, &ChunkData)>,
    mut players: Query<
        (
            &ClientId,
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
    config: Res<ServerConfigResource>,
) {
    for (
        client_id,
        position,
        mut current_chunk,
        mut effective_vd,
        mut loaded_chunks,
        dimension,
        settings,
    ) in players.iter_mut()
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
            let _ = channels.outgoing.send(OutgoingPacket {
                client_id: client_id.0,
                packet: clientbound::ClientboundPacket::Play(
                    clientbound::PlayPacket::SetCenterChunk(clientbound::SetCenterChunk {
                        chunk_x: new_chunk.x,
                        chunk_z: new_chunk.z,
                    }),
                ),
            });
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
            let _ = channels.outgoing.send(OutgoingPacket {
                client_id: client_id.0,
                packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::UnloadChunk(
                    clientbound::UnloadChunk {
                        chunk_x: pos.x,
                        chunk_z: pos.z,
                    },
                )),
            });
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
                let chunk = world_gen.0.generate_chunk(pos);
                let entity = commands
                    .spawn((
                        ChunkPosition(*pos),
                        ChunkData::from_protocol_chunk(&chunk),
                        ChunkDimension(dim_id),
                    ))
                    .id();
                chunk_index.0.insert(key, entity);

                // For newly spawned chunks, build the packet directly from the protocol chunk
                let packet = chunk.to_packet();
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: client_id.0,
                    packet: clientbound::ClientboundPacket::ManualPlay(
                        clientbound::ManualPlayPacket::ChunkDataAndLight(packet),
                    ),
                });
                loaded_chunks.0.insert(*pos);
                continue;
            }

            // Chunk exists in index — query its data
            if let Some(&chunk_entity) = chunk_index.0.get(&key) {
                if let Ok((chunk_pos, chunk_data)) = chunk_query.get(chunk_entity) {
                    let packet = chunk_data.to_packet(chunk_pos.0.x, chunk_pos.0.z);
                    let _ = channels.outgoing.send(OutgoingPacket {
                        client_id: client_id.0,
                        packet: clientbound::ClientboundPacket::ManualPlay(
                            clientbound::ManualPlayPacket::ChunkDataAndLight(packet),
                        ),
                    });
                    loaded_chunks.0.insert(*pos);
                }
            }
        }
    }
}
