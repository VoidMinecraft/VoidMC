use bevy_ecs::prelude::*;
use tracing::instrument;

use crate::components::{
    EntityCollider, EntityDimension, Grounded, MovementConfig, Position, SpawnedEntity, Velocity,
    VerticalVelocity,
};
use crate::world::{
    ChunkData, ChunkIndex, ChunkPosition, block_state_at_world, is_solid_block_state,
};

const GRAVITY_STEP: f64 = 0.08;
const TERMINAL_VELOCITY: f64 = -3.92;
const COLLISION_EPSILON: f64 = 1.0e-7;
const SUPPORT_EPSILON: f64 = 1.0e-4;

/// Applies a small server-authoritative physics step for spawned entities.
#[instrument(
    name = "entity_physics",
    level = "info",
    skip(chunk_index, chunks, query)
)]
pub fn apply_spawned_entity_physics(
    chunk_index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    mut query: Query<
        (
            &mut Position,
            &MovementConfig,
            &EntityDimension,
            &mut Velocity,
            &mut VerticalVelocity,
            &mut Grounded,
            Option<&EntityCollider>,
        ),
        With<SpawnedEntity>,
    >,
) {
    for (
        mut position,
        movement,
        dimension,
        mut velocity,
        mut vertical_velocity,
        mut grounded,
        collider,
    ) in query.iter_mut()
    {
        let collider = collider.copied().unwrap_or_default();
        let mut next_position = *position;
        let mut next_velocity = *velocity;
        let mut solid_at = |x, y, z| {
            block_state_at_world(&chunk_index, &chunks, dimension.0, x, y, z)
                .is_some_and(is_solid_block_state)
        };

        apply_physics_step(
            &mut next_position,
            movement,
            &mut next_velocity,
            &mut vertical_velocity,
            &mut grounded,
            collider,
            &mut solid_at,
        );

        if *position != next_position {
            *position = next_position;
        }
        if *velocity != next_velocity {
            *velocity = next_velocity;
        }
    }
}

fn apply_physics_step(
    position: &mut Position,
    movement: &MovementConfig,
    velocity: &mut Velocity,
    vertical_velocity: &mut VerticalVelocity,
    grounded: &mut Grounded,
    collider: EntityCollider,
    solid_at: &mut impl FnMut(i32, i32, i32) -> bool,
) {
    let mut next_x = position.x;
    let mut next_y = position.y;
    let mut next_z = position.z;

    if movement.block_collision_enabled {
        let proposed_x = next_x + velocity.x;
        if collides_at(proposed_x, next_y, next_z, collider, solid_at) {
            if let Some(step_y) =
                step_height_at(proposed_x, next_y, next_z, collider, grounded.0, solid_at)
            {
                next_x = proposed_x;
                next_y = step_y;
                grounded.0 = true;
                vertical_velocity.0 = 0.0;
            } else {
                velocity.x = 0.0;
            }
        } else {
            next_x = proposed_x;
        }

        let proposed_z = next_z + velocity.z;
        if collides_at(next_x, next_y, proposed_z, collider, solid_at) {
            if let Some(step_y) =
                step_height_at(next_x, next_y, proposed_z, collider, grounded.0, solid_at)
            {
                next_z = proposed_z;
                next_y = step_y;
                grounded.0 = true;
                vertical_velocity.0 = 0.0;
            } else {
                velocity.z = 0.0;
            }
        } else {
            next_z = proposed_z;
        }
    } else {
        next_x += velocity.x;
        next_z += velocity.z;
    }

    if movement.gravity_enabled {
        let supported = movement.block_collision_enabled
            && is_supported(next_x, next_y, next_z, collider, solid_at);

        if grounded.0 && vertical_velocity.0 <= 0.0 && supported {
            vertical_velocity.0 = 0.0;
            velocity.y = 0.0;
        } else {
            vertical_velocity.0 = (vertical_velocity.0 - GRAVITY_STEP).max(TERMINAL_VELOCITY);
            let proposed_y = next_y + vertical_velocity.0;
            // Sweep the whole fall, not only its end: a fast fall can carry the
            // box past a thin floor between two ticks.
            let landing = (movement.block_collision_enabled && vertical_velocity.0 < 0.0)
                .then(|| landing_height(next_x, next_y, proposed_y, next_z, collider, solid_at))
                .flatten();

            if let Some(landing_y) = landing {
                next_y = landing_y;
                grounded.0 = true;
                vertical_velocity.0 = 0.0;
            } else if movement.block_collision_enabled
                && collides_at(next_x, proposed_y, next_z, collider, solid_at)
            {
                // Blocked with no block top to land on: a ceiling when rising,
                // or a box that already overlaps a block.
                grounded.0 = vertical_velocity.0 < 0.0;
                vertical_velocity.0 = 0.0;
            } else {
                next_y = proposed_y;
                grounded.0 = false;
            }

            velocity.y = vertical_velocity.0;
        }
    }

    position.x = next_x;
    position.y = next_y;
    position.z = next_z;
}

fn step_height_at(
    x: f64,
    y: f64,
    z: f64,
    collider: EntityCollider,
    grounded: bool,
    solid_at: &mut impl FnMut(i32, i32, i32) -> bool,
) -> Option<f64> {
    if !grounded || collider.step_height <= 0.0 {
        return None;
    }

    let step_y = y + collider.step_height;
    (!collides_at(x, step_y, z, collider, solid_at)
        && is_supported(x, step_y, z, collider, solid_at))
    .then_some(step_y)
}

fn is_supported(
    x: f64,
    y: f64,
    z: f64,
    collider: EntityCollider,
    solid_at: &mut impl FnMut(i32, i32, i32) -> bool,
) -> bool {
    collides_at(x, y - SUPPORT_EPSILON, z, collider, solid_at)
}

fn collides_at(
    x: f64,
    y: f64,
    z: f64,
    collider: EntityCollider,
    solid_at: &mut impl FnMut(i32, i32, i32) -> bool,
) -> bool {
    let min_x = (x - collider.half_width + COLLISION_EPSILON).floor() as i32;
    let max_x = (x + collider.half_width - COLLISION_EPSILON).floor() as i32;
    let min_y = (y + COLLISION_EPSILON).floor() as i32;
    let max_y = (y + collider.height - COLLISION_EPSILON).floor() as i32;
    let min_z = (z - collider.half_width + COLLISION_EPSILON).floor() as i32;
    let max_z = (z + collider.half_width - COLLISION_EPSILON).floor() as i32;

    (min_x..=max_x).any(|block_x| {
        (min_y..=max_y)
            .any(|block_y| (min_z..=max_z).any(|block_z| solid_at(block_x, block_y, block_z)))
    })
}

fn landing_height(
    x: f64,
    current_y: f64,
    proposed_y: f64,
    z: f64,
    collider: EntityCollider,
    solid_at: &mut impl FnMut(i32, i32, i32) -> bool,
) -> Option<f64> {
    let min_x = (x - collider.half_width + COLLISION_EPSILON).floor() as i32;
    let max_x = (x + collider.half_width - COLLISION_EPSILON).floor() as i32;
    let min_z = (z - collider.half_width + COLLISION_EPSILON).floor() as i32;
    let max_z = (z + collider.half_width - COLLISION_EPSILON).floor() as i32;
    let min_y = (proposed_y + COLLISION_EPSILON).floor() as i32;
    let max_y = (current_y - COLLISION_EPSILON).floor() as i32;

    (min_y..=max_y)
        .rev()
        .find(|block_y| {
            (min_x..=max_x)
                .any(|block_x| (min_z..=max_z).any(|block_z| solid_at(block_x, *block_y, block_z)))
        })
        .map(|block_y| block_y as f64 + 1.0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn config() -> MovementConfig {
        MovementConfig {
            wander: true,
            gravity_enabled: true,
            block_collision_enabled: true,
        }
    }

    fn step(
        position: &mut Position,
        velocity: &mut Velocity,
        vertical_velocity: &mut VerticalVelocity,
        grounded: &mut Grounded,
        blocks: &HashSet<(i32, i32, i32)>,
    ) {
        apply_physics_step(
            position,
            &config(),
            velocity,
            vertical_velocity,
            grounded,
            EntityCollider::for_entity_name("minecraft:pig"),
            &mut |x, y, z| blocks.contains(&(x, y, z)),
        );
    }

    #[test]
    fn grounded_entity_does_not_hover_or_bounce() {
        let blocks = HashSet::from([(0, 63, 0)]);
        let mut position = Position {
            x: 0.5,
            y: 64.0,
            z: 0.5,
        };
        let mut velocity = Velocity {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let mut vertical_velocity = VerticalVelocity(0.0);
        let mut grounded = Grounded(true);

        for _ in 0..1_200 {
            step(
                &mut position,
                &mut velocity,
                &mut vertical_velocity,
                &mut grounded,
                &blocks,
            );
        }

        assert_eq!(position.y, 64.0);
        assert_eq!(vertical_velocity.0, 0.0);
        assert_eq!(velocity.y, 0.0);
        assert!(grounded.0);
    }

    #[test]
    fn falling_entity_lands_exactly_on_block_top() {
        let blocks = HashSet::from([(0, 63, 0)]);
        let mut position = Position {
            x: 0.5,
            y: 65.2,
            z: 0.5,
        };
        let mut velocity = Velocity {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let mut vertical_velocity = VerticalVelocity(0.0);
        let mut grounded = Grounded(false);

        for _ in 0..20 {
            step(
                &mut position,
                &mut velocity,
                &mut vertical_velocity,
                &mut grounded,
                &blocks,
            );
        }

        assert_eq!(position.y, 64.0);
        assert!(grounded.0);
    }

    #[test]
    fn pig_body_cannot_clip_through_wall() {
        let blocks = HashSet::from([(1, 64, 0)]);
        let mut position = Position {
            x: 0.5,
            y: 64.0,
            z: 0.5,
        };
        let mut velocity = Velocity {
            x: 0.08,
            y: 0.0,
            z: 0.0,
        };
        let mut vertical_velocity = VerticalVelocity(0.0);
        let mut grounded = Grounded(false);

        step(
            &mut position,
            &mut velocity,
            &mut vertical_velocity,
            &mut grounded,
            &blocks,
        );

        assert_eq!(position.x, 0.5);
        assert_eq!(velocity.x, 0.0);
    }

    #[test]
    fn grounded_pig_steps_onto_one_block_rise() {
        let blocks = HashSet::from([(0, 63, 0), (1, 63, 0), (1, 64, 0)]);
        let mut position = Position {
            x: 0.5,
            y: 64.0,
            z: 0.5,
        };
        let mut velocity = Velocity {
            x: 0.08,
            y: 0.0,
            z: 0.0,
        };
        let mut vertical_velocity = VerticalVelocity(0.0);
        let mut grounded = Grounded(true);

        step(
            &mut position,
            &mut velocity,
            &mut vertical_velocity,
            &mut grounded,
            &blocks,
        );

        assert_eq!(position.x, 0.58);
        assert_eq!(position.y, 65.0);
        assert!(grounded.0);
    }

    /// A pig driven by the test, with the physics state the server keeps.
    struct Body {
        position: Position,
        velocity: Velocity,
        vertical_velocity: VerticalVelocity,
        grounded: Grounded,
        movement: MovementConfig,
    }

    impl Body {
        fn pig(x: f64, y: f64, z: f64, grounded: bool) -> Self {
            Self {
                position: Position { x, y, z },
                velocity: Velocity {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                vertical_velocity: VerticalVelocity(0.0),
                grounded: Grounded(grounded),
                movement: config(),
            }
        }

        /// Sets the horizontal velocity the way `wander_system` does.
        fn steer(&mut self, yaw_degrees: f64, speed: f64) {
            let yaw = yaw_degrees.to_radians();
            self.velocity.x = yaw.sin() * speed;
            self.velocity.z = yaw.cos() * speed;
        }

        fn step(&mut self, blocks: &HashSet<(i32, i32, i32)>) {
            apply_physics_step(
                &mut self.position,
                &self.movement,
                &mut self.velocity,
                &mut self.vertical_velocity,
                &mut self.grounded,
                pig(),
                &mut |x, y, z| blocks.contains(&(x, y, z)),
            );
        }
    }

    fn pig() -> EntityCollider {
        EntityCollider::for_entity_name("minecraft:pig")
    }

    /// The solid blocks the pig's box overlaps with a positive volume. Exact
    /// geometry, independent of the collision code under test: touching a
    /// face is not an overlap.
    fn overlapping_blocks(
        position: &Position,
        blocks: &HashSet<(i32, i32, i32)>,
    ) -> Vec<(i32, i32, i32)> {
        let collider = pig();
        let (min_x, max_x) = (
            position.x - collider.half_width,
            position.x + collider.half_width,
        );
        let (min_y, max_y) = (position.y, position.y + collider.height);
        let (min_z, max_z) = (
            position.z - collider.half_width,
            position.z + collider.half_width,
        );
        let mut hits: Vec<_> = blocks
            .iter()
            .copied()
            .filter(|&(x, y, z)| {
                let (x, y, z) = (f64::from(x), f64::from(y), f64::from(z));
                min_x < x + 1.0
                    && max_x > x
                    && min_y < y + 1.0
                    && max_y > y
                    && min_z < z + 1.0
                    && max_z > z
            })
            .collect();
        hits.sort_unstable();
        hits
    }

    fn assert_clear(body: &Body, blocks: &HashSet<(i32, i32, i32)>, context: &str) {
        let position = &body.position;
        assert!(
            position.x.is_finite() && position.y.is_finite() && position.z.is_finite(),
            "{context}: non-finite position {position:?}"
        );
        let hits = overlapping_blocks(position, blocks);
        assert!(
            hits.is_empty(),
            "{context}: the pig's box at {position:?} overlaps {hits:?}"
        );
    }

    /// A square floor at y = 63 covering `-radius..=radius` on both axes.
    fn floor(radius: i32) -> HashSet<(i32, i32, i32)> {
        (-radius..=radius)
            .flat_map(|x| (-radius..=radius).map(move |z| (x, 63, z)))
            .collect()
    }

    /// The floor plus a ring wall `height` blocks tall standing on it, whose
    /// inner faces are at x = ±`inner` and z = ±`inner`.
    fn walled_floor(inner: i32, height: i32) -> HashSet<(i32, i32, i32)> {
        let mut blocks = floor(inner + 2);
        for a in -inner - 1..=inner {
            for y in 64..64 + height {
                blocks.insert((a, y, -inner - 1));
                blocks.insert((a, y, inner));
                blocks.insert((-inner - 1, y, a));
                blocks.insert((inner, y, a));
            }
        }
        blocks
    }

    #[test]
    fn falls_from_any_height_onto_a_one_block_thick_floor() {
        let blocks = floor(2);
        // From a fraction above the floor up to drops where the per-tick fall
        // reaches terminal velocity, far more than the pig's height plus the
        // floor's thickness.
        for k in 0..600 {
            let start = 64.0 + 0.013 + f64::from(k) * 0.5;
            let mut body = Body::pig(0.5, start, 0.5, false);
            let mut fastest = 0.0_f64;
            for tick in 0..400 {
                let before = body.position.y;
                body.step(&blocks);
                let context = format!("drop from y = {start}, tick {tick}");
                assert_clear(&body, &blocks, &context);
                assert!(body.position.y <= before, "{context}: moved up");
                assert!(body.position.y >= 64.0, "{context}: below the floor");
                fastest = fastest.max(before - body.position.y);
                if body.grounded.0 {
                    break;
                }
            }
            assert_eq!(body.position.y, 64.0, "drop from y = {start}");
            assert!(body.grounded.0, "drop from y = {start}");
            if start > 200.0 {
                assert!(fastest > 1.9, "the long drops do outrun the floor check");
            }

            for _ in 0..100 {
                body.step(&blocks);
                assert_eq!(body.position.y, 64.0, "drop from y = {start} came to rest");
                assert!(body.grounded.0);
                assert_eq!(body.vertical_velocity.0, 0.0);
            }
        }
    }

    #[test]
    fn pig_never_enters_a_two_block_wall_and_slides_into_the_corner() {
        let inner = 4;
        let blocks = walled_floor(inner, 2);
        let half_width = pig().half_width;
        for speed in [0.08, 0.35] {
            for yaw in (0..360).step_by(3) {
                let yaw = f64::from(yaw);
                let mut body = Body::pig(0.5, 64.0, 0.5, true);
                for tick in 0..2_000 {
                    body.steer(yaw, speed);
                    body.step(&blocks);
                    let context = format!("yaw {yaw}, speed {speed}, tick {tick}");
                    assert_clear(&body, &blocks, &context);
                    assert_eq!(body.position.y, 64.0, "{context}: left the floor");
                    assert!(body.grounded.0, "{context}");
                }

                // Every direction ends against the wall it heads to, sliding
                // along it into the corner when it comes in at an angle.
                body.steer(yaw, speed);
                let reach = f64::from(inner) - half_width;
                for (axis, coordinate, velocity) in [
                    ("x", body.position.x, body.velocity.x),
                    ("z", body.position.z, body.velocity.z),
                ] {
                    if velocity.abs() < 1.0e-9 {
                        continue;
                    }
                    let gap = if velocity > 0.0 {
                        reach - coordinate
                    } else {
                        coordinate + reach
                    };
                    assert!(
                        (0.0..speed).contains(&gap),
                        "yaw {yaw}, speed {speed}: stopped {gap} short of the {axis} wall"
                    );
                }
            }
        }
    }

    #[test]
    fn pig_walks_over_a_one_block_rise_from_every_side() {
        // A rise three blocks deep across the whole floor, with the floor
        // continuing beyond it.
        let rise = |rotate: &dyn Fn(i32, i32) -> (i32, i32)| -> HashSet<(i32, i32, i32)> {
            let mut blocks = floor(12);
            for along in 2..=4 {
                for across in -12..=12 {
                    let (x, z) = rotate(along, across);
                    blocks.insert((x, 64, z));
                }
            }
            blocks
        };
        let sides: [(f64, &dyn Fn(i32, i32) -> (i32, i32)); 4] = [
            (90.0, &|along, across| (along, across)),
            (270.0, &|along, across| (-along, across)),
            (0.0, &|along, across| (across, along)),
            (180.0, &|along, across| (across, -along)),
        ];
        for (heading, rotate) in sides {
            let blocks = rise(rotate);
            // Straight on, and at an angle.
            for offset in [0.0, 30.0, -40.0] {
                let yaw = heading + offset;
                let mut body = Body::pig(0.5, 64.0, 0.5, true);
                let mut on_top = 0;
                for tick in 0..120 {
                    let before = body.position.y;
                    body.steer(yaw, 0.08);
                    body.step(&blocks);
                    let context = format!("yaw {yaw}, tick {tick}");
                    assert_clear(&body, &blocks, &context);
                    assert!(
                        body.position.y - before <= 1.0,
                        "{context}: rose more than one block"
                    );
                    if body.position.y == 65.0 {
                        on_top += 1;
                    }
                }
                let along = if heading == 90.0 || heading == 270.0 {
                    body.position.x.abs()
                } else {
                    body.position.z.abs()
                };
                assert!(on_top > 10, "yaw {yaw}: never walked on the rise");
                assert!(along > 6.0, "yaw {yaw}: stuck at {:?}", body.position);
                assert_eq!(
                    body.position.y, 64.0,
                    "yaw {yaw}: back on the floor past the rise"
                );
                assert!(body.grounded.0, "yaw {yaw}");
            }
        }
    }

    #[test]
    fn without_gravity_the_pig_keeps_its_height() {
        let blocks = floor(12);
        let mut body = Body::pig(0.5, 70.25, 0.5, true);
        body.movement.gravity_enabled = false;
        for _ in 0..200 {
            body.steer(45.0, 0.08);
            body.step(&blocks);
            assert_eq!(body.position.y, 70.25);
        }
        assert!(
            body.position.x > 10.0,
            "it still walks: {:?}",
            body.position
        );
    }

    #[test]
    fn without_block_checks_floors_and_walls_do_not_stop_the_pig() {
        let blocks = walled_floor(4, 2);

        // Gravity alone: it falls through the floor.
        let mut body = Body::pig(0.5, 64.0, 0.5, false);
        body.movement.block_collision_enabled = false;
        for _ in 0..40 {
            body.step(&blocks);
        }
        assert!(body.position.y < 60.0, "{:?}", body.position);
        assert!(!body.grounded.0);

        // Neither: it walks through the wall.
        let mut body = Body::pig(0.5, 64.0, 0.5, true);
        body.movement.block_collision_enabled = false;
        body.movement.gravity_enabled = false;
        for _ in 0..100 {
            body.steer(90.0, 0.08);
            body.step(&blocks);
        }
        assert!(body.position.x > 6.0, "{:?}", body.position);
        assert_eq!(body.position.y, 64.0);
    }

    #[test]
    fn several_pigs_keep_every_invariant_over_a_long_random_walk() {
        // A walled pen with a one-block rise and a pillar in it, walked by
        // pigs that change direction like `wander_system`, from a fixed seed.
        let inner = 6;
        let mut blocks = walled_floor(inner, 2);
        for x in -2..=1 {
            for z in 1..=3 {
                blocks.insert((x, 64, z));
            }
        }
        blocks.extend([(3, 64, -3), (3, 65, -3)]);
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut pigs: Vec<(Body, f64, i32)> = (0..8)
            .map(|i| {
                let x = f64::from(i % 4) - 1.5 + random() * 0.2;
                (Body::pig(x, 66.0 + random() * 6.0, -1.5, false), 0.0, 0)
            })
            .collect();
        let limit = f64::from(inner) - pig().half_width;
        for tick in 0..5_000 {
            for (index, (body, yaw, ticks)) in pigs.iter_mut().enumerate() {
                *ticks -= 1;
                if *ticks <= 0 {
                    *yaw = random() * 360.0;
                    *ticks = 40 + (random() * 100.0) as i32;
                }
                body.steer(*yaw, 0.08);
                let before = body.position.y;
                body.step(&blocks);
                let context = format!("pig {index}, tick {tick}");
                assert_clear(body, &blocks, &context);
                let position = &body.position;
                assert!(
                    position.x.abs() <= limit + 1.0e-9 && position.z.abs() <= limit + 1.0e-9,
                    "{context}: left the pen at {position:?}"
                );
                assert!(position.y >= 64.0 && position.y <= 72.0, "{context}");
                assert!(position.y - before <= 1.0, "{context}: rose too much");
                if body.grounded.0 {
                    // Grounded means resting on the floor or the rise, never
                    // on the two-block pillar or the walls.
                    assert!(
                        position.y == 64.0 || position.y == 65.0,
                        "{context}: grounded at {position:?}"
                    );
                    assert_eq!(body.vertical_velocity.0, 0.0, "{context}");
                }
            }
        }
    }
}
