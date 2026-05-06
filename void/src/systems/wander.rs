use bevy_ecs::prelude::*;
use rand::Rng;

use crate::components::{Position, Rotation, Velocity, Wander, SpawnedEntity};

/// Very small, deterministic-seeming wander AI for demonstration.
pub fn wander_system(
    mut query: Query<(&mut Position, &mut Rotation, &mut Velocity, &mut Wander), With<SpawnedEntity>>,
) {
    for (mut pos, mut rot, mut vel, mut wander) in query.iter_mut() {
        // tick down
        wander.ticks -= 1;

        if wander.ticks <= 0 {
            let mut rng = rand::thread_rng();
            wander.yaw = rng.gen_range(0.0..360.0) as f32;
            wander.ticks = rng.gen_range(40..140);
        }

        // Move forward along yaw. Note: yaw is degrees; convert to radians.
        let yaw_radians = wander.yaw.to_radians();
        let dx = yaw_radians.sin() as f64 * wander.speed;
        let dz = yaw_radians.cos() as f64 * wander.speed;

        pos.x += dx;
        pos.z += dz;

        // keep y unchanged for this simple example

        // Face the actual movement direction. Use motion vector to compute yaw
        // with the same axis convention as the protocol: yaw = atan2(-dx, dz)
        if dx != 0.0 || dz != 0.0 {
            let yaw = (-dx).atan2(dz).to_degrees();
            // Normalize into 0..360
            rot.yaw = if yaw < 0.0 { yaw + 360.0 } else { yaw } as f32;
        } else {
            rot.yaw = wander.yaw;
        }

        // Write velocity in protocol units: 1/8000 blocks per tick mapped to i16
        let vx = (dx * 8000.0).round() as i16;
        let vy = 0i16;
        let vz = (dz * 8000.0).round() as i16;

        vel.x = vx;
        vel.y = vy;
        vel.z = vz;
    }
}
