//! The scenes of a map made from the world: where there is room for them, read off the
//! [`Land`]. All that comes out is [`LifeScene`]s, as a file could hold.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use super::{Actor, Ends, Land, LifeScene, Mark, Model, Playback, Track, Turn, Way, yaw_towards};
use crate::noise::scatter;

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
/// A leader and two lines behind it, on either side.
const GULLS: usize = 7;
const WINGSPAN: f64 = 11.0;
/// So that a gull still shows on the far model, in its blocks.
const LEAST_GULL: f64 = 1.7;
/// From one gull of a line to the next, in world blocks.
const GULL_BEHIND: f64 = 13.0;
const GULL_BESIDE: f64 = 12.0;
const GULL_SPEED: f64 = 1.0;
/// How high they fly over the land, in world blocks.
const GULL_HEIGHT: f64 = 40.0;
/// How much higher each flock flies than the last, in world blocks.
const GULL_LAYER: f64 = 7.0;
/// How far they lean into a turn. At the tightest, they stand on a wing.
const GULL_BANK: f64 = 34.0;
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
/// How far either side a line evens out the steps of its grid, in cells of it, and how
/// many times over.
const LINE_EVENING: usize = 7;
const LINE_EVENINGS: usize = 3;
/// The tightest turn of a line, as a radius in world blocks: a carriage has to fit in it.
const LINE_TURN: f64 = 30.0;
/// How far apart along a line a turn is measured, in cells of the grid.
const LINE_TURN_REACH: usize = 3;
/// How many times a line opens its tight turns.
const LINE_OPENINGS: usize = 400;
/// A line has a turn every so many cells of the grid, and a curve runs through them.
const LINE_TURN_EVERY: usize = 5;
/// How far from the rim a line stays, in world blocks.
const RIM_CLEARANCE: f64 = 24.0;
/// Tries for a round, each with a shape of its own.
const ROUND_SEEDS: u32 = 60;
/// How many turns a round has: few, for it to go in long curves.
const ROUND_TURNS: usize = 10;
/// How wide a flock's round is, and a balloon's, around its middle, in world blocks.
const FLOCK_RADII: [f64; 3] = [130.0, 170.0, 215.0];
const BALLOON_RADII: [f64; 3] = [200.0, 160.0, 120.0];
/// The tightest turn of a round, as a radius in world blocks.
const FLOCK_TURN: f64 = 50.0;
const BALLOON_TURN: f64 = 45.0;
/// How much room around its way a round keeps over the land, in world blocks.
const ROUND_ROOM: f64 = 18.0;
/// How much closer to the middle of the map than a place a round is centred.
const ROUND_INLAND: f64 = 0.55;
/// How often a round is looked at, in world blocks.
const ROUND_CHECK: f64 = 12.0;

const BALLOONS: usize = 2;
const BALLOON: f64 = 42.0;
const BALLOON_SHADOW: f64 = 34.0;
const BALLOON_SPEED: f64 = 0.22;
const BALLOON_HEIGHT: f64 = 72.0;
/// How far from under a balloon its shadow falls, east and south, in world blocks.
const SHADOW_FALLS: [f64; 2] = [26.0, 30.0];

/// Which line to draw, while there is a choice: `VOID_MAP_TRACK` is `sleepers`, `pale` or
/// `embankment`.
fn track_of_this_run() -> Track {
    match std::env::var("VOID_MAP_TRACK").as_deref() {
        Ok("pale") => Track::PaleBallast,
        Ok("embankment") => Track::Embankment,
        _ => Track::Sleepers,
    }
}

/// The scenes of a map. `open` says which places of the world are there to be seen, and
/// `places` are its waypoints.
pub fn scenes_of(
    land: &impl Land,
    places: &[[f64; 2]],
    open: &impl Fn([f64; 2]) -> bool,
) -> Vec<LifeScene> {
    let places: Vec<[f64; 2]> = places.iter().copied().filter(|at| open(*at)).collect();
    let mut scenes = Vec::new();
    scenes.extend(flocks(land, &places, open));
    scenes.extend(train(land, &places, open));
    scenes.extend(anglers(land, &places, open));
    scenes.extend(balloons(land, &places, open));
    scenes
}

/// What a round is scored on, as it is looked at along its way.
#[derive(Debug, Clone, Copy, Default)]
struct Survey {
    /// How many times it goes from land to water or back.
    shores: u32,
    /// The shares of it that are over water, and over land that is not open.
    water: f64,
    closed: f64,
}

/// A closed curve around a middle, wide of `radius`: a ring bent by two slow waves.
fn round_of(middle: [f64; 2], radius: f64, seed: u32, above: f64) -> Vec<Turn> {
    let [first, second, lobes, turn_phase] = scatter(seed, 3, 7);
    let waves = 2.0 + lobes.round();
    (0..ROUND_TURNS)
        .map(|turn| {
            let angle = turn as f64 / ROUND_TURNS as f64 * std::f64::consts::TAU;
            let bend = 0.24 * (angle * waves + first * 6.3).sin()
                + 0.07 * (angle * (waves + 1.0) + second * 6.3).sin();
            let far = radius * (1.0 + bend);
            let angle = angle + turn_phase;
            Turn {
                at: [middle[0] + angle.cos() * far, middle[1] + angle.sin() * far],
                above,
                rest: 0,
            }
        })
        .collect()
}

/// What a round goes over, or `None` if it leaves the land or turns too tight.
fn survey(
    land: &impl Land,
    open: &impl Fn([f64; 2]) -> bool,
    turns: &[Turn],
    tightest: f64,
) -> Option<Survey> {
    let scene = LifeScene {
        name: String::new(),
        way: Way::Route {
            turns: turns.to_vec(),
            ends: Ends::Round,
            speed: 1.0,
        },
        cast: Vec::new(),
        marks: Vec::new(),
    };
    let play = Playback::new(&scene);
    let looks = (play.length / ROUND_CHECK) as u32;
    let mut found = Survey::default();
    let mut was_water = None;
    for look in 0..looks {
        let distance = f64::from(look) * ROUND_CHECK;
        let at = play.along(distance);
        let room = |[dx, dz]: &[i32; 2]| {
            let side = [f64::from(*dx), f64::from(*dz)].map(|side| side * ROUND_ROOM);
            land.ground([at[0] + side[0], at[1] + side[1]]).is_some()
        };
        let before = play.heading(distance - ROUND_CHECK, ROUND_CHECK);
        let after = play.heading(distance + ROUND_CHECK, ROUND_CHECK);
        let turned = f64::from((after - before + 540.0).rem_euclid(360.0) - 180.0);
        let radius = ROUND_CHECK * 2.0 / turned.to_radians().abs().max(1e-6);
        if !AROUND.iter().all(room) || radius < tightest {
            return None;
        }
        let water = land.water(at);
        found.shores += u32::from(was_water.is_some_and(|was| was != water));
        was_water = Some(water);
        found.water += f64::from(u8::from(water)) / f64::from(looks);
        found.closed += f64::from(u8::from(!open(at))) / f64::from(looks);
    }
    Some(found)
}

/// The best round about a middle, or `None`.
fn best_round(
    land: &impl Land,
    open: &impl Fn([f64; 2]) -> bool,
    (middle, radii, above, tightest): ([f64; 2], &[f64], f64, f64),
    seed: u32,
    score: impl Fn(Survey) -> f64,
) -> Option<Vec<Turn>> {
    radii
        .iter()
        .flat_map(|radius| {
            (0..ROUND_SEEDS).map(move |at| round_of(middle, *radius, seed + at, above))
        })
        .filter_map(|turns| {
            let found = survey(land, open, &turns, tightest)?;
            Some((score(found), turns))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, turns)| turns)
}

/// As many places as asked for, as far from each other as they come.
fn spread_out(places: &[[f64; 2]], count: usize) -> Vec<[f64; 2]> {
    let away = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let mut chosen: Vec<[f64; 2]> = places.iter().copied().take(1).collect();
    while chosen.len() < count.min(places.len()) {
        let nearest = |at: &[f64; 2]| {
            let each = chosen.iter().map(|other| away(*at, *other));
            each.fold(f64::MAX, f64::min)
        };
        let next = places
            .iter()
            .max_by(|a, b| nearest(a).total_cmp(&nearest(b)));
        chosen.extend(next);
    }
    chosen
}

/// A skein, which holds its shape through the turns of its way. Each round goes along
/// the shores it can find, over the lakes and back over the land.
fn flocks(
    land: &impl Land,
    places: &[[f64; 2]],
    open: &impl Fn([f64; 2]) -> bool,
) -> Vec<LifeScene> {
    let skein = || {
        let beats = [Model::Gull, Model::GullOffBeat, Model::GullLastBeat];
        (0..GULLS).map(move |gull| {
            let rank = (gull as f64 / 2.0).ceil();
            let side = if gull % 2 == 0 { -1.0 } else { 1.0 };
            let mut actor = Actor::new(beats[gull % beats.len()], WINGSPAN);
            actor.least = LEAST_GULL;
            actor.behind = rank * GULL_BEHIND;
            actor.beside = rank * GULL_BESIDE * side;
            actor.drift = 1.2;
            actor.bob = 1.0;
            actor.bank = GULL_BANK;
            actor
        })
    };
    let score = |found: Survey| {
        f64::from(found.shores) - (found.water - 0.5).abs() * 8.0 - found.closed * 12.0
    };
    spread_out(places, FLOCKS)
        .into_iter()
        .enumerate()
        .filter_map(|(index, place)| {
            let middle = place.map(|value| value * ROUND_INLAND);
            // Over and past each other at heights of their own.
            let height = GULL_HEIGHT + index as f64 * GULL_LAYER;
            let wanted = (middle, &FLOCK_RADII[..], height, FLOCK_TURN);
            let seed = 500 + index as u32 * ROUND_SEEDS;
            let mut turns = best_round(land, open, wanted, seed, score)?;
            // And they do not all set off from the same side.
            turns.rotate_left(index * ROUND_TURNS / FLOCKS);
            Some(LifeScene {
                name: format!("Mouettes {}", index + 1),
                way: Way::Route {
                    turns,
                    ends: Ends::Round,
                    speed: GULL_SPEED,
                },
                cast: skein().collect(),
                marks: Vec::new(),
            })
        })
        .collect()
}

/// A balloon drifting slowly over the land, with its shadow on the ground under it.
fn balloons(
    land: &impl Land,
    places: &[[f64; 2]],
    open: &impl Fn([f64; 2]) -> bool,
) -> Vec<LifeScene> {
    let score = |found: Survey| -found.closed * 12.0 - found.water;
    let models = [Model::Balloon, Model::BalloonBlue];
    spread_out(places, BALLOONS)
        .into_iter()
        .zip(models)
        .enumerate()
        .filter_map(|(index, (place, model))| {
            let middle = place.map(|value| value * ROUND_INLAND);
            let wanted = (middle, &BALLOON_RADII[..], BALLOON_HEIGHT, BALLOON_TURN);
            let seed = 900 + index as u32 * ROUND_SEEDS;
            let turns = best_round(land, open, wanted, seed, score)?;
            let mut balloon = Actor::new(model, BALLOON);
            balloon.bob = 1.2;
            let mut shadow = Actor::new(Model::BalloonShadow, BALLOON_SHADOW);
            shadow.on_the_ground = true;
            // The sun is in the north-west: seen from over the balloon, it does not hide it.
            shadow.offset = SHADOW_FALLS;
            Some(LifeScene {
                name: format!("Montgolfière {}", index + 1),
                way: Way::Route {
                    turns,
                    ends: Ends::Round,
                    speed: BALLOON_SPEED,
                },
                cast: vec![balloon, shadow],
                marks: Vec::new(),
            })
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

/// Evens out a closed line of the grid: what is left of its steps goes into the curve
/// that runs through what comes out.
fn evened_out(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let count = points.len();
    let width = (LINE_EVENING * 2 + 1) as f64;
    for _ in 0..LINE_EVENINGS {
        let source = points.clone();
        for (at, point) in points.iter_mut().enumerate() {
            let around = (0..=LINE_EVENING * 2)
                .map(|side| source[(at + side + count - LINE_EVENING) % count]);
            let sum = around.fold([0.0, 0.0], |sum, at| [sum[0] + at[0], sum[1] + at[1]]);
            *point = sum.map(|value| value / width);
        }
    }
    for _ in 0..LINE_OPENINGS {
        let source = points.clone();
        let mut all_open = true;
        for (at, point) in points.iter_mut().enumerate() {
            let [before, here, after] = [count - LINE_TURN_REACH, 0, LINE_TURN_REACH]
                .map(|shift| source[(at + shift) % count]);
            let middle = [(before[0] + after[0]) / 2.0, (before[1] + after[1]) / 2.0];
            let chord = (after[0] - before[0]).hypot(after[1] - before[1]) / 2.0;
            let sag = (here[0] - middle[0]).hypot(here[1] - middle[1]);
            // How far a turn of that radius would stray from this chord.
            let allowed = LINE_TURN - (LINE_TURN.powi(2) - chord.min(LINE_TURN).powi(2)).sqrt();
            if sag > allowed {
                all_open = false;
                *point = [
                    here[0] + (middle[0] - here[0]) * 0.5,
                    here[1] + (middle[1] - here[1]) * 0.5,
                ];
            }
        }
        if all_open {
            break;
        }
    }
    points
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
    let even = evened_out(way.iter().map(|cell| middle_of(*cell)).collect());
    let round: Vec<[f64; 2]> = even.iter().copied().step_by(LINE_TURN_EVERY).collect();
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
            ends: Ends::Round,
            speed: TRAIN_SPEED,
        },
        cast,
        marks: vec![Mark::Rails {
            width: RAIL_WIDTH,
            track: track_of_this_run(),
        }],
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
        assert_eq!(has("Montgolfière"), BALLOONS);
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

    /// Pieces of a model that pass a test, by flood: which piece each cell is on.
    fn pieces(
        model: &crate::scene::Scene,
        fits: impl Fn(i32, i32) -> bool,
    ) -> BTreeMap<(i32, i32), u32> {
        let radius = model.scale().radius;
        let mut found = BTreeMap::new();
        let mut next = 0;
        for x in -radius..=radius {
            for z in -radius..=radius {
                if found.contains_key(&(x, z)) || !fits(x, z) {
                    continue;
                }
                next += 1;
                let mut open = vec![(x, z)];
                while let Some((x, z)) = open.pop() {
                    if fits(x, z) && !found.contains_key(&(x, z)) {
                        found.insert((x, z), next);
                        open.extend([(x + 1, z), (x - 1, z), (x, z + 1), (x, z - 1)]);
                    }
                }
            }
        }
        found
    }

    /// How many cells the largest piece has.
    fn largest(pieces: &BTreeMap<(i32, i32), u32>) -> usize {
        let mut sizes = BTreeMap::new();
        for piece in pieces.values() {
            *sizes.entry(*piece).or_insert(0) += 1;
        }
        sizes.into_values().max().unwrap_or(0)
    }

    /// A viaduct stands over a lake: what sails there still goes from one side to the other.
    #[test]
    fn a_line_cuts_no_lake_in_two() {
        let (found, mut models) = scenes();
        let model = &mut models[0];
        let before = pieces(model, |x, z| model.sailable(x, z));
        let (count, most) = (
            before
                .values()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            largest(&before),
        );
        model.lay_all(&found);
        let after = pieces(model, |x, z| model.sailable(x, z));
        let pieces_now = after
            .values()
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        assert_eq!(pieces_now, count);
        assert!(
            largest(&after) * 100 >= most * 97,
            "{} {most}",
            largest(&after)
        );
    }

    /// The piece of ground closest to each place of the map.
    fn pieces_of_places(
        model: &crate::scene::Scene,
        pieces: &BTreeMap<(i32, i32), u32>,
    ) -> Vec<Option<u32>> {
        let places = marker_places(&DefaultWorldGenerator::default());
        let near = |[x, z]: [i32; 2]| {
            let (x, z) = model.cell_of(x, z)?;
            let around = (-12..=12).flat_map(|dx| (-12..=12).map(move |dz| (dx, dz)));
            let closest = around
                .filter(|(dx, dz)| pieces.contains_key(&(x + dx, z + dz)))
                .min_by_key(|(dx, dz)| dx * dx + dz * dz)?;
            pieces.get(&(x + closest.0, z + closest.1)).copied()
        };
        places.into_iter().map(near).collect()
    }

    /// Walkers cross a line at a path and go nowhere along it, and a loop of rails shuts
    /// no one in: whoever could go from one place to another still can.
    #[test]
    fn a_line_is_no_road_and_no_wall() {
        let (found, mut models) = scenes();
        let model = &mut models[0];
        let before = pieces(model, |x, z| model.walkable(x, z));
        let linked_before = pieces_of_places(model, &before);
        let train = found.iter().find(|scene| scene.name == "Train").unwrap();
        let line = train.laid(&*model);
        model.lay_all(&found);
        let after = pieces(model, |x, z| model.walkable(x, z));
        let linked_after = pieces_of_places(model, &after);
        for a in 0..linked_before.len() {
            for b in 0..a {
                let was = linked_before[a].is_some() && linked_before[a] == linked_before[b];
                let is = linked_after[a].is_some() && linked_after[a] == linked_after[b];
                assert!(!was || is, "{a} {b}");
            }
        }
        let crossings = line
            .iter()
            .filter(|lay| model.walkable(lay.at.0, lay.at.1))
            .count();
        assert!(crossings > 0);
        assert!(crossings * 10 < line.len(), "{crossings} {}", line.len());
    }

    /// Who fishes stands on planks nobody walks to, and no walker goes out over the water.
    #[test]
    fn nothing_built_over_water_is_ground() {
        let (found, mut models) = scenes();
        let model = &mut models[0];
        let bare = found
            .iter()
            .flat_map(|scene| scene.laid(&*model))
            .collect::<Vec<_>>();
        let wet: Vec<_> = bare
            .iter()
            .filter(|lay| model.top(lay.at.0, lay.at.1) == Some(crate::scene::SEA_Y))
            .filter(|lay| !model.walkable(lay.at.0, lay.at.1))
            .map(|lay| lay.at)
            .collect();
        assert!(!wet.is_empty());
        model.lay_all(&found);
        assert!(wet.iter().all(|(x, z)| !model.walkable(*x, *z)));
    }
}
