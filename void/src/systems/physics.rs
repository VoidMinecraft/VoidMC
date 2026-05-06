use bevy_ecs::prelude::*;

use crate::components::{
    EntityDimension, Grounded, MovementConfig, Position, PreviousPosition, SpawnedEntity,
    VerticalVelocity,
};
use crate::world::{block_state_at_world, is_solid_block_state, ChunkData, ChunkIndex, ChunkPosition};

const GRAVITY_STEP: f64 = 0.08;
const TERMINAL_VELOCITY: f64 = -3.92;

/// Applies a small server-authoritative physics step for spawned entities.
pub fn apply_spawned_entity_physics(
    chunk_index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    mut query: Query<(
        &mut Position,
        &mut PreviousPosition,
        &MovementConfig,
        &EntityDimension,
        &mut VerticalVelocity,
        &mut Grounded,
    ), With<SpawnedEntity>>,
) {
    for (mut position, mut previous_position, movement, dimension, mut vertical_velocity, mut grounded) in
        query.iter_mut()
    {
        let mut next_x = position.x;
        let mut next_y = position.y;
        let mut next_z = position.z;

        if movement.block_collision_enabled {
            // Axis-separated horizontal collision checks to avoid tunneling
            let prev_x = previous_position.x;
            let prev_y = previous_position.y;
            let prev_z = previous_position.z;

            // Proposed positions after horizontal motion (before vertical update)
            let prop_x = next_x;
            let prop_z = next_z;

            // Check X movement first (keeping Z at previous)
            let check_x = prop_x.floor() as i32;
            let check_y = prev_y.floor() as i32;
            let check_z = prev_z.floor() as i32;

            if block_state_at_world(&chunk_index, &chunks, dimension.0, check_x, check_y, check_z)
                .is_some_and(is_solid_block_state)
            {
                // Revert X movement
                next_x = prev_x;
            } else {
                next_x = prop_x;
            }

            // Check Z movement next (using updated X)
            let check_x2 = next_x.floor() as i32;
            let check_z2 = prop_z.floor() as i32;

            if block_state_at_world(&chunk_index, &chunks, dimension.0, check_x2, check_y, check_z2)
                .is_some_and(is_solid_block_state)
            {
                // Revert Z movement
                next_z = prev_z;
            } else {
                next_z = prop_z;
            }
        }

        if movement.gravity_enabled {
            vertical_velocity.0 = (vertical_velocity.0 - GRAVITY_STEP).max(TERMINAL_VELOCITY);

            // Proposed next Y after applying gravity.
            let proposed_y = next_y + vertical_velocity.0;

            if movement.block_collision_enabled {
                let tx = next_x.floor() as i32;
                let tz = next_z.floor() as i32;

                let prev_floor = previous_position.y.floor() as i32;
                let prop_floor = proposed_y.floor() as i32;

                if prop_floor < prev_floor {
                    // Falling: sweep from just below previous floor down to proposed floor
                    let mut collided: Option<i32> = None;
                    for y_check in (prop_floor..=prev_floor - 1).rev() {
                        if block_state_at_world(&chunk_index, &chunks, dimension.0, tx, y_check, tz)
                            .is_some_and(is_solid_block_state)
                        {
                            collided = Some(y_check);
                            break;
                        }
                    }

                    if let Some(cy) = collided {
                        // Place entity on top of the collided block and snap previous_position
                        next_y = (cy as f64) + 1.0;
                        vertical_velocity.0 = 0.0;
                        grounded.0 = true;
                        previous_position.y = next_y;
                    } else {
                        next_y = proposed_y;
                        grounded.0 = false;
                    }
                } else if prop_floor > prev_floor {
                    // Moving up: simple single-check collision
                    if block_state_at_world(&chunk_index, &chunks, dimension.0, tx, prop_floor, tz)
                        .is_some_and(is_solid_block_state)
                    {
                        // Hit head on a block — revert to previous Y and snap previous_position
                        next_y = previous_position.y;
                        vertical_velocity.0 = 0.0;
                        grounded.0 = false;
                        previous_position.y = next_y;
                    } else {
                        next_y = proposed_y;
                        grounded.0 = false;
                    }
                } else {
                    // Same integer level
                    next_y = proposed_y;
                    grounded.0 = false;
                }
            } else {
                next_y = proposed_y;
                grounded.0 = false;
            }
        }

        position.x = next_x;
        position.y = next_y;
        position.z = next_z;
    }
}
