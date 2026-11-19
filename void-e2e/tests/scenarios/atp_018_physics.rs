//! VMC-ATP-018: a pig summoned with `--wander --gravity --block-checks` falls
//! onto the block top and reports a grounded state, steps onto a one-block
//! rise, cannot clip through or climb a two-block wall, and stays stable over
//! hundreds of ticks. Without `--gravity` it does not fall; without
//! `--block-checks` blocks do not stop it.
//!
//! Every scenario builds the same pen in the air above the players with block
//! edits on the game thread (`mutate_block`, the path player edits take, so
//! both clients are sent the blocks) and summons pigs with the real `/summon`
//! command. A recorder system samples every summoned entity once per tick,
//! after the simulation, so the assertions see every tick and not only the
//! ones a wait happened to land on.
//!
//! `--wander` draws a random direction. Where a scenario needs a pig to walk
//! into a given obstacle, the test points the pig's `Wander` component that
//! way (its yaw, and a timer long enough that it never re-rolls; the speed
//! stays the server's), right after the summon. Falling, landing, stepping,
//! blocking and the packets are the server's. The random-walk scenario
//! steers nothing.

use bevy_app::PostUpdate;
use bevy_ecs::prelude::*;
use voidmc::components::{
    EntityCollider, EntityDimension, Grounded, MinecraftEntityId, MovementConfig, Position,
    SpawnedEntity, Wander,
};
use voidmc::world::{
    BlockMutation, ChunkData, ChunkIndex, ChunkPosition, DimensionId, block_state_at_world,
    is_solid_block_state, mutate_block,
};
use voidmc::{BlockFace, BlockPosition};
use voidmc_e2e::core::position::BlockPos;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::registry::builtin::EntityKind;
use voidmc_e2e::{Bot, TestServer};
use voidmc_protocol::clientbound::chunk::blocks;

use crate::feet;

const ALL_FLAGS: &str = "--wander --gravity --block-checks";
/// The pen's floor spans `-INNER..=INNER` blocks around its centre.
const INNER: i32 = 4;
/// Half the width of a pig's collision box.
const PIG_HALF_WIDTH: f64 = 0.45;
/// Blocks a wandering pig moves per tick.
const WANDER_SPEED: f64 = 0.08;
/// Yaw headings in `wander_system`'s convention: 0 walks south (+z), 90 east (+x).
const EAST: f32 = 90.0;
const WEST: f32 = 270.0;
const SOUTH_EAST: f32 = 45.0;
const NORTH_EAST: f32 = 135.0;
const NORTH_WEST: f32 = 225.0;

/// A pen in the air: a one-block-thick stone floor, a ring wall two blocks
/// high, a one-block rise near the middle and a two-block pillar.
#[derive(Clone, Copy, Debug)]
struct Pen {
    x: i32,
    z: i32,
    floor: i32,
}

impl Pen {
    /// The feet height of a pig standing on the floor.
    fn ground(self) -> f64 {
        f64::from(self.floor + 1)
    }

    fn at(self, dx: f64, dz: f64) -> (f64, f64) {
        (f64::from(self.x) + dx, f64::from(self.z) + dz)
    }

    /// The lowest and highest x a pig's centre can have inside the walls.
    fn x_range(self) -> (f64, f64) {
        (
            f64::from(self.x - INNER) + PIG_HALF_WIDTH,
            f64::from(self.x + INNER + 1) - PIG_HALF_WIDTH,
        )
    }

    fn z_range(self) -> (f64, f64) {
        (
            f64::from(self.z - INNER) + PIG_HALF_WIDTH,
            f64::from(self.z + INNER + 1) - PIG_HALF_WIDTH,
        )
    }

    fn blocks(self) -> Vec<[i32; 3]> {
        let outer = INNER + 1;
        let mut blocks = Vec::new();
        for dx in -outer..=outer {
            for dz in -outer..=outer {
                let (x, z) = (self.x + dx, self.z + dz);
                blocks.push([x, self.floor, z]);
                let wall = dx.abs() == outer || dz.abs() == outer;
                // The rise is three blocks from every wall, so a pig on it
                // never faces a one-block step onto a wall.
                let rise = (0..=1).contains(&dx) && (-1..=1).contains(&dz);
                let pillar = (dx, dz) == (-2, 2);
                if wall || pillar {
                    blocks.push([x, self.floor + 1, z]);
                    blocks.push([x, self.floor + 2, z]);
                } else if rise {
                    blocks.push([x, self.floor + 1, z]);
                }
            }
        }
        blocks
    }
}

/// Builds the pen twelve blocks above Alice's feet and waits until she is
/// sent its last block.
async fn build_pen(server: &TestServer, alice: &Bot) -> Pen {
    let feet = feet(alice);
    let pen = Pen {
        x: feet.x,
        z: feet.z,
        floor: feet.y + 12,
    };
    let actor = server.player_entity(alice.uuid()).await;
    let placed = pen.blocks();
    let count = placed.len();
    let changed = server
        .with_world(move |world| {
            placed
                .into_iter()
                .filter(|&[x, y, z]| {
                    let position = BlockPosition { x, y: y as i16, z };
                    mutate_block(
                        world,
                        actor,
                        DimensionId::Overworld,
                        position,
                        blocks::STONE,
                        BlockFace::Top,
                        BlockMutation::Place,
                    )
                    .is_some()
                })
                .count()
        })
        .await;
    assert_eq!(changed, count, "the pen was built in empty air");
    let last = BlockPos::new(pen.x + INNER + 1, pen.floor + 2, pen.z + INNER + 1);
    alice
        .wait_until("the pen", |view| {
            view.block_at(last).is_some_and(|state| !state.is_air())
        })
        .await;
    pen
}

/// One summoned entity in one tick, as the server holds it after the
/// simulation ran.
#[derive(Clone, Copy, Debug)]
struct Sample {
    tick: u64,
    id: i32,
    position: [f64; 3],
    grounded: bool,
    /// A solid block that the entity's box overlaps with a positive volume,
    /// for an entity with block checks.
    overlap: Option<[i32; 3]>,
}

#[derive(Resource, Default)]
struct Trace {
    tick: u64,
    samples: Vec<Sample>,
}

type Recorded<'a> = (
    &'a MinecraftEntityId,
    &'a Position,
    &'a Grounded,
    &'a MovementConfig,
    &'a EntityCollider,
    &'a EntityDimension,
);

fn record(
    mut trace: ResMut<Trace>,
    index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    entities: Query<Recorded, With<SpawnedEntity>>,
) {
    trace.tick += 1;
    let tick = trace.tick;
    for (id, position, grounded, movement, collider, dimension) in &entities {
        let overlap = movement
            .block_collision_enabled
            .then(|| {
                // Exact geometry, independent of the server's collision code:
                // every block in these ranges overlaps the box; touching a
                // face does not count.
                let low = |c: f64| c.floor() as i32;
                let high = |c: f64| c.ceil() as i32 - 1;
                let (x0, x1) = (
                    low(position.x - collider.half_width),
                    high(position.x + collider.half_width),
                );
                let (y0, y1) = (low(position.y), high(position.y + collider.height));
                let (z0, z1) = (
                    low(position.z - collider.half_width),
                    high(position.z + collider.half_width),
                );
                (x0..=x1)
                    .flat_map(|x| (y0..=y1).flat_map(move |y| (z0..=z1).map(move |z| [x, y, z])))
                    .find(|&[x, y, z]| {
                        block_state_at_world(&index, &chunks, dimension.0, x, y, z)
                            .is_some_and(is_solid_block_state)
                    })
            })
            .flatten();
        trace.samples.push(Sample {
            tick,
            id: id.0,
            position: [position.x, position.y, position.z],
            grounded: grounded.0,
            overlap,
        });
    }
}

async fn recorded_server() -> TestServer {
    TestServer::builder()
        .plugin(|app| {
            app.init_resource::<Trace>().add_systems(PostUpdate, record);
        })
        .start()
        .await
}

/// The recorded samples of one entity, oldest first.
async fn samples(server: &TestServer, id: i32) -> Vec<Sample> {
    server
        .with_world(move |world| {
            world
                .resource::<Trace>()
                .samples
                .iter()
                .filter(|sample| sample.id == id)
                .copied()
                .collect()
        })
        .await
}

/// The entity's position and grounded state, and the last recorded tick.
fn pig_state(world: &mut World, id: i32) -> Option<([f64; 3], bool, u64)> {
    let tick = world.resource::<Trace>().tick;
    world
        .query::<(&MinecraftEntityId, &Position, &Grounded)>()
        .iter(world)
        .find(|(entity, _, _)| entity.0 == id)
        .map(|(_, position, grounded)| ([position.x, position.y, position.z], grounded.0, tick))
}

/// Runs `/summon minecraft:pig x y z flags` as Alice and returns the id of the
/// pig she is sent.
async fn summon(alice: &Bot, x: f64, y: f64, z: f64, flags: &str) -> i32 {
    alice
        .command(&format!("summon minecraft:pig {x} {y} {z} {flags}"))
        .await;
    alice
        .expect("the summon confirmation", |packet| match packet {
            ClientboundGamePacket::SystemChat(chat) => chat
                .content
                .to_string()
                .starts_with("Summoned minecraft:pig at ")
                .then_some(()),
            _ => None,
        })
        .await;
    alice
        .expect("the pig added", |packet| match packet {
            ClientboundGamePacket::AddEntity(add) if add.entity_type == EntityKind::Pig => {
                Some(add.id.0)
            }
            _ => None,
        })
        .await
}

/// Points the pig's wander direction at `yaw` for the rest of the test.
async fn steer(server: &TestServer, id: i32, yaw: f32) {
    let steered = server
        .with_world(move |world| {
            world
                .query::<(&MinecraftEntityId, &mut Wander)>()
                .iter_mut(world)
                .find(|(entity, _)| entity.0 == id)
                .map(|(_, mut wander)| {
                    wander.yaw = yaw;
                    wander.ticks = i32::MAX;
                })
                .is_some()
        })
        .await;
    assert!(steered, "pig {id} wanders");
}

/// A height a client was sent for an entity.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    y: f64,
    delta_y: f64,
    on_ground: bool,
}

/// Every height `bot` was sent for the entity after adding it, in order,
/// decoded like the vanilla client does: a relative move adds to the last
/// position in 1/4096-block units.
fn seen_moves(bot: &Bot, id: i32) -> Vec<Seen> {
    let units = |c: f64| (c * 4096.0).round() as i64;
    let mut base: Option<i64> = None;
    bot.queued(|packet| {
        let (next, on_ground) = match packet {
            ClientboundGamePacket::AddEntity(add) if add.id.0 == id => {
                base = Some(units(add.position.y));
                return None;
            }
            ClientboundGamePacket::MoveEntityPos(p) if p.entity_id.0 == id => {
                (base? + i64::from(p.delta.ya), p.on_ground)
            }
            ClientboundGamePacket::MoveEntityPosRot(p) if p.entity_id.0 == id => {
                (base? + i64::from(p.delta.ya), p.on_ground)
            }
            ClientboundGamePacket::EntityPositionSync(p) if p.id.0 == id => {
                (units(p.values.pos.y), p.on_ground)
            }
            _ => return None,
        };
        let delta_y = (next - base?) as f64 / 4096.0;
        base = Some(next);
        Some(Seen {
            y: next as f64 / 4096.0,
            delta_y,
            on_ground,
        })
    })
}

/// The entity's position and the last recorded tick, read a few ticks after
/// a wait saw it arrive, so that any last partial step is over.
async fn settled(server: &TestServer, id: i32) -> ([f64; 3], u64) {
    server.wait_ticks(5).await;
    server
        .with_world(move |world| pig_state(world, id).map(|(position, _, tick)| (position, tick)))
        .await
        .unwrap_or_else(|| panic!("pig {id} is gone"))
}

/// The client's position of the entity matches the server's, to the
/// 1/4096-block precision of relative moves.
fn assert_seen_at(bot: &Bot, id: i32, server_position: [f64; 3]) {
    let seen = bot
        .view(|view| view.entities.get(&id).map(|entity| entity.position))
        .unwrap_or_else(|| panic!("{} does not see pig {id}", bot.name()));
    let off = [
        seen.x - server_position[0],
        seen.y - server_position[1],
        seen.z - server_position[2],
    ];
    assert!(
        off.iter().all(|d| d.abs() <= 1.0 / 4096.0),
        "{} sees pig {id} at {seen:?}, the server has it at {server_position:?}",
        bot.name()
    );
}

fn assert_never_overlaps(samples: &[Sample]) {
    for sample in samples {
        assert!(sample.position.iter().all(|c| c.is_finite()), "{sample:?}");
        assert_eq!(sample.overlap, None, "the box overlaps a block: {sample:?}");
    }
}

#[tokio::test]
async fn summoned_pig_falls_onto_the_floor_and_rests_grounded() {
    let server = recorded_server().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let pen = build_pen(&server, &alice).await;
    let ground = pen.ground();
    let (west, east) = pen.x_range();
    let (north, _) = pen.z_range();

    // A short drop: within its 64-block scan, spawn settling puts the pig on
    // the ground in its first tick. Steered into the north-west corner, where
    // both walls hold it still.
    let (x, z) = pen.at(-2.5, -2.5);
    let near = summon(&alice, x, ground + 5.0, z, ALL_FLAGS).await;
    steer(&server, near, NORTH_WEST).await;

    // A 99-block drop, beyond that scan: the physics step carries the whole
    // fall, reaching more than three blocks per tick over a floor one block
    // thick. Steered east while it falls, so it lands on the floor clear of
    // the rise, then into the north-east corner.
    let height = ground + 99.0;
    assert!(height < 310.0, "the drop fits under the build limit");
    let (x, z) = pen.at(-3.5, -2.5);
    let far = summon(&alice, x, height, z, ALL_FLAGS).await;
    steer(&server, far, EAST).await;
    server
        .wait_for("the falling pig to land", move |world| {
            let (_, grounded, _) = pig_state(world, far)?;
            grounded.then_some(())
        })
        .await;
    steer(&server, far, NORTH_EAST).await;

    server
        .wait_for("both pigs on the floor in the corners", move |world| {
            let (a, a_grounded, _) = pig_state(world, near)?;
            let (b, b_grounded, _) = pig_state(world, far)?;
            let resting = a_grounded
                && b_grounded
                && a[1] == ground
                && b[1] == ground
                && a[0] < west + WANDER_SPEED
                && b[0] > east - WANDER_SPEED
                && a[2] < north + WANDER_SPEED
                && b[2] < north + WANDER_SPEED;
            resting.then_some(())
        })
        .await;
    let rest = [settled(&server, near).await, settled(&server, far).await];
    server.wait_ticks(100).await;
    for bot in [&alice, &bob] {
        bot.sync().await;
    }

    // The server: tick by tick down, never through the floor, then at rest
    // on its top face.
    let far_samples = samples(&server, far).await;
    assert_never_overlaps(&far_samples);
    let mut falling_ticks = 0;
    let mut fastest = 0.0_f64;
    for pair in far_samples.windows(2) {
        let drop = pair[0].position[1] - pair[1].position[1];
        assert!(drop >= 0.0, "the falling pig moved up: {pair:?}");
        if drop > 0.0 {
            falling_ticks += 1;
        }
        fastest = fastest.max(drop);
    }
    assert!(falling_ticks >= 30, "fell over {falling_ticks} ticks");
    assert!(fastest > 3.0, "fastest fall {fastest} blocks per tick");
    for sample in &far_samples {
        assert!(sample.position[1] >= ground, "below the floor: {sample:?}");
        assert_eq!(
            sample.grounded,
            sample.position[1] == ground,
            "grounded exactly when on the floor: {sample:?}"
        );
    }
    let near_samples = samples(&server, near).await;
    assert_never_overlaps(&near_samples);
    assert!(
        near_samples
            .iter()
            .all(|sample| sample.position[1] == ground && sample.grounded),
        "the short drop settles on the floor in its first tick: {:?}",
        near_samples.first()
    );

    // At rest for 100 ticks: no jitter, no sinking.
    for (id, samples, (rest, rested_at)) in
        [(near, &near_samples, rest[0]), (far, &far_samples, rest[1])]
    {
        let resting: Vec<_> = samples
            .iter()
            .filter(|sample| sample.tick >= rested_at)
            .collect();
        assert!(resting.len() >= 100, "pig {id}");
        for sample in resting {
            assert_eq!(sample.position, rest, "pig {id} moved at rest");
            assert!(sample.grounded, "pig {id}: {sample:?}");
        }
    }

    // Bob, another player: the fall arrives as relative moves flagged in the
    // air, the landing and everything after it flagged on the ground.
    let moves = seen_moves(&bob, far);
    let rising = moves.iter().find(|seen| seen.delta_y > 0.0);
    assert_eq!(rising, None, "the falling pig moved up");
    let landing = moves
        .iter()
        .position(|seen| seen.y == ground)
        .expect("Bob was sent the landing");
    assert!(landing >= 30, "Bob saw the fall in {landing} moves");
    assert!(
        moves[..landing].iter().any(|seen| seen.delta_y < -3.0),
        "Bob saw the fast part of the fall"
    );
    let grounded_in_the_air = moves[..landing].iter().find(|seen| seen.on_ground);
    assert_eq!(
        grounded_in_the_air, None,
        "flagged on the ground in the air"
    );
    let after = moves[landing..]
        .iter()
        .find(|seen| !seen.on_ground || seen.y != ground);
    assert_eq!(after, None, "after the landing");
    let moves = seen_moves(&bob, near);
    let off = moves
        .iter()
        .find(|seen| !seen.on_ground || seen.y != ground);
    assert_eq!(off, None, "the settled pig left the ground");
    assert_seen_at(&bob, near, rest[0].0);
    assert_seen_at(&bob, far, rest[1].0);

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn pig_steps_onto_a_rise_and_cannot_pass_or_climb_a_wall() {
    let server = recorded_server().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let pen = build_pen(&server, &alice).await;
    let ground = pen.ground();
    let (_, east) = pen.x_range();
    let (_, south) = pen.z_range();

    // West of the rise, steered east: over the rise, down its far side and
    // into the east wall.
    let (x, z) = pen.at(-2.5, 0.5);
    let pig = summon(&alice, x, ground + 3.0, z, ALL_FLAGS).await;
    steer(&server, pig, EAST).await;
    server
        .wait_for("the pig on the rise", move |world| {
            let (position, grounded, _) = pig_state(world, pig)?;
            (grounded && position[1] == ground + 1.0).then_some(())
        })
        .await;
    server
        .wait_for("the pig against the east wall", move |world| {
            let (position, grounded, _) = pig_state(world, pig)?;
            (grounded && position[1] == ground && position[0] > east - WANDER_SPEED).then_some(())
        })
        .await;
    let (at_wall, reached_wall) = settled(&server, pig).await;

    // Pushing into the wall at an angle slides it along the wall into the
    // corner, where it stays.
    server.wait_ticks(20).await;
    steer(&server, pig, SOUTH_EAST).await;
    server
        .wait_for("the pig in the south-east corner", move |world| {
            let (position, grounded, _) = pig_state(world, pig)?;
            (grounded && position[2] > south - WANDER_SPEED).then_some(())
        })
        .await;
    let (corner, cornered) = settled(&server, pig).await;
    server.wait_ticks(60).await;
    for bot in [&alice, &bob] {
        bot.sync().await;
    }

    let samples = samples(&server, pig).await;
    assert_never_overlaps(&samples);
    let on_rise = samples
        .iter()
        .filter(|sample| sample.position[1] == ground + 1.0)
        .count();
    assert!(on_rise >= 10, "walked on the rise for {on_rise} ticks");
    assert!(
        samples
            .iter()
            .all(|sample| sample.position[1] <= ground + 1.0),
        "never climbed the two-block wall or the pillar"
    );
    let left_rise = samples
        .iter()
        .rposition(|sample| sample.position[1] == ground + 1.0)
        .unwrap();
    assert!(
        samples[left_rise + 1..]
            .iter()
            .any(|sample| !sample.grounded && sample.position[1] > ground),
        "fell off the far side of the rise"
    );
    for sample in samples.iter().filter(|sample| sample.tick >= reached_wall) {
        let x = sample.position[0];
        assert!(
            (at_wall[0]..=east).contains(&x),
            "held by the east wall: {sample:?}"
        );
        assert_eq!(sample.position[1], ground, "{sample:?}");
        assert!(sample.grounded, "{sample:?}");
    }
    for sample in samples.iter().filter(|sample| sample.tick >= cornered) {
        assert_eq!(sample.position, corner, "held in the corner");
    }

    // Bob sees the step up flagged on the ground, the drop off the far side
    // in the air, and the pig on the floor in the corner.
    let moves = seen_moves(&bob, pig);
    let step = moves
        .iter()
        .position(|seen| seen.delta_y == 1.0)
        .expect("Bob was sent the step up");
    assert!(moves[step].on_ground && moves[step].y == ground + 1.0);
    assert!(
        moves[step..].iter().any(|seen| !seen.on_ground),
        "the drop off the rise"
    );
    let out_of_range = moves
        .iter()
        .find(|seen| !(ground..=ground + 1.0).contains(&seen.y));
    assert_eq!(out_of_range, None, "below the floor or above the rise");
    let last = moves.last().unwrap();
    assert!(last.on_ground && last.y == ground, "{last:?}");
    assert_seen_at(&bob, pig, corner);

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn randomly_wandering_pigs_never_clip_climb_or_leave_the_pen() {
    let server = recorded_server().await;
    let alice = server.join("Alice").await;
    let pen = build_pen(&server, &alice).await;
    let ground = pen.ground();
    let (west, east) = pen.x_range();
    let (north, south) = pen.z_range();

    // Six pigs, nothing steered: each draws its own directions.
    let spots = [
        (-3.5, -3.5),
        (3.5, -3.5),
        (3.5, 3.5),
        (-3.5, -0.5),
        (0.5, 0.5),
        (2.5, 3.5),
    ];
    let mut pigs = Vec::new();
    for (index, (dx, dz)) in spots.into_iter().enumerate() {
        let (x, z) = pen.at(dx, dz);
        let height = ground + 2.0 + index as f64;
        pigs.push((summon(&alice, x, height, z, ALL_FLAGS).await, height));
    }
    for _ in 0..5 {
        server.wait_ticks(100).await;
    }

    let mut touched_walls = 0;
    for (id, height) in pigs {
        let samples = samples(&server, id).await;
        assert!(samples.len() >= 500, "pig {id}");
        assert_never_overlaps(&samples);
        let landed = samples
            .iter()
            .position(|sample| sample.grounded)
            .unwrap_or_else(|| panic!("pig {id} never landed"));
        for sample in &samples {
            let [x, y, z] = sample.position;
            assert!(
                (west..=east).contains(&x) && (north..=south).contains(&z),
                "pig {id} left the pen: {sample:?}"
            );
            assert!((ground..=height).contains(&y), "pig {id}: {sample:?}");
        }
        for sample in &samples[landed..] {
            let y = sample.position[1];
            assert!(
                y <= ground + 1.0,
                "pig {id} climbed a wall or the pillar: {sample:?}"
            );
            assert!(
                !sample.grounded || y == ground || y == ground + 1.0,
                "pig {id} grounded off a block top: {sample:?}"
            );
        }
        for pair in samples.windows(2) {
            let [a, b] = [pair[0].position, pair[1].position];
            assert!(
                (b[0] - a[0]).abs() <= WANDER_SPEED + 1e-9
                    && (b[2] - a[2]).abs() <= WANDER_SPEED + 1e-9
                    && b[1] - a[1] <= 1.0,
                "pig {id} jumped: {pair:?}"
            );
        }
        let walled = samples.iter().any(|sample| {
            let [x, _, z] = sample.position;
            x < west + WANDER_SPEED
                || x > east - WANDER_SPEED
                || z < north + WANDER_SPEED
                || z > south - WANDER_SPEED
        });
        touched_walls += usize::from(walled);
    }
    assert!(touched_walls > 0, "no pig reached a wall");

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn gravity_and_block_checks_are_opt_in() {
    let server = recorded_server().await;
    let alice = server.join("Alice").await;
    let pen = build_pen(&server, &alice).await;
    let ground = pen.ground();
    let (west, east) = pen.x_range();

    // Block checks without gravity: it floats half a block above the floor,
    // and the east wall still stops it.
    let (x, z) = pen.at(-2.5, -2.5);
    let floating = summon(&alice, x, ground + 0.5, z, "--wander --block-checks").await;
    steer(&server, floating, EAST).await;

    // Gravity without block checks: it falls through the floor.
    let (x, z) = pen.at(2.5, 3.5);
    let sinking = summon(&alice, x, ground + 3.0, z, "--wander --gravity").await;

    // Neither: it keeps its height and walks through the west wall.
    let (x, z) = pen.at(-2.5, -3.5);
    let ghost = summon(&alice, x, ground, z, "--wander").await;
    steer(&server, ghost, WEST).await;

    let floor = f64::from(pen.floor);
    let wall = west - PIG_HALF_WIDTH - 1.0;
    server
        .wait_for(
            "the three pigs past or at the wall and floor",
            move |world| {
                let (a, _, _) = pig_state(world, floating)?;
                let (b, _, _) = pig_state(world, sinking)?;
                let (c, _, _) = pig_state(world, ghost)?;
                let done = a[0] > east - WANDER_SPEED && b[1] < floor - 2.0 && c[0] < wall - 1.0;
                done.then_some(())
            },
        )
        .await;
    server.wait_ticks(40).await;

    let samples_of_floating = samples(&server, floating).await;
    assert_never_overlaps(&samples_of_floating);
    assert!(
        samples_of_floating
            .iter()
            .all(|sample| sample.position[1] == ground + 0.5 && sample.position[0] <= east),
        "floats, held by the wall"
    );
    let samples_of_ghost = samples(&server, ghost).await;
    assert!(
        samples_of_ghost
            .iter()
            .all(|sample| sample.position[1] == ground),
        "keeps its height"
    );
    let samples_of_sinking = samples(&server, sinking).await;
    for pair in samples_of_sinking.windows(2) {
        assert!(pair[1].position[1] <= pair[0].position[1], "{pair:?}");
    }
    assert!(
        samples_of_sinking
            .iter()
            .any(|sample| sample.position[1] < floor - 2.0 && !sample.grounded),
        "fell through the floor"
    );

    alice.assert_healthy();
    server.stop().await;
}
