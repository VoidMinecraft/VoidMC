//! What falls on a region of the map. The client draws its own rain and snow only around
//! its camera, so a map that shows more than one region at a time has to say it with
//! particles, over the land they fall on.

use voidmc_protocol::clientbound::{
    GameEvent, GameEventType, LevelParticles, Particle, ParticleColor,
};

use crate::noise::scatter;
use crate::regions::Climate;
use crate::scene::Scene;

const SPOT_SEED: u32 = 300;
/// Places tried on each tick of a view. Those in a clear region stay empty.
const SPOTS_PER_TICK: i32 = 10;
/// Particles at each.
const DROPS_PER_SPOT: i32 = 2;
/// How far apart they start, in blocks.
const SPOT_SPREAD: f32 = 1.5;
/// A drop is a pale dot, which only goes down if it is told to: with no count, a packet
/// makes one particle, that its offset and speed send on its way.
const DROP_COLOUR: i32 = 0xb4dcff;
const DROP_SIZE: f32 = 1.8;
const DROP_SPEED: f32 = 9.0;
/// How far from the eye they fall, in blocks. A flake is a few pixels wide at best, so
/// that it would not show from up here on the land itself: it falls on the line from the
/// eye to the land, where it shows over the region and much larger.
const NEAR_THE_EYE: [f64; 2] = [16.0, 40.0];
/// How much of a shower the client shows in [`Weather::Sky`], out of 1.
const SKY_LEVEL: f32 = 0.5;

/// How a map shows the climate of its regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Weather {
    /// It shows nothing.
    #[default]
    Clear,
    /// Flakes and drops over the regions that have them, wherever they are on screen.
    Particles,
    /// The rain and snow of the client, around the camera, once it is over such a region.
    Sky,
}

impl Weather {
    pub const ALL: [Self; 3] = [Self::Clear, Self::Particles, Self::Sky];

    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Aucune",
            Self::Particles => "Flocons et gouttes",
            Self::Sky => "Pluie et neige du jeu",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|one| *one == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// What tells a client to show its rain and snow, or to stop.
    pub fn sky_events(raining: bool) -> [GameEvent; 2] {
        [
            GameEvent {
                event: if raining {
                    GameEventType::BeginRaining
                } else {
                    GameEventType::EndRaining
                },
                value: 0.0,
            },
            GameEvent {
                event: GameEventType::RainLevelChange,
                value: if raining { SKY_LEVEL } else { 0.0 },
            },
        ]
    }
}

/// What falls over a view this tick. All of it is in blocks of the model, for the caller
/// to place: `middle` is the land the eye looks at.
pub fn falling(
    scene: &Scene,
    eye: [f64; 3],
    middle: [f64; 2],
    reach: f64,
    tick: u32,
) -> Vec<LevelParticles> {
    (0..SPOTS_PER_TICK)
        .filter_map(|spot| {
            let [across, along, near, _] = scatter(SPOT_SEED, tick as i32, spot);
            let x = middle[0] + (across * 2.0 - 1.0) * reach;
            let z = middle[1] + (along * 2.0 - 1.0) * reach;
            let (cell_x, cell_z) = (x.floor() as i32, z.floor() as i32);
            let top = f64::from(scene.top(cell_x, cell_z)?);
            let climate = scene.climate(cell_x, cell_z)?;
            if climate == Climate::Clear {
                return None;
            }
            let way = [x - eye[0], top - eye[1], z - eye[2]];
            let length = way.iter().map(|part| part * part).sum::<f64>().sqrt();
            let from_eye = NEAR_THE_EYE[0] + (NEAR_THE_EYE[1] - NEAR_THE_EYE[0]) * near;
            let share = (from_eye / length).min(1.0);
            let at = [
                eye[0] + way[0] * share,
                eye[1] + way[1] * share,
                eye[2] + way[2] * share,
            ];
            Some(fall(climate, at))
        })
        .collect()
}

fn fall(climate: Climate, [x, y, z]: [f64; 3]) -> LevelParticles {
    let (offset, max_speed, count, particle) = if climate == Climate::Rain {
        let drop = Particle::Dust {
            color: ParticleColor(DROP_COLOUR),
            scale: DROP_SIZE,
        };
        ([0.0, -1.0, 0.0], DROP_SPEED, 0, drop)
    } else {
        let spread = [SPOT_SPREAD; 3];
        (spread, 0.0, DROPS_PER_SPOT, Particle::Snowflake)
    };
    LevelParticles {
        long_distance: true,
        always_visible: true,
        x,
        y,
        z,
        offset_x: offset[0],
        offset_y: offset[1],
        offset_z: offset[2],
        max_speed,
        count,
        particle,
    }
}

#[cfg(test)]
mod tests {
    use voidmc::DefaultWorldGenerator;

    use super::*;
    use crate::regions::{Region, RegionState, Regions};
    use crate::scene::DETAIL;
    use crate::terrain::Terrain;

    fn scene_under(climate: Climate) -> Scene {
        let regions = Regions::new(vec![Region {
            name: "Tout",
            site: [0.0, 0.0],
            state: RegionState::Discovered,
            climate,
        }]);
        let terrain = Terrain::sample(&DefaultWorldGenerator::default(), &DETAIL.window());
        Scene::new(&terrain, 62, DETAIL, &regions)
    }

    fn over(scene: &Scene) -> Vec<LevelParticles> {
        (0..40)
            .flat_map(|tick| falling(scene, [0.0, 150.0, 20.0], [0.0, 0.0], 80.0, tick))
            .collect()
    }

    #[test]
    fn nothing_falls_on_a_clear_region() {
        assert!(over(&scene_under(Climate::Clear)).is_empty());
    }

    #[test]
    fn each_climate_has_its_own_particle() {
        let first = |climate| over(&scene_under(climate)).remove(0).particle;
        assert_eq!(first(Climate::Snow), Particle::Snowflake);
        assert!(matches!(first(Climate::Rain), Particle::Dust { .. }));
    }

    /// Any further and a flake is smaller than a pixel.
    #[test]
    fn what_falls_stays_near_the_eye() {
        for drop in over(&scene_under(Climate::Snow)) {
            let from_eye = drop.x.hypot(drop.y - 150.0).hypot(drop.z - 20.0);
            assert!(
                (NEAR_THE_EYE[0]..=NEAR_THE_EYE[1]).contains(&from_eye),
                "{from_eye}"
            );
        }
    }

    #[test]
    fn the_rain_of_the_client_comes_and_goes_at_once() {
        let [on, level] = Weather::sky_events(true);
        assert!(matches!(on.event, GameEventType::BeginRaining));
        assert_eq!(level.value, SKY_LEVEL);
        assert_eq!(Weather::sky_events(false)[1].value, 0.0);
    }

    #[test]
    fn every_choice_goes_round() {
        assert_eq!(Weather::Sky.next(), Weather::Clear);
    }
}
