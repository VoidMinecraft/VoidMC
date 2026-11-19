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

        let view_distance = effective_view_distance(settings, config.view_distance);

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
        // Nearest-first: once a chunk has to wait (budget, generation cap or
        // spawn still pending), every farther one waits behind it.
        let mut held_back = false;
        let budget = send_budget.map_or(usize::MAX, |b| b.0.max(1));
        let mut sent = 0usize;
        pending.retain(|pos| {
            if loaded_chunks.0.contains(pos) {
                return false;
            }
            if held_back || sent >= budget {
                return true;
            }

            let key = (dim_id, *pos);
            if !chunk_index.0.contains_key(&key) {
                if max_chunk_generations > 0 && generated_this_tick >= max_chunk_generations {
                    throttled = true;
                    held_back = true;
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
                held_back = true;
                return true;
            };
            let Ok((chunk_pos, chunk_data)) = chunk_query.get(chunk_entity) else {
                held_back = true;
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

/// The view distance chunks are streamed with: the client's requested view
/// distance capped by the server's, or the server's when the client never sent
/// its settings.
pub(crate) fn effective_view_distance(settings: Option<&ClientSettings>, server: i32) -> i32 {
    settings
        .map_or(server, |s| i32::from(s.view_distance))
        .min(server)
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, PostUpdate};
    use voidmc_protocol::clientbound::{Chunk, ChunkBuilder, ClientboundPacket, ManualPlayPacket};

    use super::*;
    use crate::WorldGenerator;
    use crate::components::ClientId;
    use crate::config::ServerConfig;
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
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
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx))
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
    fn a_chunk_still_pending_spawn_holds_back_the_farther_ones() {
        let (mut app, receiver) = test_app(ServerConfig {
            view_distance: 1,
            max_chunk_generations_per_tick: 0,
            ..Default::default()
        });
        // Every chunk of the 3×3 range is cached except its centre, which the
        // first player to stream generates; for the second player it is still
        // pending spawn in that tick.
        let range = ChunkPos::new(0, 0).chunks_in_radius(1);
        for pos in &range[1..] {
            let data = ChunkData::from_protocol_chunk(&Empty.generate_chunk(pos));
            let chunk = app
                .world_mut()
                .spawn((
                    ChunkPosition(*pos),
                    data,
                    ChunkDimension(DimensionId::Overworld),
                ))
                .id();
            app.world_mut()
                .resource_mut::<ChunkIndex>()
                .0
                .insert((DimensionId::Overworld, *pos), chunk);
        }
        player(&mut app, 1);
        player(&mut app, 2);

        let mut received = [Vec::new(), Vec::new()];
        for _ in 0..2 {
            app.update();
            let outgoing: Vec<OutgoingPacket> = receiver.drain().collect();
            for (index, client) in [1, 2].into_iter().enumerate() {
                received[index].extend(outgoing.iter().filter_map(|p| match &p.packet {
                    ClientboundPacket::ManualPlay(ManualPlayPacket::ChunkDataAndLight(chunk))
                        if p.client_id == client =>
                    {
                        Some(ChunkPos::new(chunk.chunk_x, chunk.chunk_z))
                    }
                    _ => None,
                }));
            }
        }
        assert_eq!(
            received[0], range,
            "client 1 receives its range nearest-first"
        );
        assert_eq!(
            received[1], range,
            "client 2 receives its range nearest-first"
        );
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
