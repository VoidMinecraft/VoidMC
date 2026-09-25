use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound;

use crate::components::{
    ChunkSendBudget, ChunkStreamBacklog, ClientSettings, CurrentChunkPos, EffectiveViewDistance,
    LoadedChunks, PlayerDimension, PlayerReady, Position,
};
use crate::config::ServerConfigResource;
use crate::players::Players;
use crate::registry::RegistryDataStore;
use crate::world::{
    ChunkData, ChunkDimension, ChunkIndex, ChunkLoaderResource, ChunkPos, ChunkPosition,
    generation::WorldGen, load_or_generate,
};

/// Streams chunks to players as they move through the world.
///
/// A player is revisited while stationary only when marked
/// [`ChunkStreamBacklog`], which this system sets whenever a pass ends with
/// chunks still unsent (per-player [`ChunkSendBudget`] or the generation cap).
///
/// `LoadedChunks` is both mutated here and read by `Players`, hence the
/// `ParamSet` and the outbox.
#[instrument(
    level = "info",
    skip(
        viewers_and_players,
        chunk_index,
        chunk_query,
        commands,
        world_gen,
        loader,
        registries,
        config
    )
)]
pub fn stream_chunks(
    mut viewers_and_players: ParamSet<(
        Query<
            (
                Entity,
                &Position,
                &mut CurrentChunkPos,
                &mut EffectiveViewDistance,
                &mut LoadedChunks,
                &PlayerDimension,
                Option<&ClientSettings>,
                Option<&ChunkSendBudget>,
                Option<&mut ChunkStreamBacklog>,
            ),
            With<PlayerReady>,
        >,
        Players,
    )>,
    mut chunk_index: ResMut<ChunkIndex>,
    chunk_query: Query<(&ChunkPosition, &ChunkData)>,
    mut commands: Commands,
    world_gen: Res<WorldGen>,
    loader: Option<Res<ChunkLoaderResource>>,
    registries: Option<Res<RegistryDataStore>>,
    config: Res<ServerConfigResource>,
) {
    let max_chunk_generations = config.max_chunk_generations_per_tick;
    let mut generated_this_tick = 0usize;
    let mut throttled = false;
    let mut outbox: Vec<(Entity, clientbound::ClientboundPacket)> = Vec::new();

    let mut viewers = viewers_and_players.p0();
    for (
        player,
        position,
        mut current_chunk,
        mut effective_vd,
        mut loaded_chunks,
        dimension,
        settings,
        send_budget,
        mut backlog,
    ) in viewers.iter_mut()
    {
        let new_chunk = ChunkPos::from_block(position.x, position.z);

        let view_distance = settings
            .map(|s| s.view_distance as i32)
            .unwrap_or(config.view_distance)
            .min(config.view_distance);

        let chunk_changed = new_chunk != current_chunk.0;
        let vd_changed = view_distance != effective_vd.0;
        let has_backlog = backlog.is_some();
        if !chunk_changed && !vd_changed && !has_backlog {
            continue;
        }

        // Update tracked state
        current_chunk.0 = new_chunk;
        effective_vd.0 = view_distance;

        // Send SetCenterChunk when the chunk position changed
        if chunk_changed {
            outbox.push((
                player,
                clientbound::SetCenterChunk {
                    chunk_x: new_chunk.x,
                    chunk_z: new_chunk.z,
                }
                .into(),
            ));
        }

        let carried = backlog
            .as_deref_mut()
            .filter(|_| !chunk_changed && !vd_changed)
            .map(|b| std::mem::take(&mut b.pending))
            .filter(|pending| !pending.is_empty());
        let mut pending = match carried {
            Some(pending) => pending,
            None => {
                let desired_sorted = new_chunk.chunks_in_radius(view_distance);
                let desired_set: std::collections::HashSet<ChunkPos> =
                    desired_sorted.iter().copied().collect();
                let to_unload: Vec<ChunkPos> = loaded_chunks
                    .0
                    .iter()
                    .filter(|pos| !desired_set.contains(pos))
                    .copied()
                    .collect();
                for pos in &to_unload {
                    outbox.push((
                        player,
                        clientbound::UnloadChunk {
                            chunk_x: pos.x,
                            chunk_z: pos.z,
                        }
                        .into(),
                    ));
                    loaded_chunks.0.remove(pos);
                }
                desired_sorted
            }
        };

        let dim_id = dimension.0;
        let budget = send_budget.map_or(usize::MAX, |b| b.0.max(1));
        let mut sent = 0usize;
        pending.retain(|pos| {
            if loaded_chunks.0.contains(pos) {
                return false;
            }
            if sent >= budget {
                return true;
            }

            let key = (dim_id, *pos);
            if !chunk_index.0.contains_key(&key) {
                if max_chunk_generations > 0 && generated_this_tick >= max_chunk_generations {
                    throttled = true;
                    return true;
                }

                let mut chunk_data = load_or_generate(loader.as_deref(), &world_gen, dim_id, pos);
                if let Some(registries) = &registries {
                    chunk_data.normalize_biomes(registries.biome_count());
                }
                let packet = chunk_data.to_packet(pos.x, pos.z);
                let entity = commands
                    .spawn((ChunkPosition(*pos), chunk_data, ChunkDimension(dim_id)))
                    .id();
                chunk_index.0.insert(key, entity);
                generated_this_tick += 1;

                outbox.push((player, packet.into()));
                loaded_chunks.0.insert(*pos);
                sent += 1;
                return false;
            }

            let Some(&chunk_entity) = chunk_index.0.get(&key) else {
                return true;
            };
            let Ok((chunk_pos, chunk_data)) = chunk_query.get(chunk_entity) else {
                return true;
            };
            let packet = chunk_data.to_packet(chunk_pos.0.x, chunk_pos.0.z);
            outbox.push((player, packet.into()));
            loaded_chunks.0.insert(*pos);
            sent += 1;
            false
        });

        match (pending.is_empty(), backlog) {
            (true, Some(_)) => {
                commands.entity(player).remove::<ChunkStreamBacklog>();
            }
            (true, None) => {}
            (false, Some(mut backlog)) => backlog.pending = pending,
            (false, None) => {
                commands
                    .entity(player)
                    .insert(ChunkStreamBacklog { pending });
            }
        }
    }

    let players = viewers_and_players.p1();
    for (player, packet) in outbox {
        players.send(player, packet);
    }

    if throttled {
        tracing::debug!(
            generated_this_tick,
            max_chunk_generations_per_tick = max_chunk_generations,
            "Chunk generation throttled"
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, PostUpdate};
    use voidmc_protocol::clientbound::{Chunk, ChunkBuilder, ClientboundPacket, ManualPlayPacket};

    use super::*;
    use crate::WorldGenerator;
    use crate::components::ClientId;
    use crate::config::ServerConfig;
    use crate::network::{NetworkChannels, OutgoingPacket};
    use crate::world::DimensionId;

    struct Empty;

    impl WorldGenerator for Empty {
        fn generate_chunk(&self, pos: &ChunkPos) -> Chunk {
            ChunkBuilder::new(pos.x, pos.z).build()
        }

        fn surface_height_at(&self, _: i32, _: i32) -> i32 {
            64
        }
    }

    fn test_app(config: ServerConfig) -> (App, flume::Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_non_send_resource(incoming_tx)
        .insert_resource(ServerConfigResource::from(&config))
        .insert_resource(WorldGen(Box::new(Empty)))
        .init_resource::<ChunkIndex>()
        .add_systems(PostUpdate, stream_chunks);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(id),
                Position {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
                CurrentChunkPos(ChunkPos::new(0, 0)),
                EffectiveViewDistance(0),
                LoadedChunks(Default::default()),
                PlayerDimension(DimensionId::Overworld),
                PlayerReady,
            ))
            .id()
    }

    fn chunk_positions(receiver: &flume::Receiver<OutgoingPacket>, client: u32) -> Vec<ChunkPos> {
        receiver
            .drain()
            .filter(|p| p.client_id == client)
            .filter_map(|p| match p.packet {
                ClientboundPacket::ManualPlay(ManualPlayPacket::ChunkDataAndLight(chunk)) => {
                    Some(ChunkPos::new(chunk.chunk_x, chunk.chunk_z))
                }
                _ => None,
            })
            .collect()
    }

    fn chunk_packets(receiver: &flume::Receiver<OutgoingPacket>, client: u32) -> usize {
        chunk_positions(receiver, client).len()
    }

    #[test]
    fn budget_throttles_generated_and_cached_chunks_until_the_range_is_complete() {
        let (mut app, receiver) = test_app(ServerConfig {
            view_distance: 2,
            max_chunk_generations_per_tick: 0,
            ..Default::default()
        });
        let throttled = player(&mut app, 1);
        app.world_mut()
            .entity_mut(throttled)
            .insert(ChunkSendBudget(2));

        for tick in 1..=12 {
            app.update();
            assert_eq!(chunk_packets(&receiver, 1), 2, "tick {tick}");
            assert!(app.world().get::<ChunkStreamBacklog>(throttled).is_some());
        }
        app.update();
        assert_eq!(chunk_packets(&receiver, 1), 1);
        assert_eq!(
            app.world().get::<LoadedChunks>(throttled).unwrap().0.len(),
            25
        );
        assert!(app.world().get::<ChunkStreamBacklog>(throttled).is_none());
        app.update();
        assert_eq!(chunk_packets(&receiver, 1), 0);

        let cached = player(&mut app, 2);
        app.world_mut()
            .entity_mut(cached)
            .insert(ChunkSendBudget(4));
        for _ in 0..6 {
            app.update();
            assert_eq!(chunk_packets(&receiver, 2), 4);
        }
        app.update();
        assert_eq!(chunk_packets(&receiver, 2), 1);
        assert_eq!(app.world().resource::<ChunkIndex>().0.len(), 25);
    }

    #[test]
    fn budgeted_drain_sends_every_chunk_exactly_once_nearest_first_without_recomputing() {
        let (mut app, receiver) = test_app(ServerConfig {
            view_distance: 3,
            max_chunk_generations_per_tick: 0,
            ..Default::default()
        });
        let entity = player(&mut app, 1);
        app.world_mut()
            .entity_mut(entity)
            .insert(ChunkSendBudget(5));
        let expected = ChunkPos::new(0, 0).chunks_in_radius(3);

        let mut received = Vec::new();
        for tick in 1..=9 {
            app.update();
            let sent = chunk_positions(&receiver, 1);
            assert_eq!(sent.len(), 5, "tick {tick}");
            received.extend(sent);
            let backlog = app.world().get::<ChunkStreamBacklog>(entity).unwrap();
            assert_eq!(backlog.pending, expected[received.len()..]);
        }
        app.update();
        received.extend(chunk_positions(&receiver, 1));
        assert_eq!(received, expected);
        assert!(app.world().get::<ChunkStreamBacklog>(entity).is_none());
        app.update();
        assert!(chunk_positions(&receiver, 1).is_empty());
    }

    #[test]
    fn moving_mid_backlog_rebuilds_the_range_around_the_new_chunk() {
        let (mut app, receiver) = test_app(ServerConfig {
            view_distance: 1,
            max_chunk_generations_per_tick: 0,
            ..Default::default()
        });
        let entity = player(&mut app, 1);
        app.world_mut()
            .entity_mut(entity)
            .insert(ChunkSendBudget(4));
        app.update();
        assert_eq!(chunk_packets(&receiver, 1), 4);

        app.world_mut().get_mut::<Position>(entity).unwrap().x = 160.0;
        app.update();
        let sent = chunk_positions(&receiver, 1);
        assert_eq!(sent.len(), 4);
        assert_eq!(sent[0], ChunkPos::new(10, 0));
        let pending = &app
            .world()
            .get::<ChunkStreamBacklog>(entity)
            .unwrap()
            .pending;
        assert_eq!(pending.len(), 5);
        assert!(
            pending
                .iter()
                .all(|p| (p.x - 10).abs() <= 1 && p.z.abs() <= 1)
        );
        let loaded = &app.world().get::<LoadedChunks>(entity).unwrap().0;
        assert!(loaded.iter().all(|p| (p.x - 10).abs() <= 1));
    }

    #[test]
    fn players_without_a_budget_receive_the_whole_range_at_once() {
        let (mut app, receiver) = test_app(ServerConfig {
            view_distance: 3,
            max_chunk_generations_per_tick: 0,
            ..Default::default()
        });
        let free = player(&mut app, 1);
        app.update();
        assert_eq!(chunk_packets(&receiver, 1), 49);
        assert!(app.world().get::<ChunkStreamBacklog>(free).is_none());
        app.update();
        assert_eq!(chunk_packets(&receiver, 1), 0);
    }

    #[test]
    fn generation_cap_backlog_resumes_while_stationary() {
        let (mut app, receiver) = test_app(ServerConfig {
            view_distance: 1,
            max_chunk_generations_per_tick: 4,
            ..Default::default()
        });
        let entity = player(&mut app, 1);
        app.update();
        assert_eq!(chunk_packets(&receiver, 1), 4);
        assert!(app.world().get::<ChunkStreamBacklog>(entity).is_some());
        app.update();
        assert_eq!(chunk_packets(&receiver, 1), 4);
        app.update();
        assert_eq!(chunk_packets(&receiver, 1), 1);
        assert!(app.world().get::<ChunkStreamBacklog>(entity).is_none());
    }
}
