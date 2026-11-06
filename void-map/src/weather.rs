//! What fills the air over a region of the map. The client draws its own rain and snow only
//! around its camera, so a map that shows more than one region at a time has to say it with
//! particles, over the land they fall on. The other effects are particles too, each with
//! the land it belongs to.

use voidmc_protocol::clientbound::{
    GameEvent, GameEventType, LevelParticles, Particle, ParticleColor,
};

use crate::noise::scatter;
use crate::regions::Climate;
use crate::scene::Scene;

const SPOT_SEED: u32 = 300;
/// Places tried on each tick of a view by rain and snow. Those in a clear region stay empty.
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
/// World blocks per cell of the close model, which the number of places is meant for.
const CLOSE_CELL: i32 = 3;
/// The colours of leaves in the wind, picked one by one.
const LEAF_COLOURS: [i32; 3] = [0xffa63a, 0xff6a32, 0xffd84a];
/// Where the wind takes a leaf: mostly east.
const WIND: [f32; 3] = [1.0, 0.0, 0.25];
const WIND_SPEED: f32 = 0.9;
/// How fast an ember goes up.
const EMBER_SPEED: f32 = 0.05;

/// How a map shows what is in the air over its regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Weather {
    /// The rain and snow of the client, around the camera, once it is over such a region.
    #[default]
    Sky,
    /// Flakes and drops over the regions that have them, wherever they are on screen.
    Particles,
    /// Autumn leaves that the wind takes over the woods.
    Leaves,
    /// Small flames over the closed regions.
    Embers,
    /// Glints in the cold air of the snowy regions.
    Frost,
    /// It shows nothing.
    Clear,
}

/// A way of filling the air: on what land, how much of it and how far from the eye.
struct Effect {
    /// Whether it belongs over a column.
    zone: fn(&Scene, i32, i32) -> bool,
    /// Places tried on each tick of a view.
    spots: i32,
    near: [f64; 2],
    /// What goes out at a place, from a roll between 0 and 1.
    draw: fn(Climate, [f64; 3], f64) -> LevelParticles,
}

impl Weather {
    /// The first one is what a map opens with.
    pub const ALL: [Self; 6] = [
        Self::Sky,
        Self::Particles,
        Self::Leaves,
        Self::Embers,
        Self::Frost,
        Self::Clear,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sky => "Pluie et neige du jeu",
            Self::Particles => "Flocons et gouttes",
            Self::Leaves => "Feuilles au vent",
            Self::Embers => "Braises",
            Self::Frost => "Givre",
            Self::Clear => "Aucune",
        }
    }

    /// Whether the server has particles to send for it.
    pub fn has_particles(self) -> bool {
        self.effect().is_some()
    }

    fn effect(self) -> Option<Effect> {
        Some(match self {
            Self::Sky | Self::Clear => return None,
            Self::Particles => Effect {
                zone: |scene, x, z| scene.climate(x, z).is_some_and(|c| c != Climate::Clear),
                spots: SPOTS_PER_TICK,
                near: NEAR_THE_EYE,
                draw: fall,
            },
            Self::Leaves => Effect {
                zone: Scene::wooded,
                spots: 14,
                near: [8.0, 26.0],
                draw: leaf,
            },
            Self::Embers => Effect {
                zone: Scene::closed,
                spots: 30,
                near: [8.0, 26.0],
                draw: |_, at, _| {
                    let mut flame = still(at, Particle::SmallFlame, 0.0, 0);
                    (flame.offset_y, flame.max_speed) = (1.0, EMBER_SPEED);
                    flame
                },
            },
            Self::Frost => Effect {
                zone: |scene, x, z| scene.climate(x, z) == Some(Climate::Snow),
                spots: 30,
                near: [8.0, 26.0],
                draw: |_, at, roll| {
                    let particle = if roll < 0.3 {
                        Particle::ElectricSpark
                    } else {
                        Particle::Glow
                    };
                    still(at, particle, 3.0, 1)
                },
            },
        })
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

/// What fills the air over a view this tick. All of it is in blocks of the model, for the
/// caller to place: `middle` is the land the eye looks at.
pub fn falling(
    weather: Weather,
    scene: &Scene,
    eye: [f64; 3],
    middle: [f64; 2],
    reach: f64,
    tick: u32,
) -> Vec<LevelParticles> {
    let Some(effect) = weather.effect() else {
        return Vec::new();
    };
    // A coarser model is seen from further: as many places would show as fewer on screen.
    let spots = effect.spots * i32::from(scene.scale().blocks_per_cell) / CLOSE_CELL;
    (0..spots)
        .filter_map(|spot| {
            let [across, along, near, roll] = scatter(SPOT_SEED, tick as i32, spot);
            let x = middle[0] + (across * 2.0 - 1.0) * reach;
            let z = middle[1] + (along * 2.0 - 1.0) * reach;
            let (cell_x, cell_z) = (x.floor() as i32, z.floor() as i32);
            let top = f64::from(scene.top(cell_x, cell_z)?);
            let climate = scene.climate(cell_x, cell_z)?;
            if !(effect.zone)(scene, cell_x, cell_z) {
                return None;
            }
            let way = [x - eye[0], top - eye[1], z - eye[2]];
            let length = way.iter().map(|part| part * part).sum::<f64>().sqrt();
            let from_eye = effect.near[0] + (effect.near[1] - effect.near[0]) * near;
            let share = (from_eye / length).min(1.0);
            let at = [
                eye[0] + way[0] * share,
                eye[1] + way[1] * share,
                eye[2] + way[2] * share,
            ];
            Some((effect.draw)(climate, at, roll))
        })
        .collect()
}

/// `count` particles around a place, on their own, or one sent on its way with none.
fn still([x, y, z]: [f64; 3], particle: Particle, spread: f32, count: i32) -> LevelParticles {
    LevelParticles {
        long_distance: true,
        always_visible: true,
        x,
        y,
        z,
        offset_x: spread,
        offset_y: spread,
        offset_z: spread,
        max_speed: 0.0,
        count,
        particle,
    }
}

fn leaf(_: Climate, at: [f64; 3], roll: f64) -> LevelParticles {
    let colour = LEAF_COLOURS[(roll * LEAF_COLOURS.len() as f64) as usize % LEAF_COLOURS.len()];
    let particle = Particle::TintedLeaves {
        color: ParticleColor(colour),
    };
    LevelParticles {
        offset_x: WIND[0],
        offset_y: WIND[1],
        offset_z: WIND[2],
        max_speed: WIND_SPEED,
        ..still(at, particle, 0.0, 0)
    }
}

fn fall(climate: Climate, at: [f64; 3], _: f64) -> LevelParticles {
    if climate == Climate::Rain {
        let drop = Particle::Dust {
            color: ParticleColor(DROP_COLOUR),
            scale: DROP_SIZE,
        };
        LevelParticles {
            offset_y: -1.0,
            max_speed: DROP_SPEED,
            ..still(at, drop, 0.0, 0)
        }
    } else {
        still(at, Particle::Snowflake, SPOT_SPREAD, DROPS_PER_SPOT)
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
            .flat_map(|tick| {
                let eye = [0.0, 150.0, 20.0];
                falling(Weather::Particles, scene, eye, [0.0, 0.0], 80.0, tick)
            })
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
        let mut weather = Weather::default();
        for _ in Weather::ALL {
            weather = weather.next();
        }
        assert_eq!(weather, Weather::default());
    }

    #[test]
    fn the_rain_of_the_client_comes_first() {
        assert_eq!(Weather::default(), Weather::Sky);
        assert_eq!(Weather::ALL[0], Weather::Sky);
    }

    #[test]
    fn the_client_alone_draws_its_sky_and_nothing_draws_nothing() {
        for weather in Weather::ALL {
            let drawn = !matches!(weather, Weather::Sky | Weather::Clear);
            assert_eq!(weather.has_particles(), drawn, "{weather:?}");
        }
    }

    fn effects_over(scene: &Scene, weather: Weather) -> Vec<LevelParticles> {
        (0..120)
            .flat_map(|tick| falling(weather, scene, [0.0, 150.0, 20.0], [0.0, 0.0], 80.0, tick))
            .collect()
    }

    /// Each one belongs to its land: there is none of it anywhere else.
    #[test]
    fn an_effect_keeps_to_its_land() {
        let open = scene_under(Climate::Clear);
        assert!(effects_over(&open, Weather::Embers).is_empty());
        assert!(effects_over(&open, Weather::Frost).is_empty());
        assert!(!effects_over(&scene_under(Climate::Snow), Weather::Frost).is_empty());
        assert!(!effects_over(&open, Weather::Leaves).is_empty());
    }

    #[test]
    fn leaves_go_with_the_wind_in_several_colours() {
        let leaves = effects_over(&scene_under(Climate::Clear), Weather::Leaves);
        let mut colours: Vec<_> = leaves
            .iter()
            .map(|leaf| match leaf.particle {
                Particle::TintedLeaves { color } => color.0,
                _ => panic!("{:?}", leaf.particle),
            })
            .collect();
        colours.sort_unstable();
        colours.dedup();
        assert_eq!(colours.len(), LEAF_COLOURS.len());
        assert!(
            leaves
                .iter()
                .all(|leaf| leaf.count == 0 && leaf.offset_x > 0.0)
        );
    }
}
