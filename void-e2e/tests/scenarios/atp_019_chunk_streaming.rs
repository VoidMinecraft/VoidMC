//! VMC-ATP-019: chunks stream around a moving player.
//!
//! Every scenario replays the chunk packets its bot received, in order, and
//! checks the streaming rules on each one: a chunk arrives within the
//! effective view distance of the chunk-cache centre it was sent under, is
//! never sent while the client still holds it, and comes nearest-first
//! (squared Euclidean distance from the centre, the order of
//! `ChunkPos::chunks_in_radius`); a forget only ever names a held chunk that
//! is out of range. Chunk batch framing packets, if any, are ignored.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use voidmc::components::ChunkSendBudget;
use voidmc_e2e::block::BlockState;
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, ChunkPos, Vec3};
use voidmc_e2e::protocol::common::client_information::ClientInformation;
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_client_information::ServerboundClientInformation,
    s_player_action::{Action, ServerboundPlayerAction},
};
use voidmc_e2e::{Bot, BotOptions, TestServer};

use crate::{feet, ground};

type Bytes = Arc<Box<[u8]>>;

enum ChunkEvent {
    Center(ChunkPos),
    Chunk(ChunkPos, Bytes),
    Forget(ChunkPos),
}

fn chunk_events(bot: &Bot) -> Vec<ChunkEvent> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::SetChunkCacheCenter(center) => {
            Some(ChunkEvent::Center(ChunkPos::new(center.x, center.z)))
        }
        ClientboundGamePacket::LevelChunkWithLight(chunk) => Some(ChunkEvent::Chunk(
            ChunkPos::new(chunk.x, chunk.z),
            chunk.chunk_data.data.clone(),
        )),
        ClientboundGamePacket::ForgetLevelChunk(forget) => Some(ChunkEvent::Forget(forget.pos)),
        _ => None,
    })
}

fn square(center: ChunkPos, radius: i32) -> HashSet<ChunkPos> {
    (-radius..=radius)
        .flat_map(|dx| {
            (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
        })
        .collect()
}

fn chebyshev(a: ChunkPos, b: ChunkPos) -> i32 {
    (a.x - b.x).abs().max((a.z - b.z).abs())
}

fn squared_distance(a: ChunkPos, b: ChunkPos) -> i32 {
    (a.x - b.x).pow(2) + (a.z - b.z).pow(2)
}

fn held(bot: &Bot) -> HashSet<ChunkPos> {
    bot.view(|view| view.chunks.keys().copied().collect())
}

/// The block position in the middle of a chunk, at height `y`.
fn middle_of(chunk: ChunkPos, y: f64) -> Vec3 {
    Vec3::new(f64::from(chunk.x * 16 + 8), y, f64::from(chunk.z * 16 + 8))
}

/// What one call to [`Stream::replay`] saw.
#[derive(Default)]
struct Replayed {
    sent: Vec<ChunkPos>,
    forgotten: Vec<ChunkPos>,
    centres: Vec<ChunkPos>,
}

/// The chunk state of a client rebuilt from the packets it received.
struct Stream {
    view_distance: i32,
    center: Option<ChunkPos>,
    held: HashSet<ChunkPos>,
    first_sent: HashMap<ChunkPos, Bytes>,
    resent_changed: Vec<ChunkPos>,
    last_distance: Option<i32>,
    interrupted: usize,
    seen: usize,
}

impl Stream {
    fn new(view_distance: i32) -> Self {
        Self {
            view_distance,
            center: None,
            held: HashSet::new(),
            first_sent: HashMap::new(),
            resent_changed: Vec::new(),
            last_distance: None,
            interrupted: 0,
            seen: 0,
        }
    }

    /// The effective view distance changes from now on: the chunks it brings
    /// in start a new nearest-first sequence.
    fn set_view_distance(&mut self, view_distance: i32) {
        self.view_distance = view_distance;
        self.last_distance = None;
    }

    /// Replays the chunk packets received since the previous call into
    /// `replayed`.
    fn replay(&mut self, bot: &Bot, replayed: &mut Replayed) {
        let events = chunk_events(bot);
        for event in events.into_iter().skip(self.seen) {
            self.seen += 1;
            match event {
                ChunkEvent::Center(center) => {
                    if let Some(previous) = self.center
                        && self.held != square(previous, self.view_distance)
                    {
                        self.interrupted += 1;
                    }
                    self.center = Some(center);
                    self.last_distance = None;
                    replayed.centres.push(center);
                }
                ChunkEvent::Chunk(pos, data) => {
                    let center = self
                        .center
                        .unwrap_or_else(|| panic!("chunk {pos:?} arrived before any centre"));
                    assert!(
                        chebyshev(pos, center) <= self.view_distance,
                        "chunk {pos:?} arrived outside view distance {} of centre {center:?}",
                        self.view_distance
                    );
                    assert!(
                        self.held.insert(pos),
                        "chunk {pos:?} re-sent while the client still holds it"
                    );
                    let distance = squared_distance(pos, center);
                    if let Some(last) = self.last_distance {
                        assert!(
                            distance >= last,
                            "chunk {pos:?} (squared distance {distance}) arrived after a farther \
                             one (squared distance {last}) around centre {center:?}"
                        );
                    }
                    self.last_distance = Some(distance);
                    match self.first_sent.get(&pos) {
                        Some(first) if **first != *data => self.resent_changed.push(pos),
                        Some(_) => {}
                        None => {
                            self.first_sent.insert(pos, data);
                        }
                    }
                    replayed.sent.push(pos);
                }
                ChunkEvent::Forget(pos) => {
                    assert!(
                        self.held.remove(&pos),
                        "chunk {pos:?} forgotten while not held"
                    );
                    if let Some(center) = self.center {
                        assert!(
                            chebyshev(pos, center) > self.view_distance,
                            "chunk {pos:?} forgotten while still within view distance {} of \
                             centre {center:?}",
                            self.view_distance
                        );
                    }
                    replayed.forgotten.push(pos);
                }
            }
        }
    }
}

/// Replays what `bot` receives, one server tick at a time so that every
/// packet is checked as it arrives, until it holds exactly the square of
/// `radius` around `target` with the cache centre on it.
async fn settle(bot: &Bot, stream: &mut Stream, target: ChunkPos, radius: i32) -> Replayed {
    let expected = square(target, radius);
    let mut replayed = Replayed::default();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            bot.sync().await;
            stream.replay(bot, &mut replayed);
            if stream.center == Some(target) && stream.held == expected {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out waiting for {} to hold the chunks around {target:?}",
            bot.name()
        )
    });
    assert_eq!(held(bot), expected);
    assert_eq!(bot.view(|view| view.chunk_center), Some(target));
    replayed
}

fn as_set(chunks: &[ChunkPos]) -> HashSet<ChunkPos> {
    let set: HashSet<ChunkPos> = chunks.iter().copied().collect();
    assert_eq!(
        set.len(),
        chunks.len(),
        "a chunk was listed twice: {chunks:?}"
    );
    set
}

async fn set_view_distance(bot: &Bot, view_distance: u8) {
    bot.send(ServerboundClientInformation {
        client_information: ClientInformation {
            view_distance,
            ..Default::default()
        },
    })
    .await;
}

#[tokio::test]
async fn the_join_sends_only_the_capped_view_distance() {
    let server = TestServer::builder()
        .config(|config| config.view_distance(3).initial_chunk_radius(4))
        .start()
        .await;

    // The client asks for more than the server allows, then for less.
    for (name, requested, effective) in [("Alice", 8, 3), ("Bob", 2, 2)] {
        let bot = server
            .join_with(
                name,
                BotOptions {
                    view_distance: requested,
                },
            )
            .await;
        let spawn = ChunkPos::from(feet(&bot));
        let (radius, simulation) = bot
            .expect("the login", |packet| match packet {
                ClientboundGamePacket::Login(login) => {
                    Some((login.chunk_radius, login.simulation_distance))
                }
                _ => None,
            })
            .await;
        assert_eq!(radius, 3, "Login carries the server view distance");
        assert_eq!(
            simulation, 4,
            "Login carries the server simulation distance"
        );

        let mut stream = Stream::new(effective);
        let replayed = settle(&bot, &mut stream, spawn, effective).await;
        assert!(
            replayed.forgotten.is_empty(),
            "{name} was sent chunks it then had to forget: {:?}",
            replayed.forgotten
        );
        assert_eq!(as_set(&replayed.sent), square(spawn, effective));
        bot.assert_healthy();
        bot.disconnect().await;
    }
    server.stop().await;
}

#[tokio::test]
async fn a_view_distance_change_mid_session_shrinks_and_grows_the_square() {
    let server = TestServer::builder()
        .config(|config| config.view_distance(4))
        .start()
        .await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 4 })
        .await;
    let spawn = ChunkPos::from(feet(&alice));
    let mut stream = Stream::new(4);
    settle(&alice, &mut stream, spawn, 4).await;

    // (requested, effective, previous effective)
    for (requested, effective, previous) in [(2, 2, 4), (12, 4, 2), (3, 3, 4)] {
        set_view_distance(&alice, requested).await;
        stream.set_view_distance(effective);
        let replayed = settle(&alice, &mut stream, spawn, effective).await;
        let (wider, narrower) = if effective > previous {
            (effective, previous)
        } else {
            (previous, effective)
        };
        let ring: HashSet<ChunkPos> = square(spawn, wider)
            .difference(&square(spawn, narrower))
            .copied()
            .collect();
        if effective < previous {
            assert!(
                replayed.sent.is_empty(),
                "shrinking sent {:?}",
                replayed.sent
            );
            assert_eq!(as_set(&replayed.forgotten), ring);
        } else {
            assert!(replayed.forgotten.is_empty());
            assert_eq!(as_set(&replayed.sent), ring);
        }
        assert!(replayed.centres.is_empty(), "the centre did not move");
    }

    // The new distance keeps applying while the player moves.
    let target = ChunkPos::new(spawn.x + 1, spawn.z);
    alice.move_to(middle_of(target, alice.position().y)).await;
    let replayed = settle(&alice, &mut stream, target, 3).await;
    assert_eq!(replayed.sent.len(), 7);
    assert_eq!(replayed.forgotten.len(), 7);

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn crossing_boundaries_straight_diagonally_and_in_one_jump() {
    let server = TestServer::builder()
        .config(|config| config.view_distance(4))
        .start()
        .await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 2 })
        .await;
    let spawn = ChunkPos::from(feet(&alice));
    let y = alice.position().y;
    let mut stream = Stream::new(2);
    settle(&alice, &mut stream, spawn, 2).await;

    // Walk diagonally, half a chunk per step, across two boundaries on
    // each axis.
    let mut position = alice.position();
    for _ in 0..4 {
        position = Vec3::new(position.x + 8.0, y, position.z + 8.0);
        alice.move_to(position).await;
    }
    let diagonal = ChunkPos::new(spawn.x + 2, spawn.z + 2);
    assert_eq!(ChunkPos::from(BlockPos::from(position)), diagonal);
    settle(&alice, &mut stream, diagonal, 2).await;
    ground(&alice);

    // One move that jumps several chunks: the whole square is replaced.
    let jump = ChunkPos::new(diagonal.x - 7, diagonal.z + 3);
    alice.move_to(middle_of(jump, y)).await;
    let replayed = settle(&alice, &mut stream, jump, 2).await;
    assert_eq!(replayed.centres, vec![jump]);
    assert_eq!(as_set(&replayed.forgotten), square(diagonal, 2));
    assert_eq!(as_set(&replayed.sent), square(jump, 2));
    ground(&alice);

    // Walk straight north across two boundaries, one chunk per move, and
    // check the centre and the square after each crossing.
    for step in 1..=2 {
        let target = ChunkPos::new(jump.x, jump.z - step);
        alice.move_to(middle_of(target, y)).await;
        let replayed = settle(&alice, &mut stream, target, 2).await;
        assert_eq!(replayed.centres, vec![target]);
        let entering: HashSet<ChunkPos> = (-2..=2)
            .map(|dx| ChunkPos::new(target.x + dx, target.z - 2))
            .collect();
        let leaving: HashSet<ChunkPos> = (-2..=2)
            .map(|dx| ChunkPos::new(target.x + dx, target.z + 3))
            .collect();
        assert_eq!(as_set(&replayed.sent), entering);
        assert_eq!(as_set(&replayed.forgotten), leaving);
    }
    ground(&alice);

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn moving_back_and_forth_across_one_boundary() {
    let server = TestServer::builder()
        .config(|config| config.view_distance(4))
        .start()
        .await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 2 })
        .await;
    let spawn = ChunkPos::from(feet(&alice));
    let start = alice.position();
    let mut stream = Stream::new(2);
    settle(&alice, &mut stream, spawn, 2).await;

    let west_x = f64::from(spawn.x * 16 + 15) + 0.5;
    let west = spawn;
    let east = ChunkPos::new(spawn.x + 1, spawn.z);
    let column = |x: i32| -> HashSet<ChunkPos> {
        (-2..=2).map(|dz| ChunkPos::new(x, spawn.z + dz)).collect()
    };
    for _ in 0..4 {
        for (x, target, entering, leaving) in [
            (west_x + 1.0, east, spawn.x + 3, spawn.x - 2),
            (west_x, west, spawn.x - 2, spawn.x + 3),
        ] {
            alice.move_to(Vec3::new(x, start.y, start.z)).await;
            let replayed = settle(&alice, &mut stream, target, 2).await;
            assert_eq!(replayed.centres, vec![target]);
            assert_eq!(as_set(&replayed.sent), column(entering));
            assert_eq!(as_set(&replayed.forgotten), column(leaving));
        }
    }
    assert!(
        stream.resent_changed.is_empty(),
        "re-sent chunks changed content: {:?}",
        stream.resent_changed
    );

    alice.assert_healthy();
    server.stop().await;
}

fn blocks_of(bot: &Bot, chunk: ChunkPos) -> Vec<Option<BlockState>> {
    bot.view(|view| {
        let mut blocks = Vec::with_capacity(16 * 16 * 384);
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..320 {
                    blocks.push(view.block_at(BlockPos::new(
                        chunk.x * 16 + x,
                        y,
                        chunk.z * 16 + z,
                    )));
                }
            }
        }
        blocks
    })
}

#[tokio::test]
async fn returning_to_a_forgotten_area_restores_it_once_with_its_changes() {
    let server = TestServer::builder()
        .config(|config| config.view_distance(4))
        .start()
        .await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 2 })
        .await;
    let spawn = ChunkPos::from(feet(&alice));
    let y = alice.position().y;
    let mut stream = Stream::new(2);
    settle(&alice, &mut stream, spawn, 2).await;

    let broken = ground(&alice);
    assert_eq!(ChunkPos::from(broken), spawn);
    alice
        .send(ServerboundPlayerAction {
            action: Action::StartDestroyBlock,
            pos: broken,
            direction: Direction::Up,
            seq: 1,
        })
        .await;
    alice
        .wait_until("the broken block turn to air", |view| {
            view.block_at(broken).is_some_and(|state| state.is_air())
        })
        .await;
    alice.sync().await;
    let before = blocks_of(&alice, spawn);

    // Out three chunks east (two columns stay held), then back.
    let near = ChunkPos::new(spawn.x + 3, spawn.z);
    alice.move_to(middle_of(near, y)).await;
    settle(&alice, &mut stream, near, 2).await;
    alice.move_to(middle_of(spawn, y)).await;
    let replayed = settle(&alice, &mut stream, spawn, 2).await;
    let still_held: HashSet<ChunkPos> = square(near, 2)
        .intersection(&square(spawn, 2))
        .copied()
        .collect();
    assert_eq!(still_held.len(), 10);
    assert_eq!(
        as_set(&replayed.sent),
        square(spawn, 2).difference(&still_held).copied().collect(),
        "only the chunks forgotten on the way out come back, once each"
    );

    // Far enough that every chunk is forgotten, then back to the origin.
    let far = ChunkPos::new(spawn.x + 6, spawn.z);
    alice.move_to(middle_of(far, y)).await;
    let replayed = settle(&alice, &mut stream, far, 2).await;
    assert_eq!(as_set(&replayed.forgotten), square(spawn, 2));
    alice.move_to(middle_of(spawn, y)).await;
    let replayed = settle(&alice, &mut stream, spawn, 2).await;
    assert_eq!(as_set(&replayed.sent), square(spawn, 2));

    assert!(alice.block_at(broken).is_some_and(|state| state.is_air()));
    assert!(
        blocks_of(&alice, spawn) == before,
        "the re-sent spawn chunk differs from the one the client forgot"
    );
    assert_eq!(
        stream.resent_changed,
        vec![spawn, spawn],
        "only the chunk with the broken block comes back with new content, on both returns"
    );

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn a_send_budget_streams_nearest_first_and_drops_what_falls_out_of_range() {
    let server = TestServer::builder()
        .config(|config| config.view_distance(4))
        .start()
        .await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 4 })
        .await;
    let spawn = ChunkPos::from(feet(&alice));
    let y = alice.position().y;
    let mut stream = Stream::new(4);
    settle(&alice, &mut stream, spawn, 4).await;

    let player = server.player_entity(alice.uuid()).await;
    server
        .with_world(move |world| {
            world.entity_mut(player).insert(ChunkSendBudget(1));
        })
        .await;

    // Each move lands while the previous backlog is still pending.
    let mut target = spawn;
    for (dx, dz) in [(3, 0), (2, 3), (-1, 3), (6, 0)] {
        target = ChunkPos::new(target.x + dx, target.z + dz);
        alice.move_to(middle_of(target, y)).await;
        alice
            .wait_until("the new chunk-cache centre", |view| {
                view.chunk_center == Some(target)
            })
            .await;
    }
    settle(&alice, &mut stream, target, 4).await;
    assert!(
        stream.interrupted >= 3,
        "moves landed after the backlog drained ({} interrupted)",
        stream.interrupted
    );

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn the_generation_cap_keeps_cached_chunks_behind_nearer_ungenerated_ones() {
    // Chunks within 4 of spawn are generated at startup; the rest one per tick.
    let server = TestServer::builder()
        .config(|config| {
            config
                .view_distance(3)
                .initial_chunk_radius(3)
                .spawn_chunk_radius(4)
                .max_chunk_generations_per_tick(1)
        })
        .start()
        .await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 3 })
        .await;
    let spawn = ChunkPos::from(feet(&alice));
    assert_eq!(spawn, ChunkPos::new(0, 0));
    let mut stream = Stream::new(3);
    settle(&alice, &mut stream, spawn, 3).await;

    // Around chunk (4, 0) the cached column x = 4 mixes with fresh chunks
    // beyond it: (5, ±1) must not be overtaken by the cached (4, ±2).
    let target = ChunkPos::new(4, 0);
    alice.move_to(middle_of(target, alice.position().y)).await;
    let replayed = settle(&alice, &mut stream, target, 3).await;
    assert_eq!(replayed.sent.len(), 28);

    alice.assert_healthy();
    server.stop().await;
}
