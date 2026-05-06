use bevy_ecs::prelude::*;

use crate::components::{EntityDimension, MovementConfig, MovementUpdateCooldown, Position, PreviousPosition, RecentlySpawned, SpawnedEntity, VerticalVelocity};
use crate::world::{block_state_at_world, is_solid_block_state, ChunkData, ChunkIndex, ChunkPosition};

/// Settle newly spawned gravity-enabled entities by scanning downward and snapping them
/// onto the first solid block found within `MAX_SCAN` blocks.
pub fn settle_recent_spawns(
    chunk_index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    mut query: Query<(
        &mut Position,
        &mut PreviousPosition,
        &MovementConfig,
        &EntityDimension,
        &VerticalVelocity,
        &mut RecentlySpawned,
        &mut MovementUpdateCooldown,
    ), With<SpawnedEntity>>,
) {
    const MAX_SCAN: i32 = 64;

    for (mut pos, mut prev_pos, movement, dimension, velocity, mut marker, mut cooldown) in query.iter_mut() {
        if marker.0 == 0 {
            continue;
        }

        marker.0 -= 1;

        if !movement.gravity_enabled {
            marker.0 = 0;
            continue;
        }

        if velocity.0 < 0.0 {
            continue;
        }

        let start_y = pos.y.floor() as i32 - 1;
        let min_y = start_y - MAX_SCAN;
        let tx = pos.x.floor() as i32;
        let tz = pos.z.floor() as i32;

        for y in (min_y..=start_y).rev() {
            if let Some(block_state) = block_state_at_world(&chunk_index, &chunks, dimension.0, tx, y, tz) {
                if is_solid_block_state(block_state) {
                    let ground_y = (y as f64) + 1.0;
                    let fall_distance = prev_pos.y - ground_y;

                    if fall_distance > 0.1 {
                        pos.y = ground_y;
                        prev_pos.y = ground_y;
                        marker.0 = 0;
                        cooldown.0 = 0;
                        break;
                    }
                }
            }
        }
    }
}
