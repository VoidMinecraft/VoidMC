use bevy_ecs::prelude::*;
use rand::Rng;
use tracing::instrument;

use crate::components::{Rotation, SpawnedEntity, Velocity, Wander};
use crate::pace::Chance;

/// Very small, deterministic-seeming wander AI for demonstration.
#[instrument(name = "mob_ai_wander", level = "info", skip(query, chance))]
pub fn wander_system(
    mut chance: Chance,
    mut query: Query<(&mut Rotation, &mut Velocity, &mut Wander), With<SpawnedEntity>>,
) {
    for (mut rot, mut vel, mut wander) in query.iter_mut() {
        // tick down
        wander.ticks -= 1;

        if wander.ticks <= 0 {
            (wander.yaw, wander.ticks) =
                chance.roll(|rng| (rng.gen_range(0.0..360.0) as f32, rng.gen_range(40..140)));
        }

        // Move forward along yaw. Note: yaw is degrees; convert to radians.
        let yaw_radians = wander.yaw.to_radians();
        let dx = yaw_radians.sin() as f64 * wander.speed;
        let dz = yaw_radians.cos() as f64 * wander.speed;

        // Face the actual movement direction. Use motion vector to compute yaw
        // with the same axis convention as the protocol: yaw = atan2(-dx, dz)
        if dx != 0.0 || dz != 0.0 {
            let yaw = (-dx).atan2(dz).to_degrees();
            // Normalize into 0..360
            rot.yaw = if yaw < 0.0 { yaw + 360.0 } else { yaw } as f32;
        } else {
            rot.yaw = wander.yaw;
        }

        if vel.x != dx || vel.z != dz {
            vel.x = dx;
            vel.z = dz;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy_app::{App, Update};

    use super::*;
    use crate::pace::SteppedPacePlugin;

    #[test]
    fn two_stepped_servers_wander_the_same_way() {
        let walk = || {
            let mut app = App::new();
            app.add_plugins(SteppedPacePlugin::new(Duration::from_millis(50)))
                .add_systems(Update, wander_system);
            let mobs: Vec<Entity> = (0..4)
                .map(|_| {
                    app.world_mut()
                        .spawn((
                            SpawnedEntity(()),
                            Rotation::default(),
                            Velocity::default(),
                            Wander {
                                ticks: 1,
                                speed: 0.08,
                                yaw: 0.0,
                            },
                        ))
                        .id()
                })
                .collect();
            app.update();
            mobs.iter()
                .map(|mob| {
                    let wander = app.world().get::<Wander>(*mob).unwrap();
                    (wander.yaw.to_bits(), wander.ticks)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(walk(), walk());
    }
}
