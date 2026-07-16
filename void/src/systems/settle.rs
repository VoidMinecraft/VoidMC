use bevy_ecs::prelude::*;
use tracing::instrument;

use crate::components::{
    EntityDimension, Grounded, MovementConfig, Position, PreviousPosition, RecentlySpawned,
    SpawnedEntity, VerticalVelocity,
};
use crate::world::{
    ChunkData, ChunkIndex, ChunkPosition, block_state_at_world, is_solid_block_state,
};

/// Settle newly spawned gravity-enabled entities by scanning downward and snapping them
/// onto the first solid block found within `MAX_SCAN` blocks.
#[instrument(
    name = "entity_spawn_settling",
    level = "info",
    skip(chunk_index, chunks, query)
)]
pub fn settle_recent_spawns(
    chunk_index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    mut query: Query<
        (
            &mut Position,
            &mut PreviousPosition,
            &MovementConfig,
            &EntityDimension,
            &mut VerticalVelocity,
            &mut Grounded,
            &mut RecentlySpawned,
        ),
        With<SpawnedEntity>,
    >,
) {
    const MAX_SCAN: i32 = 64;

    for (mut pos, mut prev_pos, movement, dimension, mut velocity, mut grounded, mut marker) in
        query.iter_mut()
    {
        if marker.0 == 0 {
            continue;
        }

        marker.0 -= 1;

        if !movement.gravity_enabled {
            marker.0 = 0;
            continue;
        }

        // Only stationary summons are eligible for the initial ground snap.
        // Thrown item entities have an intentional launch velocity and must
        // follow their normal arc instead of teleporting to the ground.
        if velocity.0.abs() > f64::EPSILON {
            continue;
        }

        let start_y = pos.y.floor() as i32 - 1;
        let min_y = start_y - MAX_SCAN;
        let tx = pos.x.floor() as i32;
        let tz = pos.z.floor() as i32;

        for y in (min_y..=start_y).rev() {
            if let Some(block_state) =
                block_state_at_world(&chunk_index, &chunks, dimension.0, tx, y, tz)
            {
                if is_solid_block_state(block_state) {
                    let ground_y = (y as f64) + 1.0;
                    let fall_distance = prev_pos.y - ground_y;

                    if fall_distance > 0.1 {
                        pos.y = ground_y;
                        prev_pos.y = ground_y;
                        velocity.0 = 0.0;
                        grounded.0 = true;
                        marker.0 = 0;
                        break;
                    }
                }
            }
        }
    }
}
