use bevy_ecs::prelude::*;
use rand::Rng;
use tracing::instrument;

use crate::components::{Rotation, SpawnedEntity, Velocity, Wander};

/// Very small, deterministic-seeming wander AI for demonstration.
#[instrument(name = "mob_ai_wander", level = "info", skip(query))]
pub fn wander_system(
    mut query: Query<(&mut Rotation, &mut Velocity, &mut Wander), With<SpawnedEntity>>,
) {
    for (mut rot, mut vel, mut wander) in query.iter_mut() {
        // tick down
        wander.ticks -= 1;

        if wander.ticks <= 0 {
            let mut rng = rand::thread_rng();
            wander.yaw = rng.gen_range(0.0..360.0) as f32;
            wander.ticks = rng.gen_range(Wander::TICKS);
        }

        // Move forward along yaw.
        let (sin, cos) = heading(wander.yaw);
        let dx = sin * wander.speed;
        let dz = cos * wander.speed;

        // Face the actual movement direction. Use motion vector to compute yaw
        // with the same axis convention as the protocol: yaw = atan2(-dx, dz)
        let yaw = if dx != 0.0 || dz != 0.0 {
            let yaw = (-dx).atan2(dz).to_degrees();
            // Normalize into 0..360
            (if yaw < 0.0 { yaw + 360.0 } else { yaw }) as f32
        } else {
            wander.yaw
        };
        // Written only when it changes, so that a heading kept is sent as
        // position-only moves.
        if rot.yaw != yaw {
            rot.yaw = yaw;
        }

        if vel.x != dx || vel.z != dz {
            vel.x = dx;
            vel.z = dz;
        }
    }
}

/// The sine and cosine of a yaw in degrees, exact on the four axis-aligned
/// headings: an entity walking along an axis has no sideways component, so
/// once a wall stops it, it does not move at all.
fn heading(yaw: f32) -> (f64, f64) {
    let degrees = f64::from(yaw).rem_euclid(360.0);
    let quadrant = (degrees / 90.0).floor();
    let (sin, cos) = (degrees - quadrant * 90.0).to_radians().sin_cos();
    match quadrant as u8 % 4 {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_aligned_headings_have_no_sideways_component() {
        for (yaw, expected) in [
            (0.0, (0.0, 1.0)),
            (90.0, (1.0, 0.0)),
            (180.0, (0.0, -1.0)),
            (270.0, (-1.0, 0.0)),
            (360.0, (0.0, 1.0)),
            (-90.0, (-1.0, 0.0)),
        ] {
            assert_eq!(heading(yaw), expected, "yaw {yaw}");
        }
    }

    #[test]
    fn heading_matches_sine_and_cosine() {
        for tenth in 0..3600 {
            let yaw = tenth as f32 / 10.0;
            let (sin, cos) = heading(yaw);
            let (expected_sin, expected_cos) = f64::from(yaw).to_radians().sin_cos();
            assert!(
                (sin - expected_sin).abs() < 1e-12 && (cos - expected_cos).abs() < 1e-12,
                "yaw {yaw}: ({sin}, {cos}), expected ({expected_sin}, {expected_cos})"
            );
        }
    }

    #[test]
    fn heading_kept_leaves_rotation_unchanged() {
        let mut world = World::new();
        let entity = world
            .spawn((
                SpawnedEntity(()),
                Rotation::default(),
                Velocity::default(),
                Wander {
                    ticks: 100,
                    speed: Wander::SPEED,
                    yaw: 37.0,
                },
            ))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(wander_system);
        schedule.run(&mut world);
        world.clear_trackers();

        // The heading is kept: the entity keeps walking, but its rotation is
        // not written again.
        let first = *world.get::<Rotation>(entity).unwrap();
        schedule.run(&mut world);
        let rotation = world.entity(entity).get_ref::<Rotation>().unwrap();
        assert!(!rotation.is_changed(), "rotation rewritten");
        assert_eq!(*rotation, first);
    }
}
