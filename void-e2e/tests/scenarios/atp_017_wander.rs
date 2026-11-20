//! VMC-ATP-017: `/summon <entity> --wander` attaches the demo random walk to a
//! non-player entity; an entity summoned without the flag does not get it.
//!
//! The walk is random (direction and timer), so the scenarios record what the
//! server holds every tick and check properties that hold for every draw: the
//! documented speed and timer, the facing, the direction changes, and that
//! every watching client replays the same path from the packets it received.
//!
//! The same walk is reached through the public `EntityBuilder::wander(true)`,
//! a wanderer that keeps its heading is sent position-only moves, and one
//! pressed against a wall on an axis-aligned heading is sent no more moves.

use std::collections::{BTreeMap, HashSet};

use bevy_app::Last;
use bevy_ecs::prelude::*;
use voidmc::components::{
    MinecraftEntityId, MovementConfig, Position, Rotation, SpawnedEntity, Velocity, Wander,
};
use voidmc::world::{DimensionId, set_block_state};
use voidmc::{BlockPosition, EntityBuilder, EntityKind as VoidKind};
use voidmc_e2e::core::position::Vec3;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::registry::builtin::EntityKind;
use voidmc_e2e::{Bot, TestServer};
use voidmc_protocol::clientbound::chunk::blocks;

use crate::feet;

/// Walking speed `/summon --wander` gives the entity, in blocks per tick.
const SPEED: f64 = 0.08;
/// Ticks a heading lasts before `/summon --wander` draws a new one.
const TIMER: std::ops::Range<i32> = 40..140;
/// Direction changes each wanderer must make in the scenarios that watch it
/// walk a whole random walk.
const TURNS: usize = 4;
/// One fixed-point unit of the relative move packets, in blocks.
const UNIT: f64 = 1.0 / 4096.0;

/// What the server held for one entity at the end of one tick.
#[derive(Clone, Copy, Debug)]
struct Sample {
    position: [f64; 3],
    yaw: f32,
    velocity: [f64; 3],
    flagged: bool,
    /// `(ticks, speed, yaw)` of the `Wander` component, if any.
    wander: Option<(i32, f64, f32)>,
}

/// Every tick's [`Sample`] of every non-player entity, by network id.
#[derive(Resource, Default)]
struct Trace(BTreeMap<i32, Vec<Sample>>);

type Sampled<'a> = (
    &'a MinecraftEntityId,
    &'a Position,
    &'a Rotation,
    &'a Velocity,
    &'a MovementConfig,
    Option<&'a Wander>,
);

fn record(mut trace: ResMut<Trace>, entities: Query<Sampled, With<SpawnedEntity>>) {
    for (id, position, rotation, velocity, movement, wander) in &entities {
        trace.0.entry(id.0).or_default().push(Sample {
            position: [position.x, position.y, position.z],
            yaw: rotation.yaw,
            velocity: [velocity.x, velocity.y, velocity.z],
            flagged: movement.wander,
            wander: wander.map(|wander| (wander.ticks, wander.speed, wander.yaw)),
        });
    }
}

async fn traced_server() -> TestServer {
    TestServer::builder()
        .operators()
        .plugin(|app| {
            app.init_resource::<Trace>().add_systems(Last, record);
        })
        .start()
        .await
}

async fn trace(server: &TestServer) -> BTreeMap<i32, Vec<Sample>> {
    server
        .with_world(|world| world.resource::<Trace>().0.clone())
        .await
}

/// How many times the wander timer ran out and a new heading was drawn.
fn turns(samples: &[Sample]) -> usize {
    samples
        .windows(2)
        .filter(|pair| pair[0].wander.is_some_and(|(ticks, _, _)| ticks == 1))
        .count()
}

/// Waits until every entity in `ids` changed direction `count` times.
async fn wait_for_turns(server: &TestServer, ids: &[i32], count: usize) {
    // A heading lasts at most `TIMER.end - 1` ticks, so this many are enough
    // for any draw.
    let bound = (count as u64 + 1) * TIMER.end as u64;
    let mut waited = 0;
    loop {
        let trace = trace(server).await;
        let done = ids
            .iter()
            .all(|id| trace.get(id).is_some_and(|samples| turns(samples) >= count));
        if done {
            return;
        }
        assert!(
            waited <= bound,
            "no {count} direction changes after {waited} ticks"
        );
        server.wait_ticks(20).await;
        waited += 20;
    }
}

/// Checks the server side of a walk: every tick the entity moves `SPEED`
/// blocks horizontally, faces where it goes, and keeps its heading until the
/// documented timer runs out. Returns the headings it walked in.
fn check_walk(id: i32, samples: &[Sample]) -> Vec<f32> {
    let first = samples[0];
    let (ticks, speed, _) = first
        .wander
        .unwrap_or_else(|| panic!("entity {id} has no Wander component"));
    assert!(first.flagged, "entity {id}: MovementConfig.wander is off");
    assert_eq!(speed, SPEED, "entity {id}: wander speed");
    // Drawn from `TIMER`, then counted down once in the tick of the summon.
    assert!(
        (TIMER.start - 1..TIMER.end - 1).contains(&ticks),
        "entity {id}: first timer {ticks}"
    );

    let mut headings = vec![first.yaw];
    for (tick, pair) in samples.windows(2).enumerate() {
        let (before, after) = (pair[0], pair[1]);
        let (old_ticks, _, old_yaw) = before.wander.unwrap();
        let (ticks, speed, yaw) = after
            .wander
            .unwrap_or_else(|| panic!("entity {id} lost its Wander component"));
        assert_eq!(speed, SPEED, "entity {id}, tick {tick}: wander speed");
        assert!(after.flagged, "entity {id}: MovementConfig.wander is off");
        if old_ticks == 1 {
            assert!(
                TIMER.contains(&ticks),
                "entity {id}, tick {tick}: new timer {ticks}"
            );
            headings.push(after.yaw);
        } else {
            assert_eq!(ticks, old_ticks - 1, "entity {id}, tick {tick}: timer");
            assert_eq!(yaw, old_yaw, "entity {id}, tick {tick}: turned early");
        }

        let dx = after.position[0] - before.position[0];
        let dy = after.position[1] - before.position[1];
        let dz = after.position[2] - before.position[2];
        assert_eq!(dy, 0.0, "entity {id}, tick {tick}: no vertical motion");
        // The heading is an `f32` angle: allow for its rounding.
        assert!(
            (dx.hypot(dz) - SPEED).abs() < 1e-6,
            "entity {id}, tick {tick}: moved {} blocks",
            dx.hypot(dz)
        );
        assert!(
            (after.velocity[0] - dx).abs() < 1e-9
                && after.velocity[1] == 0.0
                && (after.velocity[2] - dz).abs() < 1e-9,
            "entity {id}, tick {tick}: velocity {:?} for a move of ({dx}, {dz})",
            after.velocity
        );
        // Minecraft yaw: 0 faces +Z, 90 faces -X.
        let facing = f64::from(after.yaw).to_radians();
        assert!(
            (dx + facing.sin() * SPEED).abs() < 1e-6 && (dz - facing.cos() * SPEED).abs() < 1e-6,
            "entity {id}, tick {tick}: faces {} but moved ({dx}, {dz})",
            after.yaw
        );
    }
    headings
}

/// One packet a client received about one entity.
#[derive(Debug)]
enum Seen {
    Add { position: Vec3, yaw: i8 },
    Remove,
    Move { delta: [i16; 3], yaw: Option<i8> },
    Sync { position: Vec3, yaw: f32 },
    Turn { yaw: i8 },
    Head { yaw: i8 },
    Motion { velocity: Vec3 },
    Teleport,
}

fn seen_packets(bot: &Bot, id: i32) -> Vec<Seen> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::AddEntity(p) if p.id.0 == id => Some(Seen::Add {
            position: p.position,
            yaw: p.y_rot,
        }),
        ClientboundGamePacket::RemoveEntities(p) if p.entity_ids.iter().any(|e| e.0 == id) => {
            Some(Seen::Remove)
        }
        ClientboundGamePacket::MoveEntityPos(p) if p.entity_id.0 == id => Some(Seen::Move {
            delta: [p.delta.xa, p.delta.ya, p.delta.za],
            yaw: None,
        }),
        ClientboundGamePacket::MoveEntityPosRot(p) if p.entity_id.0 == id => Some(Seen::Move {
            delta: [p.delta.xa, p.delta.ya, p.delta.za],
            yaw: Some(p.look_direction.y_rot),
        }),
        ClientboundGamePacket::EntityPositionSync(p) if p.id.0 == id => Some(Seen::Sync {
            position: p.values.pos,
            yaw: p.values.look_direction.y_rot(),
        }),
        ClientboundGamePacket::MoveEntityRot(p) if p.entity_id.0 == id => Some(Seen::Turn {
            yaw: p.look_direction.y_rot,
        }),
        ClientboundGamePacket::RotateHead(p) if p.entity_id.0 == id => {
            Some(Seen::Head { yaw: p.y_head_rot })
        }
        ClientboundGamePacket::SetEntityMotion(p) if p.id.0 == id => Some(Seen::Motion {
            velocity: p.delta.to_vec3(),
        }),
        ClientboundGamePacket::TeleportEntity(p) if p.id.0 == id => Some(Seen::Teleport),
        _ => None,
    })
}

/// A yaw as a fraction of a turn, in protocol angle steps (256 per turn).
fn steps(degrees: f64) -> f64 {
    (degrees / 360.0 * 256.0).rem_euclid(256.0)
}

/// The distance between two angles in protocol steps, across the wrap.
fn step_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(256.0);
    d.min(256.0 - d)
}

/// The yaw a protocol angle byte stands for, as the middle of its step.
fn byte_steps(byte: i8) -> f64 {
    f64::from(byte as u8) + 0.5
}

fn assert_close(what: &str, client: Vec3, server: [f64; 3], tolerance: f64) {
    let error = [
        client.x - server[0],
        client.y - server[1],
        client.z - server[2],
    ];
    assert!(
        error.iter().all(|e| e.abs() <= tolerance),
        "{what}: client at {client:?}, server at {server:?}"
    );
}

/// What a client made of an entity, replayed from its packets.
#[derive(Debug, Default)]
struct Replay {
    adds: usize,
    removes: usize,
    moves: usize,
    /// The trace index of the last tick the client followed.
    reached: usize,
    /// Where the client last saw the entity, while it is visible.
    position: Option<Vec3>,
    /// The trace index of every tick in which the client was sent a
    /// rotation for the entity: a move-and-rotate, rotate or head packet.
    rotations: Vec<usize>,
}

/// Replays every packet `bot` received about the entity `id` like a vanilla
/// client, and checks each one against the server's tick-by-tick `samples`.
///
/// A spawn must carry the server position of some tick exactly. A walking
/// entity moves every tick, so each later position update is the next tick:
/// the accumulated fixed-point deltas must stay within rounding of the
/// server position however far the entity walks, each step must cover
/// `SPEED` blocks, and the yaw the client holds must face along the step.
fn replay(bot: &Bot, id: i32, samples: &[Sample]) -> Replay {
    let who = format!("{} for entity {id}", bot.name());
    let mut replay = Replay::default();
    let mut index = 0;
    let mut yaw = 0.0;
    for packet in seen_packets(bot, id) {
        match packet {
            Seen::Add {
                position,
                yaw: byte,
            } => {
                assert!(replay.position.is_none(), "{who}: added while visible");
                index = (index..samples.len())
                    .find(|&i| {
                        let p = samples[i].position;
                        Vec3::new(p[0], p[1], p[2]) == position
                    })
                    .unwrap_or_else(|| {
                        panic!("{who}: spawned at {position:?}, where the server never was")
                    });
                yaw = byte_steps(byte);
                // Half a step for the byte, and a hair for float rounding.
                assert!(
                    step_distance(yaw, steps(f64::from(samples[index].yaw))) <= 0.51,
                    "{who}: spawned facing {byte}, server yaw {}",
                    samples[index].yaw
                );
                replay.position = Some(position);
                replay.adds += 1;
            }
            Seen::Remove => {
                assert!(replay.position.is_some(), "{who}: removed while hidden");
                replay.position = None;
                replay.removes += 1;
            }
            Seen::Move { delta, yaw: byte } => {
                let position = replay
                    .position
                    .unwrap_or_else(|| panic!("{who}: moved while hidden"));
                index += 1;
                assert!(index < samples.len(), "{who}: more moves than ticks");
                let [xa, ya, za] = delta.map(f64::from);
                let position = Vec3::new(
                    position.x + xa * UNIT,
                    position.y + ya * UNIT,
                    position.z + za * UNIT,
                );
                assert_close(
                    &format!("{who}, move {}", replay.moves),
                    position,
                    samples[index].position,
                    UNIT + 1e-9,
                );
                assert_eq!(ya, 0.0, "{who}: vertical step");
                // Each component is rounded to the unit, so the step length
                // is within one diagonal unit of the true one.
                assert!(
                    (xa.hypot(za) * UNIT - SPEED).abs() <= 1.5 * UNIT,
                    "{who}: stepped {} blocks",
                    xa.hypot(za) * UNIT
                );
                if let Some(byte) = byte {
                    yaw = byte_steps(byte);
                    replay.rotations.push(index);
                }
                // The step direction is off by at most atan(1.5 / 327) from
                // the true heading (0.19 steps), the byte by half a step.
                let travel = steps((-xa).atan2(za).to_degrees());
                assert!(
                    step_distance(yaw, travel) <= 0.75,
                    "{who}, move {}: faces step {yaw} but travels toward step {travel}",
                    replay.moves
                );
                replay.position = Some(position);
                replay.moves += 1;
                replay.reached = index;
            }
            Seen::Sync {
                position,
                yaw: degrees,
            } => {
                assert!(replay.position.is_some(), "{who}: synced while hidden");
                index += 1;
                assert!(index < samples.len(), "{who}: more moves than ticks");
                assert_close(&who, position, samples[index].position, 0.0);
                yaw = steps(f64::from(degrees));
                replay.position = Some(position);
                replay.moves += 1;
                replay.reached = index;
            }
            Seen::Turn { yaw: byte } => {
                yaw = byte_steps(byte);
                replay.rotations.push(index);
            }
            Seen::Head { yaw: byte } => {
                replay.rotations.push(index);
                assert!(replay.position.is_some(), "{who}: head turned while hidden");
                assert!(
                    step_distance(byte_steps(byte), yaw) <= 0.51,
                    "{who}: head at {byte} while the body faces step {yaw}"
                );
            }
            Seen::Motion { velocity } => {
                assert!(replay.position.is_some(), "{who}: pushed while hidden");
                assert!(
                    velocity.y.abs() < 1e-3 && (velocity.x.hypot(velocity.z) - SPEED).abs() < 1e-3,
                    "{who}: velocity {velocity:?}"
                );
            }
            Seen::Teleport => panic!("{who}: teleported"),
        }
    }
    replay
}

/// The ids of every pig the client knows, waiting until it knows `count`.
async fn pigs(bot: &Bot, count: usize) -> HashSet<i32> {
    bot.wait_until(&format!("{count} pigs"), |view| {
        view.entities
            .values()
            .filter(|entity| entity.kind == EntityKind::Pig)
            .count()
            == count
    })
    .await;
    bot.view(|view| {
        view.entities
            .iter()
            .filter(|(_, entity)| entity.kind == EntityKind::Pig)
            .map(|(id, _)| *id)
            .collect()
    })
}

/// Summons with `command` and returns the id of the new pig.
async fn summon(bot: &Bot, command: &str, known: &mut HashSet<i32>) -> i32 {
    bot.command(command).await;
    let now = pigs(bot, known.len() + 1).await;
    let id = *now.difference(known).next().unwrap();
    known.insert(id);
    id
}

#[tokio::test]
async fn wandering_pig_walks_a_random_walk_that_watchers_follow() {
    let server = traced_server().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // Step 1, with the long and short flag, then step 3's control pig.
    let mut known = HashSet::new();
    let long = summon(&alice, "summon minecraft:pig --wander", &mut known).await;
    let short = summon(&alice, "summon minecraft:pig -w", &mut known).await;
    let still = summon(&alice, "summon minecraft:pig", &mut known).await;
    pigs(&bob, 3).await;

    // Step 2: watch until both wanderers have changed direction a few times.
    wait_for_turns(&server, &[long, short], TURNS).await;
    alice.sync().await;
    bob.sync().await;
    let trace = trace(&server).await;

    for id in [long, short] {
        let samples = &trace[&id];
        let headings = check_walk(id, samples);
        assert!(headings.len() > TURNS, "entity {id}: headings {headings:?}");
        assert!(
            headings.iter().any(|&yaw| yaw != headings[0]),
            "entity {id} never changed heading: {headings:?}"
        );
        // The tick of the last turn `wait_for_turns` waited for, before the
        // sync: the clients must have followed the walk past it.
        let last_turn = samples
            .iter()
            .enumerate()
            .filter(|(_, sample)| sample.wander.is_some_and(|(ticks, _, _)| ticks == 1))
            .nth(TURNS - 1)
            .map(|(index, _)| index + 1)
            .unwrap();
        for bot in [&alice, &bob] {
            let seen = replay(bot, id, samples);
            assert_eq!((seen.adds, seen.removes), (1, 0), "{}", bot.name());
            assert!(
                seen.reached >= last_turn,
                "{} followed entity {id} to tick {}, not to its turn at tick {last_turn}",
                bot.name(),
                seen.reached
            );
            let shown = bot.view(|view| view.entities.get(&id).map(|entity| entity.position));
            assert_eq!(shown, seen.position, "{}: client view", bot.name());
        }
    }

    // Step 3: the pig summoned without the flag has no walk and was sent
    // nothing after its spawn, over the same ticks.
    let samples = &trace[&still];
    assert!(samples.len() > 2 * TIMER.start as usize);
    for sample in samples {
        assert!(!sample.flagged, "MovementConfig.wander on the plain pig");
        assert!(sample.wander.is_none(), "Wander on the plain pig");
        assert_eq!(sample.position, samples[0].position, "the plain pig moved");
        assert_eq!(sample.velocity, [0.0; 3], "the plain pig was pushed");
    }
    for bot in [&alice, &bob] {
        let seen = seen_packets(bot, still);
        assert!(
            matches!(seen.as_slice(), [Seen::Add { .. }]),
            "{} got {seen:?} for the plain pig",
            bot.name()
        );
        let shown = bot.view(|view| view.entities.get(&still).map(|entity| entity.position));
        let [x, y, z] = samples[0].position;
        assert_eq!(shown, Some(Vec3::new(x, y, z)));
    }

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

/// Wanderers summoned together, at the summoner's feet.
const CROWD: usize = 32;
/// A chunk corner nobody has loaded.
const FAR: f64 = 4000.0;

#[tokio::test]
async fn many_wanderers_walk_unwatched_and_in_unloaded_chunks_without_errors() {
    let server = traced_server().await;
    let alice = server.join("Alice").await;
    let home = alice.position();

    for _ in 0..CROWD {
        alice.command("summon minecraft:pig --wander").await;
    }
    let crowd = pigs(&alice, CROWD).await;
    alice
        .command(&format!("summon minecraft:pig {FAR} 100 {FAR} --wander"))
        .await;
    let far = server
        .wait_for("the wanderer in unloaded chunks", |world| {
            world
                .query_filtered::<(&MinecraftEntityId, &Position), With<SpawnedEntity>>()
                .iter(world)
                .find(|(_, position)| position.x > FAR - 100.0)
                .map(|(id, _)| id.0)
        })
        .await;
    server.wait_ticks(20).await;
    alice.sync().await;
    assert!(
        seen_packets(&alice, far).is_empty(),
        "Alice was sent the far wanderer"
    );

    // Alice walks away: the crowd leaves her view and keeps walking unwatched,
    // the far wanderer comes into view where it has walked to.
    alice.command(&format!("tp {FAR} 100 {FAR}")).await;
    alice
        .wait_until("the far wanderer, without the crowd", |view| {
            view.entities.contains_key(&far)
                && crowd.iter().all(|id| !view.entities.contains_key(id))
        })
        .await;
    server.wait_ticks(20).await;

    // And back: the crowd comes back into view where it has walked to.
    alice
        .command(&format!("tp {} {} {}", home.x, home.y, home.z))
        .await;
    alice
        .wait_until("the crowd, without the far wanderer", |view| {
            !view.entities.contains_key(&far)
                && crowd.iter().all(|id| view.entities.contains_key(id))
        })
        .await;
    server.wait_ticks(20).await;
    alice.sync().await;
    let trace = trace(&server).await;

    // Watched or not, in loaded chunks or not, every wanderer kept walking.
    for id in crowd.iter().copied().chain([far]) {
        check_walk(id, &trace[&id]);
    }
    // Alice saw each one exactly while it was in her view, and each spawn and
    // step agreed with the server.
    for id in crowd.iter().copied() {
        let seen = replay(&alice, id, &trace[&id]);
        assert_eq!((seen.adds, seen.removes), (2, 1), "crowd pig {id}");
        let shown = alice.view(|view| view.entities.get(&id).map(|entity| entity.position));
        assert_eq!(shown, seen.position, "crowd pig {id}: client view");
    }
    let seen = replay(&alice, far, &trace[&far]);
    assert_eq!((seen.adds, seen.removes), (1, 1), "far pig");
    assert!(seen.moves > 0, "Alice never saw the far pig walk");

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn pig_built_with_wander_walks_the_summoned_random_walk() {
    let server = traced_server().await;
    let alice = server.join("Alice").await;
    let home = alice.position();

    // Through the public builder API on the game thread, not `/summon`.
    let id = server
        .with_world(move |world| {
            EntityBuilder::new(VoidKind::Pig)
                .at(home.x, home.y, home.z)
                .wander(true)
                .spawn_in(world)
                .get::<MinecraftEntityId>()
                .expect("a spawned entity has a network id")
                .0
        })
        .await;
    assert!(pigs(&alice, 1).await.contains(&id));

    // Alice sees it walk away from where it appeared.
    let appeared = alice.view(|view| view.entities[&id].position);
    alice
        .wait_until("the built pig walk a block", |view| {
            view.entities
                .get(&id)
                .is_some_and(|pig| pig.position.distance_to(appeared) >= 1.0)
        })
        .await;

    // It walks the walk `/summon --wander` gives: its speed and timer, facing
    // where it goes, and a new heading when the timer runs out.
    wait_for_turns(&server, &[id], 1).await;
    alice.sync().await;
    let samples = &trace(&server).await[&id];
    let headings = check_walk(id, samples);
    assert!(headings.len() > 1, "headings {headings:?}");
    let seen = replay(&alice, id, samples);
    assert_eq!((seen.adds, seen.removes), (1, 0));
    assert!(seen.moves > 0, "Alice never saw the built pig walk");
    let shown = alice.view(|view| view.entities.get(&id).map(|entity| entity.position));
    assert_eq!(shown, seen.position, "client view");

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn wanderer_keeping_its_heading_is_sent_moves_without_rotation() {
    let server = traced_server().await;
    let alice = server.join("Alice").await;
    let mut known = HashSet::new();
    let ids = [
        summon(&alice, "summon minecraft:pig --wander", &mut known).await,
        summon(&alice, "summon minecraft:pig --wander", &mut known).await,
    ];

    wait_for_turns(&server, &ids, 2).await;
    alice.sync().await;
    let trace = trace(&server).await;

    for id in ids {
        let samples = &trace[&id];
        check_walk(id, samples);
        let seen = replay(&alice, id, samples);
        // The ticks in which the server turned the entity.
        let turned: Vec<usize> = (1..samples.len())
            .filter(|&index| samples[index].yaw != samples[index - 1].yaw)
            .collect();
        assert!(!turned.is_empty(), "entity {id} never turned");
        for &index in &seen.rotations {
            assert!(
                turned.contains(&index),
                "Alice was sent a rotation for entity {id} in tick {index}, which kept \
                 the heading {} (it turned only in ticks {turned:?})",
                samples[index].yaw
            );
        }
        // Each turn Alice followed reached her as a move-and-rotate packet and
        // a head rotation.
        for &index in turned.iter().filter(|&&index| index <= seen.reached) {
            let sent = seen.rotations.iter().filter(|&&r| r == index).count();
            assert_eq!(
                sent, 2,
                "entity {id}: rotations sent for its turn in tick {index}"
            );
        }
    }

    alice.assert_healthy();
    server.stop().await;
}

/// Wander headings along the axes, in `wander_system`'s convention (0 walks
/// south, +z; 90 east, +x), with the axis each walks along (0 for x, 2 for
/// z) and its direction on it.
const CARDINALS: [(f32, usize, f64); 4] = [
    (0.0, 2, 1.0),
    (90.0, 0, 1.0),
    (180.0, 2, -1.0),
    (270.0, 0, -1.0),
];
/// Half the width of a pig's collision box.
const PIG_HALF_WIDTH: f64 = 0.45;
/// Blocks from the pigs' start block to the wall around it.
const RING: i32 = 3;

#[tokio::test]
async fn wanderer_pressed_against_a_wall_on_an_axis_is_sent_no_more_moves() {
    let server = traced_server().await;
    let alice = server.join("Alice").await;
    let feet = feet(&alice);
    let (x, y, z) = (feet.x, feet.y + 12, feet.z);

    // A ring wall in the air around the block (x, y, z), two blocks high so a
    // pig cannot step onto it.
    let placed = server
        .with_world(move |world| {
            let mut placed = 0;
            for dx in -RING..=RING {
                for dz in -RING..=RING {
                    if dx.abs() < RING && dz.abs() < RING {
                        continue;
                    }
                    for dy in 0..2 {
                        let position = BlockPosition {
                            x: x + dx,
                            y: (y + dy) as i16,
                            z: z + dz,
                        };
                        let old = set_block_state(
                            world,
                            DimensionId::Overworld,
                            position,
                            blocks::STONE,
                            None,
                        );
                        placed += usize::from(old.is_some());
                    }
                }
            }
            placed
        })
        .await;
    assert_eq!(
        placed,
        16 * RING as usize,
        "the wall was built in empty air"
    );

    // One pig per axis-aligned heading, built through the public API with
    // block checks and no gravity. The staged `Wander` points each one at its
    // wall for good, with the server's speed.
    let start = [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5];
    let ids: Vec<i32> = server
        .with_world(move |world| {
            CARDINALS
                .iter()
                .map(|&(yaw, ..)| {
                    EntityBuilder::new(VoidKind::Pig)
                        .at(start[0], start[1], start[2])
                        .block_collision(true)
                        .wander(true)
                        .with(Wander {
                            ticks: i32::MAX,
                            speed: SPEED,
                            yaw,
                        })
                        .spawn_in(world)
                        .get::<MinecraftEntityId>()
                        .expect("a spawned entity has a network id")
                        .0
                })
                .collect()
        })
        .await;

    // Each pig ends with its box against the wall's face, less than one step
    // short of it, without drifting along the wall.
    let reach = f64::from(RING) - 0.5 - PIG_HALF_WIDTH;
    let pressed = move |position: Vec3, axis: usize, sign: f64| {
        let position = [position.x, position.y, position.z];
        let across = 2 - axis;
        let gap = (start[axis] + sign * reach - position[axis]) * sign;
        (-UNIT..SPEED + UNIT).contains(&gap)
            && (position[across] - start[across]).abs() <= UNIT
            && (position[1] - start[1]).abs() <= UNIT
    };
    let watched = ids.clone();
    alice
        .wait_until("every pig pressed against the wall", move |view| {
            watched.iter().zip(CARDINALS).all(|(id, (_, axis, sign))| {
                view.entities
                    .get(id)
                    .is_some_and(|pig| pressed(pig.position, axis, sign))
            })
        })
        .await;

    // From then on a pressed pig does not move: no move or rotation packet
    // reaches Alice for it.
    server.wait_ticks(5).await;
    alice.sync().await;
    alice.clear();
    server.wait_ticks(40).await;
    alice.sync().await;
    let mut moving = Vec::new();
    for (id, (yaw, axis, sign)) in ids.iter().zip(CARDINALS) {
        let moves: Vec<Seen> = seen_packets(&alice, *id)
            .into_iter()
            .filter(|seen| !matches!(seen, Seen::Motion { .. }))
            .collect();
        if !moves.is_empty() {
            moving.push(format!(
                "heading {yaw}: {} packets, first ones {:?}",
                moves.len(),
                &moves[..moves.len().min(2)]
            ));
        }
        let shown = alice.view(|view| view.entities[id].position);
        assert!(
            pressed(shown, axis, sign),
            "heading {yaw}: Alice shows the pig at {shown:?}"
        );
    }
    assert!(
        moving.is_empty(),
        "Alice was sent move or rotation packets in 40 ticks for pigs pressed \
         against the wall:\n{}",
        moving.join("\n")
    );

    alice.assert_healthy();
    server.stop().await;
}
