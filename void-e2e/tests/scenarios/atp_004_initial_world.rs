//! VMC-ATP-004: the connected player receives the initial chunk area around
//! spawn, can look around and walk away and back, and finds the world as it
//! left it.

use std::collections::{HashMap, HashSet};

use azalea_entity::LookDirection;
use voidmc::components::{KeepAliveState, Position, Rotation};
use voidmc::systems::KeepAliveTicker;
use voidmc_e2e::core::position::{BlockPos, ChunkPos, Vec3};
use voidmc_e2e::protocol::common::movements::MoveFlags;
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, c_game_event::EventType,
    s_chunk_batch_received::ServerboundChunkBatchReceived,
    s_move_player_rot::ServerboundMovePlayerRot,
};
use voidmc_e2e::{Bot, BotOptions, ClientView, TestServer};

use crate::{feet, ground};

/// The `TestServer` default view distance; bots ask for more, so it is the
/// effective one.
const VIEW_DISTANCE: i32 = 4;

/// What the vanilla client acts on while it loads terrain, in arrival order.
#[derive(Clone, Debug, PartialEq)]
enum Seen {
    Login { chunk_radius: u32 },
    WaitForLevelChunks,
    Center(ChunkPos),
    BatchStart,
    Chunk(ChunkPos),
    BatchFinished(u32),
    Forget(ChunkPos),
    Teleport(Vec3),
    KeepAlive,
}

fn trace(bot: &Bot) -> Vec<Seen> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::Login(login) => Some(Seen::Login {
            chunk_radius: login.chunk_radius,
        }),
        ClientboundGamePacket::GameEvent(event) if event.event == EventType::WaitForLevelChunks => {
            Some(Seen::WaitForLevelChunks)
        }
        ClientboundGamePacket::SetChunkCacheCenter(center) => {
            Some(Seen::Center(ChunkPos::new(center.x, center.z)))
        }
        ClientboundGamePacket::ChunkBatchStart(_) => Some(Seen::BatchStart),
        ClientboundGamePacket::LevelChunkWithLight(chunk) => {
            Some(Seen::Chunk(ChunkPos::new(chunk.x, chunk.z)))
        }
        ClientboundGamePacket::ChunkBatchFinished(finished) => {
            Some(Seen::BatchFinished(finished.batch_size))
        }
        ClientboundGamePacket::ForgetLevelChunk(forget) => Some(Seen::Forget(forget.pos)),
        ClientboundGamePacket::PlayerPosition(teleport) => {
            Some(Seen::Teleport(teleport.change.pos))
        }
        ClientboundGamePacket::KeepAlive(_) => Some(Seen::KeepAlive),
        _ => None,
    })
}

/// Checks a chunk stream the way the vanilla client consumes it: every chunk
/// arrives inside a `ChunkBatchStart`/`ChunkBatchFinished` pair whose size
/// counts it, and within the client's chunk storage around the current chunk
/// cache centre (`max(radius, 2) + 3`), outside of which the client drops it.
/// Returns how many batches were seen.
fn assert_chunks_land_in_framed_batches(
    trace: &[Seen],
    chunk_radius: u32,
    mut center: Option<ChunkPos>,
) -> usize {
    let storage = chunk_radius.max(2) as i32 + 3;
    let mut open: Option<u32> = None;
    let mut batches = 0;
    for seen in trace {
        match seen {
            Seen::Center(pos) => center = Some(*pos),
            Seen::BatchStart => {
                assert!(open.is_none(), "a chunk batch started inside another");
                open = Some(0);
            }
            Seen::Chunk(pos) => {
                let count = open
                    .as_mut()
                    .unwrap_or_else(|| panic!("chunk {pos:?} was sent outside a chunk batch"));
                *count += 1;
                let center =
                    center.unwrap_or_else(|| panic!("chunk {pos:?} arrived before any centre"));
                assert!(
                    (pos.x - center.x).abs() <= storage && (pos.z - center.z).abs() <= storage,
                    "chunk {pos:?} is outside the client's chunk storage around {center:?}"
                );
            }
            Seen::BatchFinished(size) => {
                let count = open
                    .take()
                    .expect("a chunk batch finished without having started");
                assert_eq!(*size, count, "the batch size counts the chunks it framed");
                batches += 1;
            }
            _ => {}
        }
    }
    assert!(open.is_none(), "a chunk batch was never finished");
    batches
}

/// Answers every finished batch like the vanilla client does.
async fn acknowledge_batches(bot: &Bot, trace: &[Seen]) {
    for seen in trace {
        if matches!(seen, Seen::BatchFinished(_)) {
            bot.send(ServerboundChunkBatchReceived {
                desired_chunks_per_tick: 25.0,
            })
            .await;
        }
    }
}

fn login_chunk_radius(trace: &[Seen]) -> u32 {
    trace
        .iter()
        .find_map(|seen| match seen {
            Seen::Login { chunk_radius } => Some(*chunk_radius),
            _ => None,
        })
        .expect("the login was received")
}

fn assert_each_chunk_sent_once(trace: &[Seen], expected: &HashSet<ChunkPos>) {
    let mut sent: Vec<ChunkPos> = trace
        .iter()
        .filter_map(|seen| match seen {
            Seen::Chunk(pos) => Some(*pos),
            _ => None,
        })
        .collect();
    sent.sort_by_key(|pos| (pos.x, pos.z));
    let mut expected: Vec<ChunkPos> = expected.iter().copied().collect();
    expected.sort_by_key(|pos| (pos.x, pos.z));
    assert_eq!(
        sent, expected,
        "every chunk of the square is sent exactly once"
    );
}

fn keep_alives(trace: &[Seen]) -> usize {
    trace
        .iter()
        .filter(|seen| matches!(seen, Seen::KeepAlive))
        .count()
}

fn square(center: ChunkPos, radius: i32) -> HashSet<ChunkPos> {
    (-radius..=radius)
        .flat_map(|dx| {
            (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
        })
        .collect()
}

fn held(view: &ClientView) -> HashSet<ChunkPos> {
    view.chunks.keys().copied().collect()
}

/// Whether the chunk holds terrain: a non-air block somewhere in its middle
/// column.
fn has_terrain(view: &ClientView, chunk: ChunkPos) -> bool {
    let top = BlockPos::new(chunk.x * 16 + 8, 319, chunk.z * 16 + 8);
    view.ground_below(top).is_some()
}

async fn wait_for_the_square(bot: &Bot, what: &str, center: ChunkPos, radius: i32) {
    bot.wait_until(what, |view| {
        view.chunk_center == Some(center) && held(view) == square(center, radius)
    })
    .await;
}

async fn walk(bot: &Bot, from: Vec3, to: Vec3) {
    let steps = (to.x - from.x).abs().max((to.z - from.z).abs()).ceil() as i32;
    for step in 1..=steps {
        let t = f64::from(step) / f64::from(steps);
        bot.move_to(Vec3::new(
            from.x + (to.x - from.x) * t,
            from.y,
            from.z + (to.z - from.z) * t,
        ))
        .await;
        bot.sync().await;
    }
}

async fn server_position(server: &TestServer, bot: &Bot) -> Vec3 {
    let entity = server.player_entity(bot.uuid()).await;
    server
        .with_world(move |world| {
            let position = world
                .get::<Position>(entity)
                .expect("players have a position");
            Vec3::new(position.x, position.y, position.z)
        })
        .await
}

#[tokio::test]
async fn the_initial_area_arrives_ready_to_render() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let spawn = ChunkPos::from(feet(&alice));

    wait_for_the_square(&alice, "the area around spawn", spawn, VIEW_DISTANCE).await;
    alice.sync().await;
    let trace = trace(&alice);

    let Some(Seen::Login { chunk_radius }) = trace.first().cloned() else {
        panic!("the first play packet is not the login: {trace:?}");
    };
    assert!(
        chunk_radius >= VIEW_DISTANCE as u32,
        "the client keeps a chunk storage wide enough for the view distance"
    );
    assert!(
        trace.contains(&Seen::WaitForLevelChunks),
        "the client is told to wait for level chunks"
    );
    let batches = assert_chunks_land_in_framed_batches(&trace, chunk_radius, None);
    assert!(batches >= 1);

    assert_each_chunk_sent_once(&trace, &square(spawn, VIEW_DISTANCE));

    let teleports: Vec<Vec3> = trace
        .iter()
        .filter_map(|seen| match seen {
            Seen::Teleport(pos) => Some(*pos),
            _ => None,
        })
        .collect();
    assert_eq!(
        teleports,
        [alice.position()],
        "one position sync places the player at spawn"
    );
    assert_eq!(
        server_position(&server, &alice).await,
        alice.position(),
        "the client and the server agree on the spawn position"
    );

    let bare: Vec<ChunkPos> = alice.view(|view| {
        square(spawn, VIEW_DISTANCE)
            .into_iter()
            .filter(|chunk| !has_terrain(view, *chunk))
            .collect()
    });
    assert!(bare.is_empty(), "chunks without terrain: {bare:?}");
    assert_eq!(
        ground(&alice).y,
        feet(&alice).y - 1,
        "a floor under the spawn position"
    );

    acknowledge_batches(&alice, &trace).await;
    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn the_initial_area_is_whole_beyond_the_pregenerated_spawn_chunks() {
    let server = TestServer::builder()
        .config(|config| {
            config
                .view_distance(2)
                .spawn_chunk_radius(0)
                .initial_chunk_radius(2)
        })
        .start()
        .await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 2 })
        .await;
    let spawn = ChunkPos::from(feet(&alice));

    wait_for_the_square(&alice, "the whole area around spawn", spawn, 2).await;
    alice.sync().await;
    assert_eq!(alice.view(held), square(spawn, 2));
    // Only the spawn chunk is pre-generated: it comes in the join batch, the
    // rest is streamed in later batches, each chunk once.
    let trace = trace(&alice);
    let batches = assert_chunks_land_in_framed_batches(&trace, login_chunk_radius(&trace), None);
    assert!(
        batches >= 2,
        "the missing chunks were streamed after the join"
    );
    assert_each_chunk_sent_once(&trace, &square(spawn, 2));
    let bare: Vec<ChunkPos> = alice.view(|view| {
        square(spawn, 2)
            .into_iter()
            .filter(|chunk| !has_terrain(view, *chunk))
            .collect()
    });
    assert!(bare.is_empty(), "chunks without terrain: {bare:?}");

    acknowledge_batches(&alice, &trace).await;
    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn turning_on_the_spot_keeps_the_connection_and_the_loaded_area() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let spawn = ChunkPos::from(feet(&alice));
    wait_for_the_square(&alice, "the area around spawn", spawn, VIEW_DISTANCE).await;
    alice.sync().await;
    let position = alice.position();
    alice.clear();

    let mut last = LookDirection::default();
    for step in 0..=24u8 {
        last = LookDirection::new(f32::from(step) * 15.0, f32::from(step % 5) * 20.0 - 40.0);
        alice
            .send(ServerboundMovePlayerRot {
                look_direction: last,
                flags: MoveFlags {
                    on_ground: true,
                    horizontal_collision: false,
                },
            })
            .await;
    }
    alice.sync().await;

    let entity = server.player_entity(alice.uuid()).await;
    let rotation = server
        .with_world(move |world| *world.get::<Rotation>(entity).expect("players turn"))
        .await;
    assert_eq!(
        (rotation.yaw, rotation.pitch),
        (last.y_rot(), last.x_rot()),
        "the server follows the turn"
    );
    assert_eq!(server_position(&server, &alice).await, position);

    let trace = trace(&alice);
    assert!(
        trace.iter().all(|seen| matches!(seen, Seen::KeepAlive)),
        "turning on the spot changed the loaded area or moved the player: {trace:?}"
    );
    assert_eq!(alice.view(held), square(spawn, VIEW_DISTANCE));
    assert_eq!(ground(&alice).y, feet(&alice).y - 1);

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn walking_away_and_back_streams_terrain_and_keeps_the_world_intact() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let entity = server.player_entity(alice.uuid()).await;
    let start = alice.position();
    let spawn = ChunkPos::from(BlockPos::from(start));
    wait_for_the_square(&alice, "the area around spawn", spawn, VIEW_DISTANCE).await;
    alice.sync().await;
    let chunk_radius = login_chunk_radius(&trace(&alice));
    let first_seen: HashMap<ChunkPos, Vec<_>> = alice.view(|view| {
        view.chunks
            .iter()
            .map(|(pos, chunk)| (*pos, chunk.sections.to_vec()))
            .collect()
    });
    alice.clear();
    server
        .with_world(|world| {
            let mut ticker = world.resource_mut::<KeepAliveTicker>();
            ticker.interval_ticks = 2;
            ticker.ticks_since_last = 0;
        })
        .await;

    let away = Vec3::new(start.x + 40.0, start.y, start.z);
    walk(&alice, start, away).await;
    let far = ChunkPos::from(BlockPos::from(away));
    assert!(far.x - spawn.x >= 2, "40 blocks east crosses two chunks");
    wait_for_the_square(&alice, "the terrain ahead", far, VIEW_DISTANCE).await;
    assert_eq!(
        server_position(&server, &alice).await,
        away,
        "the server accepted the walk"
    );
    let new_terrain: Vec<ChunkPos> = square(far, VIEW_DISTANCE)
        .difference(&square(spawn, VIEW_DISTANCE))
        .copied()
        .collect();
    assert!(!new_terrain.is_empty());
    let bare: Vec<ChunkPos> = alice.view(|view| {
        new_terrain
            .iter()
            .copied()
            .filter(|chunk| !has_terrain(view, *chunk))
            .collect()
    });
    assert!(bare.is_empty(), "streamed chunks without terrain: {bare:?}");
    let keep_alives_out = keep_alives(&trace(&alice));

    walk(&alice, away, start).await;
    wait_for_the_square(&alice, "the area around spawn again", spawn, VIEW_DISTANCE).await;
    alice.sync().await;
    assert_eq!(server_position(&server, &alice).await, start);

    let trace = trace(&alice);
    let teleports: Vec<&Seen> = trace
        .iter()
        .filter(|seen| matches!(seen, Seen::Teleport(_)))
        .collect();
    assert!(
        teleports.is_empty(),
        "the server pulled the walking player back: {teleports:?}"
    );
    assert_chunks_land_in_framed_batches(&trace, chunk_radius, Some(spawn));
    // The server sends the next keep-alive only once the previous answer is
    // accepted, so two or more each way prove the exchange kept going.
    let keep_alives_back = keep_alives(&trace) - keep_alives_out;
    assert!(
        keep_alives_out >= 2 && keep_alives_back >= 2,
        "keep-alives stalled while chunks streamed: {keep_alives_out} out, {keep_alives_back} back"
    );
    server
        .with_world(|world| world.resource_mut::<KeepAliveTicker>().interval_ticks = u32::MAX)
        .await;
    server
        .wait_for("the last keep-alive answer to be accepted", move |world| {
            let state = world.get::<KeepAliveState>(entity)?;
            (!state.awaiting_response).then_some(())
        })
        .await;

    alice.view(|view| {
        assert_eq!(held(view), square(spawn, VIEW_DISTANCE));
        for (pos, chunk) in &view.chunks {
            assert!(
                first_seen.get(pos).map(Vec::as_slice) == Some(&chunk.sections[..]),
                "chunk {pos:?} differs from when it was first received"
            );
        }
    });
    assert_eq!(ground(&alice).y, feet(&alice).y - 1);

    acknowledge_batches(&alice, &trace).await;
    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}
