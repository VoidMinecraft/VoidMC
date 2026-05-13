use bevy_ecs::prelude::*;

use crate::components::{CirclePig, CirclePigState, Position, Rotation};

const CIRCLE_RADIUS: f64 = 2.0;
const ROTATION_SPEED_DEG: f32 = 2.0;

pub fn circle_system(
    mut pigs: Query<(&mut Position, &mut Rotation, &mut CirclePigState), With<CirclePig>>,
    targets: Query<&Position, Without<CirclePig>>,
) {
    for (mut pig_pos, mut pig_rot, mut state) in pigs.iter_mut() {
        state.angle = (state.angle + ROTATION_SPEED_DEG) % 360.0;

        let Ok(target_pos) = targets.get(state.target) else {
            continue;
        };

        let angle_rad = (state.angle as f64).to_radians();
        pig_pos.x = target_pos.x + angle_rad.sin() * CIRCLE_RADIUS;
        pig_pos.z = target_pos.z + angle_rad.cos() * CIRCLE_RADIUS;
        pig_pos.y = target_pos.y;

        // Face tangentially (CCW direction of travel): yaw = atan2(-cos θ, -sin θ)
        let a = state.angle.to_radians();
        let yaw = (-(a.cos())).atan2(-(a.sin())).to_degrees();
        pig_rot.yaw = if yaw < 0.0 { yaw + 360.0 } else { yaw };
    }
}
