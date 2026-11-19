use bevy_ecs::prelude::*;
use tracing::instrument;

use crate::components::{
    EntityCollider, EntityDimension, Grounded, MovementConfig, Position, SpawnedEntity, Velocity,
    VerticalVelocity,
};
use crate::config::ExecutionMode;
use crate::world::{
    ChunkData, ChunkIndex, ChunkPosition, RegionPos, block_state_at_world, for_each_region,
    is_solid_block_state,
};

const GRAVITY_STEP: f64 = 0.08;
const TERMINAL_VELOCITY: f64 = -3.92;
const COLLISION_EPSILON: f64 = 1.0e-7;
const SUPPORT_EPSILON: f64 = 1.0e-4;

/// Applies a small server-authoritative physics step for spawned entities.
///
/// Entities are grouped by the region they stand in and the regions are
/// stepped with [`for_each_region`], concurrently under
/// [`ExecutionMode::MultiThreaded`]. A step only reads chunk data and writes
/// the entity's own components, so the result does not depend on how regions
/// are scheduled.
#[instrument(
    name = "entity_physics",
    level = "info",
    skip(mode, chunk_index, chunks, query)
)]
pub fn apply_spawned_entity_physics(
    mode: Option<Res<ExecutionMode>>,
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
    let mode = mode.as_deref().copied().unwrap_or_default();
    let entities: Vec<_> = query.iter_mut().collect();
    for_each_region(
        mode,
        entities,
        |(position, _, dimension, ..)| RegionPos::of_block(dimension.0, position.x, position.z),
        |_, entities| {
            for (position, movement, dimension, velocity, vertical_velocity, grounded, collider) in
                entities
            {
                step_entity(
                    &chunk_index,
                    &chunks,
                    position,
                    movement,
                    dimension,
                    velocity,
                    vertical_velocity,
                    grounded,
                    *collider,
                );
            }
        },
    );
}

fn step_entity(
    chunk_index: &ChunkIndex,
    chunks: &Query<(&ChunkPosition, &ChunkData)>,
    position: &mut Mut<Position>,
    movement: &MovementConfig,
    dimension: &EntityDimension,
    velocity: &mut Mut<Velocity>,
    vertical_velocity: &mut Mut<VerticalVelocity>,
    grounded: &mut Mut<Grounded>,
    collider: Option<&EntityCollider>,
) {
    let collider = collider.copied().unwrap_or_default();
    let mut next_position = **position;
    let mut next_velocity = **velocity;
    let mut solid_at = |x, y, z| {
        block_state_at_world(chunk_index, chunks, dimension.0, x, y, z)
            .is_some_and(is_solid_block_state)
    };

    apply_physics_step(
        &mut next_position,
        movement,
        &mut next_velocity,
        vertical_velocity,
        grounded,
        collider,
        &mut solid_at,
    );

    if **position != next_position {
        **position = next_position;
    }
    if **velocity != next_velocity {
        **velocity = next_velocity;
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

            if movement.block_collision_enabled
                && collides_at(next_x, proposed_y, next_z, collider, solid_at)
            {
                if vertical_velocity.0 < 0.0 {
                    next_y = landing_height(next_x, next_y, proposed_y, next_z, collider, solid_at)
                        .unwrap_or(next_y);
                    grounded.0 = true;
                } else {
                    grounded.0 = false;
                }
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

    mod regions {
        use bevy_app::{App, Update};
        use bevy_ecs::schedule::{ExecutorKind, IntoScheduleConfigs};
        use voidmc_protocol::clientbound::chunk::{
            ChunkHeightmaps, ChunkSection, LightData, blocks,
        };

        use super::super::*;
        use crate::components::RecentlySpawned;
        use crate::entity::{EntityBuilder, EntityKind};
        use crate::systems::settle::settle_recent_spawns;
        use crate::world::{ChunkPos, DimensionId};

        const TICKS: usize = 120;

        /// Chunks in three regions: (0, 0), (-1, -1) and (1, 0).
        const CHUNKS: [(i32, i32); 3] = [(1, 1), (-2, -2), (33, 0)];

        /// A floor at y = 63, a one-block step along local x = 12 and a
        /// two-block wall along local z = 12.
        fn chunk() -> ChunkData {
            let mut chunk = ChunkData::new(
                (0..24).map(|_| ChunkSection::empty()).collect(),
                ChunkHeightmaps::empty(),
                LightData::empty(),
            );
            for a in 0..16 {
                for b in 0..16 {
                    chunk.set_block(a, 63, b, blocks::STONE);
                }
                chunk.set_block(12, 64, a, blocks::STONE);
                chunk.set_block(a, 64, 12, blocks::STONE);
                chunk.set_block(a, 65, 12, blocks::STONE);
            }
            chunk
        }

        struct Run {
            /// Per entity, in spawn order: position, velocity, vertical
            /// velocity, grounded and settle ticks, floats as raw bits.
            state: Vec<[u64; 9]>,
            flyers: Vec<(Entity, Position, Velocity)>,
            app: App,
        }

        fn run(mode: ExecutionMode) -> Run {
            let mut app = App::new();
            app.insert_resource(mode)
                .init_resource::<ChunkIndex>()
                .add_systems(
                    Update,
                    (settle_recent_spawns, apply_spawned_entity_physics).chain(),
                );
            app.edit_schedule(Update, |schedule| {
                schedule.set_executor_kind(match mode {
                    ExecutionMode::SingleThreaded => ExecutorKind::SingleThreaded,
                    ExecutionMode::MultiThreaded => ExecutorKind::MultiThreaded,
                });
            });

            let world = app.world_mut();
            let mut entities = Vec::new();
            let mut flyers = Vec::new();
            for (cx, cz) in CHUNKS {
                let pos = ChunkPos::new(cx, cz);
                let chunk = world.spawn((ChunkPosition(pos), chunk())).id();
                world
                    .resource_mut::<ChunkIndex>()
                    .0
                    .insert((DimensionId::Overworld, pos), chunk);

                for i in 0..16 {
                    let x = (cx * 16) as f64 + 1.5 + (i % 4) as f64 * 2.5;
                    let z = (cz * 16) as f64 + 1.5 + (i / 4) as f64 * 2.5;
                    let velocity = Velocity {
                        x: 0.04 + 0.01 * (i % 5) as f64,
                        y: 0.0,
                        z: 0.03 * (i % 3) as f64,
                    };
                    let builder = EntityBuilder::new(EntityKind::Pig)
                        .at(x, 64.0 + (i % 6) as f64 * 1.7, z)
                        .velocity(velocity)
                        .gravity(i % 7 != 0)
                        .block_collision(i % 7 != 0);
                    let entity = builder.spawn_in(world).id();
                    if i % 7 == 0 {
                        flyers.push((entity, *world.get::<Position>(entity).unwrap(), velocity));
                    }
                    entities.push(entity);
                }
            }

            for _ in 0..TICKS {
                app.update();
            }

            let world = app.world();
            let state = entities
                .iter()
                .map(|entity| {
                    let p = world.get::<Position>(*entity).unwrap();
                    let v = world.get::<Velocity>(*entity).unwrap();
                    let vertical = world.get::<VerticalVelocity>(*entity).unwrap();
                    let grounded = world.get::<Grounded>(*entity).unwrap();
                    let settle = world.get::<RecentlySpawned>(*entity).unwrap();
                    [
                        p.x.to_bits(),
                        p.y.to_bits(),
                        p.z.to_bits(),
                        v.x.to_bits(),
                        v.y.to_bits(),
                        v.z.to_bits(),
                        vertical.0.to_bits(),
                        grounded.0 as u64,
                        settle.0 as u64,
                    ]
                })
                .collect();
            Run { state, flyers, app }
        }

        #[test]
        fn region_parallel_physics_matches_the_single_threaded_baseline() {
            let baseline = run(ExecutionMode::SingleThreaded);
            let parallel = run(ExecutionMode::MultiThreaded);

            for (i, (serial, multi)) in baseline.state.iter().zip(&parallel.state).enumerate() {
                assert_eq!(serial, multi, "entity {i} diverged from the baseline");
            }
            assert_eq!(baseline.state.len(), parallel.state.len());

            // The scenario exercises falling, landing, stepping and walls:
            // 6 of 16 entities per chunk start without z velocity, so more
            // than 18 without it means some ran into the wall.
            let y = |state: &[u64; 9]| f64::from_bits(state[1]);
            assert!(baseline.state.iter().any(|s| y(s) == 64.0 && s[7] == 1));
            assert!(baseline.state.iter().any(|s| y(s) == 65.0 && s[7] == 1));
            let stopped = |state: &&[u64; 9]| f64::from_bits(state[5]) == 0.0;
            assert!(baseline.state.iter().filter(stopped).count() > 18);
        }

        #[test]
        fn every_tick_steps_every_entity_exactly_once_in_both_modes() {
            for mode in [ExecutionMode::SingleThreaded, ExecutionMode::MultiThreaded] {
                let run = run(mode);
                assert_eq!(run.flyers.len(), 3 * 3);
                for (entity, start, velocity) in &run.flyers {
                    // A flyer moves by its velocity once per step, so its
                    // position counts the steps it took.
                    let mut expected = *start;
                    for _ in 0..TICKS {
                        expected.x += velocity.x;
                        expected.z += velocity.z;
                    }
                    assert_eq!(
                        *run.app.world().get::<Position>(*entity).unwrap(),
                        expected,
                        "{mode:?}: an entity was stepped more or fewer than {TICKS} times"
                    );
                }
            }
        }
    }
}
