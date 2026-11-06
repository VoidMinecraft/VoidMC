//! The scenes of a map made from the world: where there is room for them, read off the
//! [`Land`]. All that comes out is [`LifeScene`]s, as a file could hold.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use super::{Actor, Ends, Land, LifeScene, Mark, Model, Playback, Turn, Way, yaw_towards};

/// A person, at the size of a figure of the map: eight times life.
const PERSON: f64 = 14.4;
/// As wide on screen as a border, three blocks on the close model.
const RAIL_WIDTH: f64 = 9.0;
const PIER_WIDTH: f64 = 6.0;
const PIER_LENGTH: f64 = 21.0;
/// A pier reaches this far past its end for the water to be open.
const PIER_CLEAR: f64 = 18.0;
/// Where a pier can start from a place, in world blocks.
const PIER_AROUND: [f64; 2] = [30.0, 150.0];
const ANGLERS: usize = 3;

const FLOCKS: usize = 4;
const GULLS: usize = 5;
const WINGSPAN: f64 = 11.0;
/// So that a gull still shows on the far model, in its blocks.
const LEAST_GULL: f64 = 1.7;
const LOCOMOTIVE: f64 = 21.0;
const CARRIAGE: f64 = 19.0;
const BETWEEN_CARS: f64 = 1.0;
const TRAIN_SPEED: f64 = 1.1;
/// Ticks a train waits at a station.
const STATION_REST: u32 = 80;
const STATIONS: usize = 3;
/// The grid a line is searched on, in world blocks.
const GRID: f64 = 6.0;
const GRID_REACH: i32 = 80;
/// What a cell costs a line, on top of its length.
const WATER_COST: f64 = 2.0;
const TREES_COST: f64 = 5.0;
const CLIMB_COST: f64 = 3.0;
/// A leg of a loop stays away from the ones before it.
const TAKEN_COST: f64 = 30.0;
/// The turns of a line, this far apart in world blocks once smoothed.
const LINE_SPACING: f64 = 6.0;
/// How far from the rim a line stays, in world blocks.
const RIM_CLEARANCE: f64 = 24.0;
const SMOOTHING_ROUNDS: usize = 3;
/// Seeds a flock tries, for a round that stays over the land.
const FLOCK_SEEDS: u32 = 40;
/// How much room around its way a flock keeps over the land, in world blocks.
const FLOCK_ROOM: f64 = 18.0;
/// How much closer to the middle of the map than its place a flock stays.
const FLOCK_INLAND: f64 = 0.75;
/// How far a flock roams, the widest that fits.
const FLOCK_RADII: [f64; 3] = [110.0, 80.0, 55.0];
const STROLLERS: usize = 2;
const STROLL_SPEED: f64 = 0.14;
/// How far from a place someone strolls, in world blocks.
const STROLL_AROUND: [f64; 3] = [34.0, 46.0, 60.0];
const STROLL_TURNS: usize = 12;
const WALK_CHECK: f64 = 3.0;

/// The scenes of a map. `open` says which places of the world are there to be seen, and
/// `places` are its waypoints.
pub fn scenes_of(
    land: &impl Land,
    places: &[[f64; 2]],
    open: &impl Fn([f64; 2]) -> bool,
) -> Vec<LifeScene> {
    let places: Vec<[f64; 2]> = places.iter().copied().filter(|at| open(*at)).collect();
    let mut scenes = Vec::new();
    scenes.extend(flocks(land, &places));
    scenes.extend(train(land, &places, open));
    scenes.extend(anglers(land, &places, open));
    scenes.extend(strollers(land, &places));
    scenes
}

/// Whether a whole round of a scene stays over the land, with room around it.
fn over_the_land(land: &impl Land, scene: &LifeScene) -> bool {
    let play = Playback::new(scene);
    (0..play.length as u32 / 8).all(|step| {
        let [x, z] = play.along(f64::from(step) * 8.0).0;
        AROUND.iter().all(|[dx, dz]| {
            let room = [f64::from(*dx) * FLOCK_ROOM, f64::from(*dz) * FLOCK_ROOM];
            land.ground([x + room[0], z + room[1]]).is_some()
        })
    })
}

fn flocks(land: &impl Land, places: &[[f64; 2]]) -> Vec<LifeScene> {
    let flock = |index: usize, middle: [f64; 2], radius: f64, seed: u32| {
        let beats = [Model::Gull, Model::GullOffBeat, Model::GullLastBeat];
        let cast = (0..GULLS).map(|gull| {
            let angle = gull as f64 * 2.4;
            let far = 5.0 + gull as f64 * 3.5;
            let mut actor = Actor::new(beats[gull % beats.len()], WINGSPAN);
            actor.at_rest = Some(Model::GullPerched);
            actor.least = LEAST_GULL;
            actor.offset = [angle.cos() * far, angle.sin() * far];
            actor.drift = 4.0;
            actor.bob = 1.5;
            actor
        });
        LifeScene {
            name: format!("Mouettes {}", index + 1),
            way: Way::Zone {
                middle,
                radius,
                hops: 10,
                height: 34.0,
                perch_every: 5,
                rest: 160,
                speed: 1.0,
                seed,
            },
            cast: cast.collect(),
            marks: Vec::new(),
        }
    };
    places
        .iter()
        .cycle()
        .take(FLOCKS.min(places.len() * 2))
        .enumerate()
        .filter_map(|(index, at)| {
            // Twice over a place, they keep to different sides of it.
            let side = if index < places.len() { 20.0 } else { -50.0 };
            let seeds = (0..FLOCK_SEEDS).map(|seed| 500 + (index as u32 * FLOCK_SEEDS + seed) * 10);
            let middle = [at[0] * FLOCK_INLAND + side, at[1] * FLOCK_INLAND - side];
            FLOCK_RADII
                .iter()
                .flat_map(|radius| {
                    seeds
                        .clone()
                        .map(|seed| flock(index, middle, *radius, seed))
                })
                .find(|scene| over_the_land(land, scene))
        })
        .collect()
}

fn cell_of(at: [f64; 2]) -> [i32; 2] {
    at.map(|value| (value / GRID).round() as i32)
}

fn middle_of(cell: [i32; 2]) -> [f64; 2] {
    cell.map(|value| f64::from(value) * GRID)
}

const AROUND: [[i32; 2]; 8] = [
    [1, 0],
    [-1, 0],
    [0, 1],
    [0, -1],
    [1, 1],
    [1, -1],
    [-1, 1],
    [-1, -1],
];

/// Whether a spot is clear of the rim, which a coarser model draws elsewhere.
fn inland(land: &impl Land, at: [f64; 2]) -> bool {
    AROUND.iter().all(|[x, z]| {
        let room = [f64::from(*x), f64::from(*z)].map(|side| side * RIM_CLEARANCE);
        land.ground([at[0] + room[0], at[1] + room[1]]).is_some()
    })
}

/// The cheapest line between two cells, or `None`.
fn line(
    land: &impl Land,
    open: &impl Fn([f64; 2]) -> bool,
    taken: &[[i32; 2]],
    from: [i32; 2],
    to: [i32; 2],
) -> Option<Vec<[i32; 2]>> {
    let mut costs = BTreeMap::from([(from, 0u64)]);
    let mut came_from = BTreeMap::new();
    let mut queue = BinaryHeap::from([(Reverse(0u64), from)]);
    while let Some((Reverse(cost), at)) = queue.pop() {
        if at == to {
            let mut cells = vec![at];
            while let Some(before) = came_from.get(cells.last()?) {
                cells.push(*before);
            }
            cells.reverse();
            return Some(cells);
        }
        if costs.get(&at).is_some_and(|best| cost > *best) {
            continue;
        }
        let here = land.ground(middle_of(at))?;
        for [dx, dz] in AROUND {
            let next = [at[0] + dx, at[1] + dz];
            if next[0].abs() > GRID_REACH || next[1].abs() > GRID_REACH {
                continue;
            }
            let spot = middle_of(next);
            let Some(ground) = land.ground(spot).filter(|_| open(spot)) else {
                continue;
            };
            if !inland(land, spot) {
                continue;
            }
            let length = f64::from(dx).hypot(f64::from(dz));
            let mut step = 1.0;
            if land.water(spot) {
                step += WATER_COST;
            } else if !land.free(spot) {
                step += TREES_COST;
            }
            step += (ground - here).abs() * CLIMB_COST;
            if taken.contains(&next) {
                step += TAKEN_COST;
            }
            let total = cost + (step * length * 100.0) as u64;
            if costs.get(&next).is_none_or(|best| total < *best) {
                costs.insert(next, total);
                came_from.insert(next, at);
                queue.push((Reverse(total), next));
            }
        }
    }
    None
}

/// Rounds the corners of a closed line, by cutting them.
fn smoothed(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    for _ in 0..SMOOTHING_ROUNDS {
        let count = points.len();
        points = (0..count)
            .flat_map(|at| {
                let (from, to) = (points[at], points[(at + 1) % count]);
                let part = |share: f64| {
                    [
                        from[0] + (to[0] - from[0]) * share,
                        from[1] + (to[1] - from[1]) * share,
                    ]
                };
                [part(0.25), part(0.75)]
            })
            .collect();
    }
    points
}

/// The same closed line, with its turns evenly apart.
fn evenly(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut spaced = vec![points[0]];
    let mut since = 0.0;
    for at in 0..points.len() {
        let (from, to) = (points[at], points[(at + 1) % points.len()]);
        let leg = (to[0] - from[0]).hypot(to[1] - from[1]);
        let mut along = LINE_SPACING - since;
        while along < leg {
            let share = along / leg;
            spaced.push([
                from[0] + (to[0] - from[0]) * share,
                from[1] + (to[1] - from[1]) * share,
            ]);
            along += LINE_SPACING;
        }
        since = leg - (along - LINE_SPACING);
    }
    spaced
}

/// A locomotive and its train, round the places, stopping at each.
fn train(
    land: &impl Land,
    places: &[[f64; 2]],
    open: &impl Fn([f64; 2]) -> bool,
) -> Option<LifeScene> {
    let mean_of = |places: &[[f64; 2]]| {
        let count = places.len().max(1) as f64;
        let sum = places
            .iter()
            .fold([0.0, 0.0], |sum, at| [sum[0] + at[0], sum[1] + at[1]]);
        [sum[0] / count, sum[1] / count]
    };
    let mut stations: Vec<[f64; 2]> = places
        .iter()
        .copied()
        .filter(|at| inland(land, *at))
        .collect();
    let middle = mean_of(&stations);
    let away = |at: &[f64; 2]| (at[0] - middle[0]).hypot(at[1] - middle[1]);
    stations.sort_by(|a, b| away(a).total_cmp(&away(b)));
    stations.truncate(STATIONS);
    if stations.len() < STATIONS {
        return None;
    }
    let middle = mean_of(&stations);
    stations.sort_by(|a, b| {
        let around = |at: &[f64; 2]| (at[1] - middle[1]).atan2(at[0] - middle[0]);
        around(a).total_cmp(&around(b))
    });
    let cells: Vec<[i32; 2]> = stations.iter().map(|at| cell_of(*at)).collect();
    let mut way: Vec<[i32; 2]> = Vec::new();
    for at in 0..cells.len() {
        let leg = line(land, open, &way, cells[at], cells[(at + 1) % cells.len()])?;
        way.extend(leg.iter().skip(1));
    }
    let round = evenly(&smoothed(way.iter().map(|cell| middle_of(*cell)).collect()));
    let closest = |to: [f64; 2]| {
        let away = |at: &[f64; 2]| (at[0] - to[0]).hypot(at[1] - to[1]);
        (0..round.len()).min_by(|a, b| away(&round[*a]).total_cmp(&away(&round[*b])))
    };
    let stops: Vec<usize> = stations.iter().filter_map(|at| closest(*at)).collect();
    let turns = round.iter().enumerate().map(|(index, at)| Turn {
        at: *at,
        above: 0.0,
        rest: if stops.contains(&index) {
            STATION_REST
        } else {
            0
        },
    });
    let mut cast = vec![Actor::new(Model::Locomotive, LOCOMOTIVE)];
    let mut behind = LOCOMOTIVE;
    for model in [Model::Carriage, Model::Carriage, Model::Wagon] {
        let mut car = Actor::new(model, CARRIAGE);
        behind += BETWEEN_CARS;
        car.behind = behind;
        behind += CARRIAGE;
        cast.push(car);
    }
    Some(LifeScene {
        name: "Train".to_string(),
        way: Way::Route {
            turns: turns.collect(),
            ends: Ends::Loop,
            speed: TRAIN_SPEED,
        },
        cast,
        marks: vec![Mark::Rails { width: RAIL_WIDTH }],
    })
}

/// A spot of the shore and a way out over open water, around a place.
fn shore_near(
    land: &impl Land,
    place: [f64; 2],
    open: &impl Fn([f64; 2]) -> bool,
) -> Option<([f64; 2], [f64; 2])> {
    let directions = AROUND.map(|[x, z]| {
        let length = f64::from(x).hypot(f64::from(z));
        [f64::from(x) / length, f64::from(z) / length]
    });
    let mut far = PIER_AROUND[0];
    while far < PIER_AROUND[1] {
        for step in 0..24 {
            let angle = f64::from(step) / 24.0 * std::f64::consts::TAU;
            let at = [place[0] + angle.cos() * far, place[1] + angle.sin() * far];
            for out in directions {
                let reach = |length: f64| [at[0] + out[0] * length, at[1] + out[1] * length];
                let dry = (1..=3).all(|back| land.free(reach(-f64::from(back) * 3.0)));
                let wet = (2..=((PIER_LENGTH + PIER_CLEAR) / 3.0) as i32)
                    .all(|ahead| land.water(reach(f64::from(ahead) * 3.0)));
                if dry && wet && open(at) {
                    return Some((at, out));
                }
            }
        }
        far += GRID;
    }
    None
}

/// Someone on a pier, with a line in the water.
fn anglers(
    land: &impl Land,
    places: &[[f64; 2]],
    open: &impl Fn([f64; 2]) -> bool,
) -> Vec<LifeScene> {
    let spots = places
        .iter()
        .filter_map(|place| shore_near(land, *place, open));
    spots
        .take(ANGLERS)
        .enumerate()
        .map(|(index, (shore, out))| {
            let reach = |length: f64| [shore[0] + out[0] * length, shore[1] + out[1] * length];
            let float_ahead = PERSON * 1.2;
            let mut float = Actor::new(Model::Float, 13.0);
            float.offset = [out[0] * float_ahead, out[1] * float_ahead];
            float.least = 1.5;
            LifeScene {
                name: format!("Pêcheur {}", index + 1),
                way: Way::Still {
                    at: reach(PIER_LENGTH - 3.0),
                    facing: yaw_towards(out[0], out[1]),
                },
                cast: vec![Actor::new(Model::Angler, PERSON), float],
                marks: vec![Mark::Pier {
                    from: reach(-3.0),
                    to: reach(PIER_LENGTH),
                    width: PIER_WIDTH,
                }],
            }
        })
        .collect()
}

/// Whether someone can walk straight from one spot to another.
fn walks(land: &impl Land, from: [f64; 2], to: [f64; 2]) -> bool {
    let length = (to[0] - from[0]).hypot(to[1] - from[1]);
    let steps = (length / WALK_CHECK).ceil().max(1.0) as u32;
    (0..=steps).all(|step| {
        let share = f64::from(step) / f64::from(steps);
        land.free([
            from[0] + (to[0] - from[0]) * share,
            from[1] + (to[1] - from[1]) * share,
        ])
    })
}

/// The longest stretch of a ring around a place that can be walked.
fn stroll_around(land: &impl Land, place: [f64; 2]) -> Option<Vec<[f64; 2]>> {
    let ring_of = |far: f64| -> Vec<[f64; 2]> {
        (0..STROLL_TURNS)
            .map(|turn| {
                let angle = turn as f64 / STROLL_TURNS as f64 * std::f64::consts::TAU;
                [place[0] + angle.cos() * far, place[1] + angle.sin() * far]
            })
            .collect()
    };
    let stretches = STROLL_AROUND.iter().flat_map(|far| {
        let ring = ring_of(*far);
        (0..ring.len()).map(move |start| {
            let turned: Vec<_> = (0..ring.len())
                .map(|at| ring[(at + start) % ring.len()])
                .collect();
            let mut stretch = vec![turned[0]];
            for pair in turned.windows(2) {
                if !land.free(pair[0]) || !walks(land, pair[0], pair[1]) {
                    break;
                }
                stretch.push(pair[1]);
            }
            stretch
        })
    });
    stretches
        .max_by_key(Vec::len)
        .filter(|stretch| stretch.len() >= 3)
}

/// Someone out for a walk, holding balloons.
fn strollers(land: &impl Land, places: &[[f64; 2]]) -> Vec<LifeScene> {
    let spots = places
        .iter()
        .filter_map(|place| stroll_around(land, *place));
    spots
        .take(STROLLERS)
        .enumerate()
        .map(|(index, stretch)| {
            let last = stretch.len() - 1;
            let turns = stretch.iter().enumerate().map(|(at, spot)| Turn {
                at: *spot,
                above: 0.0,
                rest: if at == 0 || at == last { 60 } else { 0 },
            });
            let mut balloons = Actor::new(Model::Balloons, 15.0);
            balloons.up = PERSON * 0.55;
            balloons.behind = 2.0;
            // Beside them, or from above they would hide who holds them.
            balloons.offset = [5.0, 3.0];
            balloons.bob = 0.8;
            balloons.least = 2.6;
            LifeScene {
                name: format!("Ballons {}", index + 1),
                way: Way::Route {
                    turns: turns.collect(),
                    ends: Ends::BackAndForth,
                    speed: STROLL_SPEED,
                },
                cast: vec![Actor::new(Model::Stroller, PERSON), balloons],
                marks: Vec::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::life::testing::Plain;
    use crate::plugin::{built_scene, marker_places};
    use crate::regions::{RegionState, Regions};
    use crate::scene::{DETAIL, MIDDLE, OVERVIEW};
    use voidmc::DefaultWorldGenerator;

    fn scenes() -> (Vec<LifeScene>, Vec<crate::scene::Scene>) {
        let generator = DefaultWorldGenerator::default();
        let regions = Regions::demo();
        let places: Vec<[f64; 2]> = marker_places(&generator)
            .iter()
            .map(|[x, z]| [f64::from(*x), f64::from(*z)])
            .collect();
        let detail = built_scene(&generator, DETAIL, &regions);
        let open = |at: [f64; 2]| regions.at(at[0], at[1]).state == RegionState::Discovered;
        let found = scenes_of(&detail, &places, &open);
        let models =
            [DETAIL, MIDDLE, OVERVIEW].map(|scale| built_scene(&generator, scale, &regions));
        (found, models.into_iter().collect())
    }

    #[test]
    fn the_map_has_every_kind_of_scene() {
        let (found, _) = scenes();
        let has = |word: &str| found.iter().filter(|s| s.name.starts_with(word)).count();
        assert_eq!(has("Mouettes"), FLOCKS);
        assert_eq!(has("Train"), 1);
        assert_eq!(has("Pêcheur"), ANGLERS);
        assert_eq!(has("Ballons"), STROLLERS);
    }

    #[test]
    fn every_actor_is_on_every_model_all_along_its_way() {
        let (found, models) = scenes();
        for scene in &found {
            let play = Playback::new(scene);
            for model in &models {
                for actor in 0..play.actors() {
                    for tick in (0..20_000).step_by(37) {
                        let at = play.place(actor, tick, model);
                        assert!(at.is_some(), "{} {actor} {tick}", scene.name);
                    }
                }
            }
        }
    }

    /// Someone fishes from planks on every model, wherever each draws its shore.
    #[test]
    fn a_pier_reaches_the_water_on_every_model() {
        let (found, mut models) = scenes();
        for model in &mut models {
            model.lay_all(&found);
            for scene in &found {
                let Way::Still { at, .. } = scene.way else {
                    continue;
                };
                let (x, z) = model.cell_of(at[0] as i32, at[1] as i32).unwrap();
                assert_eq!(model.stand_near(x, z), Some((x, z)), "{}", scene.name);
            }
        }
    }

    #[test]
    fn a_line_goes_round_a_hole_in_the_land() {
        let land = Plain {
            shore: 1000.0,
            per_cell: 3.0,
        };
        let open = |at: [f64; 2]| !(at[0].abs() < 30.0 && at[1].abs() < 200.0);
        let cells = line(&land, &open, &[], [-10, 0], [10, 0]).unwrap();
        assert!(cells.iter().all(|cell| open(middle_of(*cell))));
    }
}
