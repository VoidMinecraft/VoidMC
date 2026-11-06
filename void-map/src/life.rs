//! Scenes of life on the map: birds, a train, someone fishing, someone with balloons.
//!
//! A [`LifeScene`] is data: who is in it, the way or the zone they keep to, and what it
//! leaves on the ground. It serialises, so that a file can hold it as well as code can
//! make it. [`Playback`] is the only way to play one, and asks nothing of the map but
//! [`Land`]: this module reads no generator, and no model of the map either. A map made
//! from the world gets its scenes from [`habitat`], and a map made by hand would read them
//! from a file: from there on, they go down the same road.
//!
//! Where anyone is at a tick is a function of that tick. There is no state to keep up, so
//! a scene nobody looks at costs nothing, and looking further ahead is as cheap as looking
//! at now, which is what lets a client glide from one place to the next: see [`show`].

use serde::{Deserialize, Serialize};

use crate::noise::scatter;

pub mod habitat;
pub mod models;
pub mod show;

/// Ground a walking actor keeps level over, in world blocks ahead and behind: a terrace
/// comes as a ramp.
const LEVEL_REACH: f64 = 6.0;
/// How much of the way a sample of the height stands for, for what flies.
const LEVEL_SAMPLES: [f64; 3] = [-1.0, 0.0, 1.0];
/// The least a flying actor stays over what is under it, in blocks of the model.
const LEAST_CLEARANCE: f64 = 2.0;
/// How far apart marks are stamped along their line, in cells.
const STAMP_STEP: f64 = 0.4;
/// A sleeper every so far along the rails, in world blocks, and as long as the gap.
const SLEEPER_SPACING: f64 = 6.0;

/// What a scene needs of a map, whichever way that map was made.
pub trait Land {
    /// Where a place of the world is on the model, in its blocks.
    fn on_model(&self, at: [f64; 2]) -> [f64; 2];
    /// How long a length of the world is on the model.
    fn length(&self, world: f64) -> f64;
    /// What an actor stands on, or swims on, in heights of the model. `None` off the map.
    fn ground(&self, at: [f64; 2]) -> Option<f64>;
    /// Whether this place is water.
    fn water(&self, at: [f64; 2]) -> bool;
    /// Whether someone can stand here: dry, and with nothing rising out of the ground.
    fn free(&self, at: [f64; 2]) -> bool;
}

/// Something alive on the map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifeScene {
    pub name: String,
    pub way: Way,
    pub cast: Vec<Actor>,
    /// What it leaves on the map, for the model to draw.
    #[serde(default)]
    pub marks: Vec<Mark>,
}

/// Where a scene goes, all in world blocks and ticks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Way {
    /// It stays on one spot, facing this way, in Minecraft degrees.
    Still { at: [f64; 2], facing: f32 },
    /// It goes from turn to turn.
    Route {
        turns: Vec<Turn>,
        ends: Ends,
        /// In blocks per tick.
        speed: f64,
    },
    /// It roams over a zone, from places drawn from `seed`.
    Zone {
        middle: [f64; 2],
        radius: f64,
        hops: u32,
        /// Over the ground, between two places.
        height: f64,
        /// Every so many hops it comes down and rests, or never with 0.
        perch_every: u32,
        rest: u32,
        speed: f64,
        seed: u32,
    },
}

/// A place on a route.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub at: [f64; 2],
    /// Over the ground, in world blocks.
    #[serde(default)]
    pub above: f64,
    /// Ticks it stops here.
    #[serde(default)]
    pub rest: u32,
}

/// What happens at the end of a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ends {
    /// The last turn leads back to the first.
    Loop,
    /// It turns round, and comes back the same way.
    BackAndForth,
}

/// One entity of a scene.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub model: Model,
    /// What it turns into while the way rests.
    #[serde(default)]
    pub at_rest: Option<Model>,
    /// The height of its model, in world blocks, so that it follows the zoom.
    pub size: f64,
    /// The least it can be, in blocks of the model: what is true to scale on the close
    /// model can be under a pixel on the far one.
    #[serde(default)]
    pub least: f64,
    /// How far back along the way, in world blocks: what follows the same rails.
    #[serde(default)]
    pub behind: f64,
    /// Where it stands from the way, east and south, in world blocks.
    #[serde(default)]
    pub offset: [f64; 2],
    /// Over the way, in world blocks.
    #[serde(default)]
    pub up: f64,
    /// How far it strays from there and back, while it moves, in world blocks.
    #[serde(default)]
    pub drift: f64,
    /// How far it rises and falls, in world blocks.
    #[serde(default)]
    pub bob: f64,
}

impl Actor {
    pub fn new(model: Model, size: f64) -> Self {
        Self {
            model,
            at_rest: None,
            size,
            least: 0.0,
            behind: 0.0,
            offset: [0.0; 2],
            up: 0.0,
            drift: 0.0,
            bob: 0.0,
        }
    }
}

/// A model of the pack, which [`models`] draws. Facing north, standing on its lowest face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Model {
    /// A gull in flight. The three beat their wings out of step: a client plays the
    /// frames of a texture by itself, all the copies of it together.
    Gull,
    GullOffBeat,
    GullLastBeat,
    GullPerched,
    Locomotive,
    Carriage,
    Wagon,
    Angler,
    /// A float on the water, with its rings.
    Float,
    Stroller,
    Balloons,
}

/// What a scene leaves on the map.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Mark {
    /// Along the scene's own way, this wide in world blocks.
    Rails { width: f64 },
    /// Planks out over the water, this wide in world blocks.
    Pier {
        from: [f64; 2],
        to: [f64; 2],
        width: f64,
    },
}

/// What a cell of a model turns into under a [`Mark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Laid {
    Ballast,
    Sleeper,
    Planks,
}

impl LifeScene {
    /// What its marks turn into on a model: the cell and what lies on it. A later entry
    /// takes over from an earlier one.
    pub fn laid(&self, land: &impl Land) -> Vec<((i32, i32), Laid)> {
        let mut cells = Vec::new();
        for mark in &self.marks {
            match mark {
                Mark::Rails { width } => {
                    let points = Playback::new(self).points;
                    for pair in points.windows(2) {
                        let stamp = |along: f64| {
                            let sleeper = (along / SLEEPER_SPACING).rem_euclid(2.0) < 1.0;
                            if sleeper {
                                Laid::Sleeper
                            } else {
                                Laid::Ballast
                            }
                        };
                        stamp_line(
                            land,
                            pair[0].at,
                            pair[1].at,
                            *width,
                            pair[0].along,
                            &stamp,
                            &mut cells,
                        );
                    }
                }
                Mark::Pier { from, to, width } => {
                    stamp_line(land, *from, *to, *width, 0.0, &|_| Laid::Planks, &mut cells);
                }
            }
        }
        cells
    }
}

/// Stamps a band along a line.
fn stamp_line(
    land: &impl Land,
    from: [f64; 2],
    to: [f64; 2],
    width: f64,
    start: f64,
    laid: &impl Fn(f64) -> Laid,
    cells: &mut Vec<((i32, i32), Laid)>,
) {
    let length = (to[0] - from[0]).hypot(to[1] - from[1]);
    let on_model = land.length(length);
    let steps = (on_model / STAMP_STEP).ceil().max(1.0) as u32;
    let half = (land.length(width) / 2.0).max(0.75);
    let reach = half.ceil() as i32;
    for step in 0..=steps {
        let share = f64::from(step) / f64::from(steps);
        let [x, z] = land.on_model(between(from, to, share));
        let (cell_x, cell_z) = (x.floor() as i32, z.floor() as i32);
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let (mid_x, mid_z) = (f64::from(cell_x + dx) + 0.5, f64::from(cell_z + dz) + 0.5);
                if (mid_x - x).hypot(mid_z - z) <= half {
                    cells.push(((cell_x + dx, cell_z + dz), laid(start + length * share)));
                }
            }
        }
    }
}

fn between(from: [f64; 2], to: [f64; 2], share: f64) -> [f64; 2] {
    [
        from[0] + (to[0] - from[0]) * share,
        from[1] + (to[1] - from[1]) * share,
    ]
}

/// Faces a way, in Minecraft degrees.
pub(crate) fn yaw_towards(dx: f64, dz: f64) -> f32 {
    (-dx).atan2(dz).to_degrees() as f32
}

/// A place of a way, and how far along it is.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    at: [f64; 2],
    above: f64,
    along: f64,
}

/// A stretch of time of a way, at one speed. It rests at 0.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Beat {
    start: u64,
    from: f64,
    speed: f64,
}

/// One actor at one tick, on a model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// In blocks of the model, from its centre.
    pub at: [f64; 3],
    pub yaw: f32,
    pub model: Model,
}

/// A scene ready to play: what comes down to a timetable, worked out once.
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    cast: Vec<Actor>,
    points: Vec<Point>,
    beats: Vec<Beat>,
    /// How long the way is, in world blocks, and in ticks.
    length: f64,
    period: u64,
    facing: f32,
}

/// The turns of a zone, as the loop it comes down to.
fn zone_turns(
    middle: [f64; 2],
    radius: f64,
    hops: u32,
    height: f64,
    perch_every: u32,
    rest: u32,
    seed: u32,
) -> Vec<Turn> {
    (0..hops)
        .map(|hop| {
            let [angle, far, ..] = scatter(seed, hop as i32, 0);
            let angle = angle * std::f64::consts::TAU;
            let far = far.sqrt() * radius;
            let perch = perch_every != 0 && hop % perch_every == perch_every - 1;
            Turn {
                at: [middle[0] + angle.cos() * far, middle[1] + angle.sin() * far],
                above: if perch { 0.0 } else { height },
                rest: if perch { rest } else { 0 },
            }
        })
        .collect()
}

impl Playback {
    pub fn new(scene: &LifeScene) -> Self {
        let (turns, ends, speed, facing) = match &scene.way {
            Way::Still { at, facing } => {
                let turn = Turn {
                    at: *at,
                    above: 0.0,
                    rest: 0,
                };
                (vec![turn], Ends::Loop, 0.0, *facing)
            }
            Way::Route { turns, ends, speed } => (turns.clone(), *ends, *speed, 0.0),
            Way::Zone {
                middle,
                radius,
                hops,
                height,
                perch_every,
                rest,
                speed,
                seed,
            } => {
                let turns =
                    zone_turns(*middle, *radius, *hops, *height, *perch_every, *rest, *seed);
                (turns, Ends::Loop, *speed, 0.0)
            }
        };
        // Both ways of an end come down to a loop: a way there and back is one that passes
        // twice over the same ground.
        let mut round = turns.clone();
        match ends {
            Ends::Loop => round.extend(turns.first()),
            Ends::BackAndForth => round.extend(turns.iter().rev().skip(1)),
        }
        let (mut points, mut beats) = (Vec::with_capacity(round.len()), Vec::new());
        let (mut along, mut tick) = (0.0, 0);
        for (index, turn) in round.iter().enumerate() {
            points.push(Point {
                at: turn.at,
                above: turn.above,
                along,
            });
            let Some(next) = round.get(index + 1) else {
                break;
            };
            if turn.rest > 0 {
                beats.push(Beat {
                    start: tick,
                    from: along,
                    speed: 0.0,
                });
                tick += u64::from(turn.rest);
            }
            let leg = (next.at[0] - turn.at[0]).hypot(next.at[1] - turn.at[1]);
            if leg > 0.0 && speed > 0.0 {
                let ticks = (leg / speed).ceil().max(1.0);
                beats.push(Beat {
                    start: tick,
                    from: along,
                    speed: leg / ticks,
                });
                tick += ticks as u64;
                along += leg;
            }
        }
        Self {
            cast: scene.cast.clone(),
            points,
            beats,
            length: along,
            period: tick.max(1),
            facing,
        }
    }

    pub fn actors(&self) -> usize {
        self.cast.len()
    }

    pub fn actor(&self, index: usize) -> &Actor {
        &self.cast[index]
    }

    /// How far along the way the scene is, and whether it rests.
    fn travelled(&self, tick: u64) -> (f64, bool) {
        let within = tick % self.period;
        let beat = self.beats.partition_point(|beat| beat.start <= within);
        match beat.checked_sub(1).and_then(|at| self.beats.get(at)) {
            Some(beat) => (
                beat.from + beat.speed * (within - beat.start) as f64,
                beat.speed == 0.0,
            ),
            None => (0.0, true),
        }
    }

    /// The place at a distance along the way, around it, its height over the ground and
    /// the way it goes there.
    fn along(&self, distance: f64) -> ([f64; 2], f64, f32) {
        let first = self.points[0];
        if self.length <= 0.0 {
            return (first.at, first.above, self.facing);
        }
        let distance = distance.rem_euclid(self.length);
        let next = self.points.partition_point(|point| point.along <= distance);
        let next = next.clamp(1, self.points.len() - 1);
        let (from, to) = (self.points[next - 1], self.points[next]);
        let share = (distance - from.along) / (to.along - from.along).max(f64::MIN_POSITIVE);
        (
            between(from.at, to.at, share),
            from.above + (to.above - from.above) * share,
            yaw_towards(to.at[0] - from.at[0], to.at[1] - from.at[1]),
        )
    }

    /// An actor at a tick, or `None` if that is off the map.
    pub fn place(&self, actor: usize, tick: u64, land: &impl Land) -> Option<Placed> {
        let cast = self.cast.get(actor)?;
        let (travelled, resting) = self.travelled(tick);
        let distance = travelled - cast.behind;
        let (way, above, yaw) = self.along(distance);
        let time = tick as f64;
        // Each actor keeps time of its own, from its place in the cast.
        let phase = actor as f64 * 2.4;
        let drift = if resting { 0.0 } else { cast.drift };
        let spot = [
            way[0] + cast.offset[0] + drift * (time / 41.0 + phase).sin(),
            way[1] + cast.offset[1] + drift * (time / 53.0 + phase * 1.7).cos(),
        ];
        let reach = LEVEL_REACH + above;
        let (sum, count) = LEVEL_SAMPLES
            .iter()
            .filter_map(|side| land.ground(self.along(distance + side * reach).0))
            .fold((0.0, 0.0), |(sum, count), ground| {
                (sum + ground, count + 1.0)
            });
        let level = (count > 0.0).then(|| sum / count);
        // A coarser model has its rim a little elsewhere: the way counts, not this spot.
        let here = land.ground(spot).or(level)?;
        let level = level.unwrap_or(here);
        let over = land.length(above);
        let y = if resting {
            here + over
        } else if above > 0.0 {
            (level + over).max(here + over.min(LEAST_CLEARANCE))
        } else {
            level
        };
        let bob = cast.bob * (time / 19.0 + phase).sin();
        let [x, z] = land.on_model(spot);
        Some(Placed {
            at: [x, y + land.length(cast.up + bob), z],
            yaw,
            model: cast.at_rest.filter(|_| resting).unwrap_or(cast.model),
        })
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Open land, with the sea east of `shore` and each block the size of a world block.
    pub(crate) struct Plain {
        pub shore: f64,
        pub per_cell: f64,
    }

    impl Land for Plain {
        fn on_model(&self, at: [f64; 2]) -> [f64; 2] {
            at.map(|value| value / self.per_cell)
        }

        fn length(&self, world: f64) -> f64 {
            world / self.per_cell
        }

        fn ground(&self, at: [f64; 2]) -> Option<f64> {
            (at[0].abs() < 1000.0 && at[1].abs() < 1000.0).then_some(65.0)
        }

        fn water(&self, at: [f64; 2]) -> bool {
            at[0] > self.shore
        }

        fn free(&self, at: [f64; 2]) -> bool {
            !self.water(at)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::Plain;
    use super::*;

    const LAND: Plain = Plain {
        shore: 500.0,
        per_cell: 1.0,
    };

    fn turn(x: f64, z: f64) -> Turn {
        Turn {
            at: [x, z],
            above: 0.0,
            rest: 0,
        }
    }

    fn walk(ends: Ends, turns: Vec<Turn>, cast: Vec<Actor>) -> Playback {
        Playback::new(&LifeScene {
            name: "test".into(),
            way: Way::Route {
                turns,
                ends,
                speed: 1.0,
            },
            cast,
            marks: Vec::new(),
        })
    }

    fn spot(play: &Playback, actor: usize, tick: u64) -> [f64; 3] {
        play.place(actor, tick, &LAND).unwrap().at
    }

    fn square(cast: Vec<Actor>) -> Playback {
        let turns = vec![
            turn(0.0, 0.0),
            turn(100.0, 0.0),
            turn(100.0, 100.0),
            turn(0.0, 100.0),
        ];
        walk(Ends::Loop, turns, cast)
    }

    #[test]
    fn a_loop_comes_back_to_where_it_started() {
        let play = square(vec![Actor::new(Model::Locomotive, 10.0)]);
        assert_eq!(play.period, 400);
        assert_eq!(spot(&play, 0, 0), spot(&play, 0, 400));
        assert_eq!(spot(&play, 0, 150), [100.0, 65.0, 50.0]);
    }

    #[test]
    fn a_walk_there_and_back_ends_where_it_started_and_faces_the_way_back() {
        let turns = vec![turn(0.0, 0.0), turn(100.0, 0.0)];
        let play = walk(
            Ends::BackAndForth,
            turns,
            vec![Actor::new(Model::Stroller, 2.0)],
        );
        assert_eq!(play.period, 200);
        assert_eq!(spot(&play, 0, 150), [50.0, 65.0, 0.0]);
        assert_eq!(play.place(0, 50, &LAND).unwrap().yaw, -90.0);
        assert_eq!(play.place(0, 150, &LAND).unwrap().yaw, 90.0);
        assert_eq!(spot(&play, 0, 200), spot(&play, 0, 0));
    }

    /// One position every so many ticks is only smooth if a way has no jumps in it.
    #[test]
    fn no_tick_jumps_ahead_of_the_speed() {
        let mut stop = turn(100.0, 0.0);
        stop.rest = 30;
        let turns = vec![turn(0.0, 0.0), stop, turn(100.0, 100.0)];
        let play = walk(Ends::Loop, turns, vec![Actor::new(Model::Stroller, 2.0)]);
        for tick in 0..play.period * 2 {
            let (a, b) = (spot(&play, 0, tick), spot(&play, 0, tick + 1));
            assert!((a[0] - b[0]).hypot(a[2] - b[2]) <= 1.01, "{tick}");
        }
    }

    #[test]
    fn a_way_rests_where_a_turn_says_and_shows_what_rests() {
        let mut stop = turn(100.0, 0.0);
        stop.rest = 30;
        let turns = vec![turn(0.0, 0.0), stop];
        let mut actor = Actor::new(Model::Gull, 2.0);
        actor.at_rest = Some(Model::GullPerched);
        let play = walk(Ends::BackAndForth, turns, vec![actor]);
        let (flying, resting) = (play.place(0, 50, &LAND), play.place(0, 110, &LAND));
        assert_eq!(flying.unwrap().model, Model::Gull);
        let resting = resting.unwrap();
        assert_eq!(resting.model, Model::GullPerched);
        assert_eq!(resting.at, [100.0, 65.0, 0.0]);
    }

    #[test]
    fn what_comes_behind_goes_over_the_same_ground_later() {
        let mut wagon = Actor::new(Model::Carriage, 10.0);
        wagon.behind = 30.0;
        let play = square(vec![Actor::new(Model::Locomotive, 10.0), wagon]);
        assert_eq!(spot(&play, 1, 130), spot(&play, 0, 100));
        // Round the corner of the way, and back past its start.
        assert_eq!(spot(&play, 1, 10), spot(&play, 0, 380));
    }

    #[test]
    fn a_zone_stays_in_its_zone_and_comes_down_to_rest() {
        let scene = LifeScene {
            name: "gulls".into(),
            way: Way::Zone {
                middle: [100.0, 50.0],
                radius: 60.0,
                hops: 12,
                height: 30.0,
                perch_every: 4,
                rest: 40,
                speed: 1.0,
                seed: 7,
            },
            cast: vec![Actor::new(Model::Gull, 3.0)],
            marks: Vec::new(),
        };
        let play = Playback::new(&scene);
        let mut heights = Vec::new();
        for tick in 0..play.period {
            let [x, y, z] = spot(&play, 0, tick);
            assert!((x - 100.0).hypot(z - 50.0) <= 60.1);
            heights.push(y);
        }
        let highest = heights.iter().copied().fold(f64::MIN, f64::max);
        let lowest = heights.iter().copied().fold(f64::MAX, f64::min);
        assert!(highest > 90.0, "{highest}");
        assert_eq!(lowest, 65.0);
    }

    #[test]
    fn a_still_scene_stays_put() {
        let scene = LifeScene {
            name: "fishing".into(),
            way: Way::Still {
                at: [10.0, 20.0],
                facing: 45.0,
            },
            cast: vec![Actor::new(Model::Angler, 2.0)],
            marks: Vec::new(),
        };
        let play = Playback::new(&scene);
        let first = play.place(0, 0, &LAND).unwrap();
        assert_eq!(first.yaw, 45.0);
        assert_eq!(first.at, [10.0, 65.0, 20.0]);
        assert_eq!(play.place(0, 1234, &LAND), Some(first));
    }

    #[test]
    fn a_scene_is_at_the_same_share_of_the_land_on_every_model() {
        let play = square(vec![Actor::new(Model::Locomotive, 10.0)]);
        let coarse = Plain {
            shore: 500.0,
            per_cell: 5.0,
        };
        let on_it = play.place(0, 150, &coarse).unwrap();
        assert_eq!(on_it.at, [20.0, 65.0, 10.0]);
    }

    /// The word of the task: a file and code are the same road. What comes back from a
    /// file plays exactly what went in, tick by tick.
    #[test]
    fn a_scene_comes_back_from_a_file_and_plays_the_same() {
        let mut bird = Actor::new(Model::Gull, 9.0);
        bird.at_rest = Some(Model::GullPerched);
        bird.offset = [3.0, -2.0];
        bird.drift = 2.5;
        let scene = LifeScene {
            name: "gulls".into(),
            way: Way::Zone {
                middle: [10.0, 20.0],
                radius: 80.0,
                hops: 8,
                height: 24.0,
                perch_every: 3,
                rest: 100,
                speed: 1.1,
                seed: 3,
            },
            cast: vec![bird],
            marks: vec![Mark::Pier {
                from: [0.0, 0.0],
                to: [9.0, 0.0],
                width: 3.0,
            }],
        };
        let file = serde_json::to_string_pretty(&scene).unwrap();
        let read: LifeScene = serde_json::from_str(&file).unwrap();
        assert_eq!(read, scene);
        let (going, coming) = (Playback::new(&scene), Playback::new(&read));
        for tick in (0..going.period).step_by(7) {
            assert_eq!(going.place(0, tick, &LAND), coming.place(0, tick, &LAND));
        }
    }

    /// What someone would write by hand: all that has a sensible default can be left out.
    #[test]
    fn a_scene_can_be_written_by_hand() {
        let file = r#"{
            "name": "Le ballon de Lou",
            "way": {"Route": {
                "turns": [{"at": [0, 0]}, {"at": [40, 0], "rest": 60}],
                "ends": "BackAndForth",
                "speed": 0.2
            }},
            "cast": [
                {"model": "Stroller", "size": 14.4},
                {"model": "Balloons", "size": 20, "up": 12, "behind": 1.5, "bob": 0.5}
            ]
        }"#;
        let scene: LifeScene = serde_json::from_str(file).unwrap();
        let play = Playback::new(&scene);
        assert_eq!(play.actors(), 2);
        assert!(play.place(1, 300, &LAND).is_some());
    }

    #[test]
    fn a_typo_in_a_file_is_refused_rather_than_ignored() {
        let file = r#"{"name": "x", "way": {"Still": {"at": [0, 0], "facing": 0}},
            "cast": [{"modle": "Gull", "size": 1}]}"#;
        assert!(serde_json::from_str::<LifeScene>(file).is_err());
    }

    #[test]
    fn rails_turn_to_sleepers_and_ballast_and_a_pier_to_planks() {
        let scene = LifeScene {
            name: "line".into(),
            way: Way::Route {
                turns: vec![turn(0.0, 0.0), turn(60.0, 0.0)],
                ends: Ends::BackAndForth,
                speed: 1.0,
            },
            cast: Vec::new(),
            marks: vec![
                Mark::Rails { width: 3.0 },
                Mark::Pier {
                    from: [0.0, 30.0],
                    to: [9.0, 30.0],
                    width: 1.0,
                },
            ],
        };
        let cells = scene.laid(&LAND);
        let has = |laid| cells.iter().any(|(_, one)| *one == laid);
        assert!(has(Laid::Ballast) && has(Laid::Sleeper) && has(Laid::Planks));
        let plank = |cell: &(i32, i32)| cell.1 == 30;
        assert!(
            cells
                .iter()
                .filter(|(cell, _)| plank(cell))
                .all(|(_, laid)| *laid == Laid::Planks)
        );
    }

    /// The rule the map relies on, kept where it cannot be forgotten: what plays scenes
    /// knows neither the generator of the world nor a model.
    #[test]
    fn the_engine_reads_no_generator_and_no_model() {
        let source = include_str!("life.rs");
        let code = source.split("#[cfg(test)]").next().unwrap();
        for word in [
            "WorldGen",
            "WorldGenerator",
            "Terrain",
            "crate::scene",
            "crate::plugin",
        ] {
            assert!(!code.contains(word), "{word}");
        }
    }
}
