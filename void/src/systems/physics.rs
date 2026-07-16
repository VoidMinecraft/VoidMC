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
}
