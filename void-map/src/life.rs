//! Scenes of life on the map: birds, a train, someone fishing, a balloon.
//!
//! A [`LifeScene`] is data: who is in it, the way they keep to, and what it leaves on the
//! ground. It serialises, so that a file can hold it as well as code can make it.
//! [`Playback`] is the only way to play one, and asks nothing of the map but [`Land`]: this
//! module reads no generator, and no model of the map either. A map made from the world
//! gets its scenes from [`habitat`], and a map made by hand would read them from a file:
//! from there on, they go down the same road.
//!
//! Where anyone is at a tick is a function of that tick. There is no state to keep up, so
//! a scene nobody looks at costs nothing, and looking further ahead is as cheap as looking
//! at now, which is what lets a client glide from one place to the next: see [`show`].
//! The heights of a way on a model are worked out once, as a [`Course`]: a way follows a
//! curve and never the steps of a terrace.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod habitat;
pub mod models;
pub mod show;

/// How far apart a round way is cut, and its heights are taken, in world blocks.
const SAMPLE: f64 = 3.0;
/// The most a line climbs, in blocks of a model for each block along it.
const STEEPEST: f64 = 0.22;
/// How far a line rounds off its slopes, in world blocks.
const LINE_ROUNDING: f64 = 12.0;
/// How far ahead and behind what flies looks at the ground, in world blocks.
const FLIGHT_SIGHT: f64 = 36.0;
/// How long a stretch what flies evens out its height over, in world blocks.
const FLIGHT_ROUNDING: f64 = 60.0;
/// A deck over the water leaves room for a boat under it, in world blocks over the water.
const VIADUCT_CLEARANCE: f64 = 18.0;
/// A pier of a viaduct every so many cells.
const PILLAR_SPACING: f64 = 9.0;
/// Ticks a way takes to get up to speed from a stop, and to come down from it.
const EASING: u32 = 40;
/// How far from the middle of what leans its turn is measured, at the least, in world blocks.
const LEANING_REACH: f64 = 15.0;
/// The most anything leans into a turn, in degrees.
const STEEPEST_BANK: f64 = 38.0;
/// How far apart marks are stamped along their line, in cells.
const STAMP_STEP: f64 = 0.4;
/// The least width of a line, in cells: what holds on the far model holds on the others.
const LINE_CELLS: f64 = 3.0;
/// Sleepers come every so many cells, on every model.
const SLEEPER_PERIOD: f64 = 3.0;
/// How far from the middle of a line its single rail reaches, in cells.
const RAIL_HALF: f64 = 0.65;
/// How high an embankment stands over the land, in cells, and how far its slopes reach.
const EMBANKMENT_RISE: f64 = 2.0;
const EMBANKMENT_SLOPE: f64 = 1.5;

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
    /// The same, along one curve through all the turns, with no corner anywhere.
    Round,
    /// It turns round, and comes back the same way.
    BackAndForth,
}

/// One entity of a scene.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub model: Model,
    /// The height of its model, in world blocks, so that it follows the zoom. It is as long
    /// as that too: both its ends keep to the way, as the bogies of a carriage do.
    pub size: f64,
    /// The least it can be, in blocks of the model: what is true to scale on the close
    /// model can be under a pixel on the far one.
    #[serde(default)]
    pub least: f64,
    /// How far back along the way, in world blocks: what follows the same rails.
    #[serde(default)]
    pub behind: f64,
    /// How far to the right of the way, in world blocks: its place in a formation, which
    /// turns with the way.
    #[serde(default)]
    pub beside: f64,
    /// Where it stands from the way, east and south, in world blocks.
    #[serde(default)]
    pub offset: [f64; 2],
    /// Over the way, in world blocks.
    #[serde(default)]
    pub up: f64,
    /// How far it strays from there and back, in world blocks.
    #[serde(default)]
    pub drift: f64,
    /// How far it rises and falls, in world blocks.
    #[serde(default)]
    pub bob: f64,
    /// How far it leans into a turn, in degrees for each degree the way turns in a block.
    #[serde(default)]
    pub bank: f64,
    /// It lies on the ground under the way, as a shadow does, however high the way goes.
    #[serde(default)]
    pub on_the_ground: bool,
}

impl Actor {
    pub fn new(model: Model, size: f64) -> Self {
        Self {
            model,
            size,
            least: 0.0,
            behind: 0.0,
            beside: 0.0,
            offset: [0.0; 2],
            up: 0.0,
            drift: 0.0,
            bob: 0.0,
            bank: 0.0,
            on_the_ground: false,
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
    Locomotive,
    Carriage,
    Wagon,
    Angler,
    /// A float on the water, with its rings.
    Float,
    /// A hot-air balloon, in the colours of each.
    Balloon,
    BalloonBlue,
    /// What a balloon throws on the ground.
    BalloonShadow,
}

/// What a scene leaves on the map.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Mark {
    /// Along the scene's own way, this wide in world blocks.
    Rails {
        width: f64,
        #[serde(default)]
        track: Track,
    },
    /// Planks out over the water, this wide in world blocks.
    Pier {
        from: [f64; 2],
        to: [f64; 2],
        width: f64,
    },
}

/// How a line looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Track {
    /// A ladder: two rails, and dark sleepers across them.
    #[default]
    Sleepers,
    /// A pale band with one dark rail down its middle.
    PaleBallast,
    /// The same on a bank of its own, which stands out of the land.
    Embankment,
}

impl Track {
    /// What a cell of a line turns into, from how far along it is and how far from its
    /// middle, in cells, and how far under the crest of the line it lies.
    fn cell(self, along: f64, across: f64) -> (Laid, i32) {
        let edge = LINE_CELLS / 2.0 - 0.55;
        match self {
            Self::Sleepers if along.rem_euclid(SLEEPER_PERIOD) < 1.0 => (Laid::Sleeper, 0),
            Self::Sleepers if across >= edge => (Laid::Rail, 0),
            Self::Sleepers => (Laid::Ballast, 0),
            Self::PaleBallast | Self::Embankment if across < RAIL_HALF => (Laid::DarkRail, 0),
            Self::PaleBallast => (Laid::PaleBallast, 0),
            Self::Embankment if across <= LINE_CELLS / 2.0 => (Laid::PaleBallast, 0),
            Self::Embankment => (Laid::Ballast, 1),
        }
    }

    /// How far its crest stands over the line, in cells.
    fn rise(self) -> f64 {
        match self {
            Self::Embankment => EMBANKMENT_RISE,
            Self::Sleepers | Self::PaleBallast => 0.0,
        }
    }

    /// How far either side of its middle it reaches, at the least, in cells.
    fn half_width(self) -> f64 {
        match self {
            Self::Embankment => LINE_CELLS / 2.0 + EMBANKMENT_SLOPE,
            Self::Sleepers | Self::PaleBallast => LINE_CELLS / 2.0 + 0.2,
        }
    }
}

/// What a cell of a model turns into under a [`Mark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Laid {
    Ballast,
    PaleBallast,
    Sleeper,
    Rail,
    DarkRail,
    Planks,
}

/// A cell of a model under a [`Mark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lay {
    pub at: (i32, i32),
    pub laid: Laid,
    /// The height of the block on top, for a line. A pier lies on the water.
    pub level: Option<i32>,
    /// Over water, it stands on a pier of masonry here.
    pub pillar: bool,
}

impl LifeScene {
    /// What its marks turn into on a model, which must not have any on it yet: each cell
    /// and what lies on it. A cell goes by the stamp whose middle it is closest to,
    /// whichever came last.
    pub fn laid(&self, land: &impl Land) -> Vec<Lay> {
        let mut cells = Stamps::new();
        let play = Playback::new(self);
        for mark in &self.marks {
            match mark {
                Mark::Rails { width, track } => {
                    let course = play.course(land);
                    let half = (land.length(*width) / 2.0).max(track.half_width());
                    let step = STAMP_STEP * (SAMPLE / land.length(SAMPLE));
                    let steps = (play.length / step).ceil() as u32;
                    for index in 0..steps {
                        let distance = f64::from(index) * step;
                        let along = land.length(distance);
                        let crest = course.way_at(distance).round() as i32 - 1;
                        let at = land.on_model(play.along(distance));
                        stamp(at, half, &mut cells, |across| {
                            let (laid, drop) = track.cell(along, across);
                            Lay {
                                at: (0, 0),
                                laid,
                                level: Some(crest - drop),
                                pillar: across < 1.0 && along.rem_euclid(PILLAR_SPACING) < 1.0,
                            }
                        });
                    }
                }
                Mark::Pier { from, to, width } => {
                    let length = (to[0] - from[0]).hypot(to[1] - from[1]);
                    let steps = (land.length(length) / STAMP_STEP).ceil().max(1.0) as u32;
                    let half = (land.length(*width) / 2.0).max(0.75);
                    for step in 0..=steps {
                        let at = between(*from, *to, f64::from(step) / f64::from(steps));
                        stamp(land.on_model(at), half, &mut cells, |_| Lay {
                            at: (0, 0),
                            laid: Laid::Planks,
                            level: None,
                            pillar: false,
                        });
                    }
                }
            }
        }
        cells
            .into_iter()
            .map(|(at, (_, lay))| Lay { at, ..lay })
            .collect()
    }
}

/// The cells of a mark, with how far from its middle each one was stamped.
type Stamps = BTreeMap<(i32, i32), (f64, Lay)>;

/// Stamps a disc around a place of a model. `lay` is told how far from the middle a cell is.
fn stamp([x, z]: [f64; 2], half: f64, cells: &mut Stamps, lay: impl Fn(f64) -> Lay) {
    let (cell_x, cell_z) = (x.floor() as i32, z.floor() as i32);
    let reach = half.ceil() as i32;
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            let (mid_x, mid_z) = (f64::from(cell_x + dx) + 0.5, f64::from(cell_z + dz) + 0.5);
            let across = (mid_x - x).hypot(mid_z - z);
            if across <= half {
                let stamp = (across, lay(across));
                cells
                    .entry((cell_x + dx, cell_z + dz))
                    .and_modify(|best| {
                        if across < best.0 {
                            *best = stamp;
                        }
                    })
                    .or_insert(stamp);
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

/// A closed curve through turns: the way each leg leaves one is the way it reaches the next.
fn rounded(turns: &[Turn]) -> Vec<Turn> {
    let count = turns.len();
    let mut dense = Vec::new();
    for index in 0..count {
        let at = |shift: usize| turns[(index + shift + count - 1) % count].at;
        let [before, from, to, after] = [at(0), at(1), at(2), at(3)];
        let leg = (to[0] - from[0]).hypot(to[1] - from[1]);
        let pieces = (leg / SAMPLE).ceil().max(1.0) as u32;
        let next = turns[(index + 1) % count];
        for piece in 0..pieces {
            let t = f64::from(piece) / f64::from(pieces);
            let axis = |axis: usize| {
                let [a, b, c, d] = [before[axis], from[axis], to[axis], after[axis]];
                b + 0.5
                    * t
                    * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
            };
            dense.push(Turn {
                at: [axis(0), axis(1)],
                above: turns[index].above + (next.above - turns[index].above) * t,
                rest: if piece == 0 { turns[index].rest } else { 0 },
            });
        }
    }
    dense
}

/// Averages a closed line of heights, twice over, which rounds its corners.
fn evened(heights: &mut [f64], reach: usize) {
    let count = heights.len();
    for _ in 0..2 {
        let source = heights.to_vec();
        let width = (reach * 2 + 1) as f64;
        for (index, height) in heights.iter_mut().enumerate() {
            let sum: f64 = (0..=reach * 2)
                .map(|at| source[(index + at + count * (reach + 1) - reach) % count])
                .sum();
            *height = sum / width;
        }
    }
}

/// The highest of a closed line of heights, around each of them.
fn highest_around(heights: &[f64], reach: usize) -> Vec<f64> {
    let count = heights.len();
    (0..count)
        .map(|index| {
            (0..=reach * 2)
                .map(|at| heights[(index + at + count * (reach + 1) - reach) % count])
                .fold(f64::MIN, f64::max)
        })
        .collect()
}

/// Raises what is too far under its neighbours: a terrace or a deck comes with a ramp.
fn ramped(heights: &mut [f64], rise: f64) {
    let count = heights.len();
    for round in 0..count * 4 {
        let (at, before) = if round < count * 2 {
            (round % count, (round + count - 1) % count)
        } else {
            let at = count - 1 - round % count;
            (at, (at + 1) % count)
        };
        heights[at] = heights[at].max(heights[before] - rise);
    }
}

/// The heights of a way on one model, worked out from the bare land, before any mark.
#[derive(Debug, Clone, PartialEq)]
pub struct Course {
    /// How far apart the heights are, in world blocks.
    step: f64,
    length: f64,
    /// Of the way itself.
    way: Vec<f64>,
    /// Of the ground under it, on the highest of what something lying there covers.
    ground: Vec<f64>,
}

impl Course {
    fn height(&self, heights: &[f64], distance: f64) -> f64 {
        let spot = distance.rem_euclid(self.length.max(f64::MIN_POSITIVE)) / self.step;
        let count = heights.len();
        let (at, share) = (spot.floor() as usize % count, spot.fract());
        heights[at] + (heights[(at + 1) % count] - heights[at]) * share
    }

    fn way_at(&self, distance: f64) -> f64 {
        self.height(&self.way, distance)
    }

    fn ground_at(&self, distance: f64) -> f64 {
        self.height(&self.ground, distance)
    }
}

/// A place of a way, and how far along it is.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    at: [f64; 2],
    above: f64,
    along: f64,
}

/// A stretch of time of a way, at one speed or one change of it. It rests at 0.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Beat {
    start: u64,
    from: f64,
    speed: f64,
    /// What it gains each tick.
    gain: f64,
}

/// The beats of one run, from a stop or the start to the next, which ease into and out of
/// the stops at their ends.
fn run_beats(
    beats: &mut Vec<Beat>,
    tick: &mut u64,
    from: f64,
    run: f64,
    speed: f64,
    stops: [bool; 2],
) {
    let [start, end] = stops.map(|stop| if stop { f64::from(EASING) } else { 0.0 });
    let lost = (start + end) / 2.0;
    let ticks = (run / speed + lost).ceil().max(start + end + 1.0);
    let top = run / (ticks - lost);
    let mut beat = |length: f64, from: f64, speed: f64, gain: f64| {
        if length > 0.0 {
            beats.push(Beat {
                start: *tick,
                from,
                speed,
                gain,
            });
            *tick += length as u64;
        }
    };
    beat(start, from, 0.0, top / start.max(1.0));
    beat(ticks - start - end, from + top * start / 2.0, top, 0.0);
    beat(end, from + run - top * end / 2.0, top, -top / end.max(1.0));
}

/// One actor at one tick, on a model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// In blocks of the model, from its centre.
    pub at: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// How far it leans to its right, in degrees.
    pub roll: f32,
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
    /// How its rails look, if it has any.
    track: Option<Track>,
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
        };
        // All ends come down to a loop: a way there and back is one that passes twice over
        // the same ground.
        let mut round = match ends {
            Ends::Round => rounded(&turns),
            Ends::Loop | Ends::BackAndForth => turns.clone(),
        };
        match ends {
            Ends::Loop | Ends::Round => round.extend(round.first().copied()),
            Ends::BackAndForth => round.extend(turns.iter().rev().skip(1)),
        }
        let mut points = Vec::with_capacity(round.len());
        let (mut beats, mut tick, mut along) = (Vec::new(), 0, 0.0);
        let mut run_from = (0.0, round[0].rest > 0);
        for (index, turn) in round.iter().enumerate() {
            points.push(Point {
                at: turn.at,
                above: turn.above,
                along,
            });
            let last = index + 1 == round.len();
            if (turn.rest > 0 || last) && along > run_from.0 && speed > 0.0 {
                let run = along - run_from.0;
                let stops = [run_from.1, turn.rest > 0];
                run_beats(&mut beats, &mut tick, run_from.0, run, speed, stops);
                run_from = (along, turn.rest > 0);
            }
            if last {
                break;
            }
            if turn.rest > 0 {
                beats.push(Beat {
                    start: tick,
                    from: along,
                    speed: 0.0,
                    gain: 0.0,
                });
                tick += u64::from(turn.rest);
            }
            let next = round[index + 1];
            along += (next.at[0] - turn.at[0]).hypot(next.at[1] - turn.at[1]);
        }
        let track = scene.marks.iter().find_map(|mark| match mark {
            Mark::Rails { track, .. } => Some(*track),
            Mark::Pier { .. } => None,
        });
        Self {
            cast: scene.cast.clone(),
            points,
            beats,
            length: along,
            period: tick.max(1),
            facing,
            track,
        }
    }

    pub fn actors(&self) -> usize {
        self.cast.len()
    }

    pub fn actor(&self, index: usize) -> &Actor {
        &self.cast[index]
    }

    /// How far along the way the scene is.
    fn travelled(&self, tick: u64) -> f64 {
        let within = tick % self.period;
        let beat = self.beats.partition_point(|beat| beat.start <= within);
        beat.checked_sub(1)
            .and_then(|at| self.beats.get(at))
            .map_or(0.0, |beat| {
                let time = (within - beat.start) as f64;
                beat.from + beat.speed * time + beat.gain * time * time / 2.0
            })
    }

    /// The two turns around a distance along the way, and how far it is from one to the other.
    fn around(&self, distance: f64) -> (Point, Point, f64) {
        let distance = distance.rem_euclid(self.length);
        let next = self.points.partition_point(|point| point.along <= distance);
        let next = next.clamp(1, self.points.len() - 1);
        let (from, to) = (self.points[next - 1], self.points[next]);
        let share = (distance - from.along) / (to.along - from.along).max(f64::MIN_POSITIVE);
        (from, to, share)
    }

    /// The place at a distance along the way.
    fn along(&self, distance: f64) -> [f64; 2] {
        if self.length <= 0.0 {
            return self.points[0].at;
        }
        let (from, to, share) = self.around(distance);
        between(from.at, to.at, share)
    }

    /// How high over the ground the way goes at a distance along it, in world blocks.
    fn above(&self, distance: f64) -> f64 {
        if self.length <= 0.0 {
            return self.points[0].above;
        }
        let (from, to, share) = self.around(distance);
        from.above + (to.above - from.above) * share
    }

    /// The heights of this way on a model, which must not have its marks on it yet: a line
    /// is laid at these heights, and what comes after would read them back as ground. What
    /// flies goes by the highest of what is ahead, in long swells, and a line goes up a
    /// terrace by a ramp, over the water on a deck, and always over the land under it.
    pub fn course(&self, land: &impl Land) -> Course {
        let count = (self.length / SAMPLE).ceil().max(1.0) as usize;
        let step = self.length / count as f64;
        let step = if step > 0.0 { step } else { SAMPLE };
        let spots = (0..count).map(|at| self.along(at as f64 * step));
        let bare: Vec<Option<f64>> = spots.clone().map(|at| land.ground(at)).collect();
        let lowest = bare.iter().flatten().copied().fold(f64::MAX, f64::min);
        // Past the rim, there is nothing to keep clear of.
        let bare: Vec<f64> = bare.iter().map(|ground| ground.unwrap_or(lowest)).collect();
        let samples = |world: f64| ((world / step).round() as usize).min(count / 2);
        let flies = self.points.iter().any(|point| point.above > 0.0);
        let way = if flies {
            let mut way = highest_around(&bare, samples(FLIGHT_SIGHT));
            evened(&mut way, samples(FLIGHT_ROUNDING));
            for (at, height) in way.iter_mut().enumerate() {
                *height += land.length(self.above(at as f64 * step));
            }
            way
        } else {
            let mut way = bare.clone();
            if let Some(track) = self.track {
                let clearance = land.length(VIADUCT_CLEARANCE);
                for (height, at) in way.iter_mut().zip(spots) {
                    if land.water(at) {
                        *height += clearance;
                    } else {
                        *height += track.rise();
                    }
                }
                ramped(&mut way, STEEPEST * land.length(step));
                // Widened first, what rounds the slopes takes nothing off the top of them.
                let rounding = samples(LINE_ROUNDING);
                way = highest_around(&way, rounding);
                evened(&mut way, rounding / 2);
            }
            way
        };
        let widest = self.cast.iter().map(|actor| actor.size).fold(0.0, f64::max);
        let mut ground = highest_around(&bare, samples(widest / 2.0));
        evened(&mut ground, samples(widest / 2.0));
        Course {
            step,
            length: self.length,
            way,
            ground,
        }
    }

    /// Which way the way goes at a distance along it, from a little behind to a little ahead.
    fn heading(&self, distance: f64, reach: f64) -> f32 {
        let (rear, front) = (self.along(distance - reach), self.along(distance + reach));
        yaw_towards(front[0] - rear[0], front[1] - rear[1])
    }

    /// How far an actor leans at a distance along the way.
    fn lean(&self, actor: &Actor, distance: f64) -> f32 {
        if actor.bank == 0.0 || self.length <= 0.0 {
            return 0.0;
        }
        let reach = (actor.size / 2.0).max(LEANING_REACH);
        let before = self.heading(distance - reach, reach);
        let after = self.heading(distance + reach, reach);
        let turned = f64::from((after - before + 540.0).rem_euclid(360.0) - 180.0);
        (turned / (reach * 2.0) * actor.bank).clamp(-STEEPEST_BANK, STEEPEST_BANK) as f32
    }

    /// An actor at a tick. Both its ends are on the way: its middle is between them, it
    /// faces from one to the other, and it climbs as they do.
    pub fn place(&self, actor: usize, tick: u64, land: &impl Land, course: &Course) -> Placed {
        let cast = &self.cast[actor];
        let distance = self.travelled(tick) - cast.behind;
        let half = cast.size / 2.0;
        let (rear, front) = (self.along(distance - half), self.along(distance + half));
        let way = between(rear, front, 0.5);
        let (dx, dz) = (front[0] - rear[0], front[1] - rear[1]);
        let span = dx.hypot(dz);
        let (yaw, right) = if self.length > 0.0 && span > 0.0 {
            (yaw_towards(dx, dz), [-dz / span, dx / span])
        } else {
            (self.facing, [0.0, 0.0])
        };
        let time = tick as f64;
        // Each actor keeps time of its own, from its place in the cast.
        let phase = actor as f64 * 2.4;
        let [x, z] = land.on_model([
            way[0]
                + right[0] * cast.beside
                + cast.offset[0]
                + cast.drift * (time / 41.0 + phase).sin(),
            way[1]
                + right[1] * cast.beside
                + cast.offset[1]
                + cast.drift * (time / 53.0 + phase * 1.7).cos(),
        ]);
        let (rear, front) = (
            course.way_at(distance - half),
            course.way_at(distance + half),
        );
        let (y, pitch) = if cast.on_the_ground {
            (course.ground_at(distance), 0.0)
        } else {
            let climb = (front - rear).atan2(land.length(span).max(f64::MIN_POSITIVE));
            ((front + rear) / 2.0, -climb.to_degrees() as f32)
        };
        let bob = cast.bob * (time / 19.0 + phase).sin();
        Placed {
            at: [x, y + land.length(cast.up + bob), z],
            yaw,
            pitch,
            roll: self.lean(cast, distance),
            model: cast.model,
        }
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

    fn placed(play: &Playback, actor: usize, tick: u64) -> Placed {
        play.place(actor, tick, &LAND, &play.course(&LAND))
    }

    fn spot(play: &Playback, actor: usize, tick: u64) -> [f64; 3] {
        placed(play, actor, tick).at
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

    /// A ring of the given radius, as few turns as a hand would write.
    fn ring(radius: f64, above: f64) -> Vec<Turn> {
        (0..8)
            .map(|at| {
                let angle = f64::from(at) / 8.0 * std::f64::consts::TAU;
                Turn {
                    at: [radius * angle.cos(), radius * angle.sin()],
                    above,
                    rest: 0,
                }
            })
            .collect()
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
            vec![Actor::new(Model::Angler, 2.0)],
        );
        assert_eq!(play.period, 200);
        assert_eq!(spot(&play, 0, 150), [50.0, 65.0, 0.0]);
        assert_eq!(placed(&play, 0, 50).yaw, -90.0);
        assert_eq!(placed(&play, 0, 150).yaw, 90.0);
        assert_eq!(spot(&play, 0, 200), spot(&play, 0, 0));
    }

    /// One position every so many ticks is only smooth if a way has no jumps in it.
    #[test]
    fn no_tick_jumps_ahead_of_the_speed() {
        let mut stop = turn(100.0, 0.0);
        stop.rest = 30;
        let turns = vec![turn(0.0, 0.0), stop, turn(100.0, 100.0)];
        let play = walk(Ends::Loop, turns, vec![Actor::new(Model::Angler, 2.0)]);
        for tick in 0..play.period * 2 {
            let (a, b) = (spot(&play, 0, tick), spot(&play, 0, tick + 1));
            assert!((a[0] - b[0]).hypot(a[2] - b[2]) <= 1.05, "{tick}");
        }
    }

    /// No dry stop: from a station and into it, speed changes a little at a time.
    #[test]
    fn a_way_eases_into_a_stop_and_out_of_it() {
        let mut stop = turn(200.0, 0.0);
        stop.rest = 30;
        let mut start = turn(0.0, 0.0);
        start.rest = 30;
        let play = walk(Ends::BackAndForth, vec![start, stop], vec![]);
        let speeds: Vec<f64> = (0..play.period - 1)
            .map(|tick| play.travelled(tick + 1) - play.travelled(tick))
            .collect();
        let steps = speeds.windows(2).map(|pair| (pair[1] - pair[0]).abs());
        assert!(steps.fold(0.0, f64::max) <= 1.5 / f64::from(EASING));
        assert!(speeds.contains(&0.0));
        assert!(speeds.iter().any(|speed| *speed > 0.99));
    }

    #[test]
    fn a_way_rests_where_a_turn_says() {
        let mut stop = turn(100.0, 0.0);
        stop.rest = 30;
        let turns = vec![turn(0.0, 0.0), stop];
        let play = walk(
            Ends::BackAndForth,
            turns,
            vec![Actor::new(Model::Float, 0.0)],
        );
        let arrived = play
            .beats
            .iter()
            .find(|beat| beat.speed == 0.0 && beat.gain == 0.0);
        let arrived = arrived.unwrap().start;
        assert_eq!(spot(&play, 0, arrived + 5), [100.0, 65.0, 0.0]);
        assert_eq!(spot(&play, 0, arrived + 25), [100.0, 65.0, 0.0]);
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

    /// Each carriage has both ends on the rails: on a curve it faces along its chord, so a
    /// heading never jumps from one leg of a way to the next.
    #[test]
    fn a_long_actor_turns_little_by_little_on_a_corner() {
        let play = square(vec![Actor::new(Model::Carriage, 20.0)]);
        for tick in 80..120 {
            let (a, b) = (placed(&play, 0, tick).yaw, placed(&play, 0, tick + 1).yaw);
            assert!(
                ((b - a + 540.0).rem_euclid(360.0) - 180.0).abs() <= 6.0,
                "{tick}"
            );
        }
    }

    #[test]
    fn a_round_way_has_no_corner_and_goes_through_its_turns() {
        let play = walk(
            Ends::Round,
            ring(100.0, 0.0),
            vec![Actor::new(Model::Gull, 4.0)],
        );
        for turn in ring(100.0, 0.0) {
            let closest = (0..play.period).map(|tick| {
                let [x, _, z] = spot(&play, 0, tick);
                (x - turn.at[0]).hypot(z - turn.at[1])
            });
            assert!(closest.fold(f64::MAX, f64::min) < 1.5);
        }
        for distance in 0..play.length as u32 {
            let distance = f64::from(distance);
            let (a, b) = (
                play.heading(distance, 3.0),
                play.heading(distance + 1.0, 3.0),
            );
            assert!(((b - a + 540.0).rem_euclid(360.0) - 180.0).abs() < 1.5);
        }
    }

    /// A formation is a shape: its gulls stay as far apart in a turn as on a straight line,
    /// and the outer ones are never left behind.
    #[test]
    fn a_formation_turns_as_one_and_leans_into_its_turn() {
        let mut wing = Actor::new(Model::Gull, 4.0);
        wing.behind = 15.0;
        wing.beside = 15.0;
        wing.bank = 34.0;
        let mut leader = Actor::new(Model::Gull, 4.0);
        leader.bank = 34.0;
        let play = walk(Ends::Round, ring(100.0, 20.0), vec![leader, wing]);
        for tick in (0..play.period).step_by(13) {
            let (a, b) = (spot(&play, 0, tick), spot(&play, 1, tick));
            let apart = (a[0] - b[0]).hypot(a[2] - b[2]);
            assert!((apart - 15.0 * 2f64.sqrt()).abs() < 3.0, "{tick} {apart}");
            let lean = placed(&play, 0, tick).roll;
            assert!(
                (12.0..=STEEPEST_BANK as f32).contains(&lean.abs()),
                "{lean}"
            );
        }
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
        let first = placed(&play, 0, 0);
        assert_eq!(first.yaw, 45.0);
        assert_eq!(first.at, [10.0, 65.0, 20.0]);
        assert_eq!(placed(&play, 0, 1234), first);
    }

    #[test]
    fn a_scene_is_at_the_same_share_of_the_land_on_every_model() {
        let play = square(vec![Actor::new(Model::Locomotive, 10.0)]);
        let coarse = Plain {
            shore: 500.0,
            per_cell: 5.0,
        };
        let on_it = play.place(0, 150, &coarse, &play.course(&coarse));
        assert_eq!(on_it.at, [20.0, 65.0, 10.0]);
    }

    /// Land with a terrace and a lake on the way of a line, east of it.
    struct Steps;

    impl Land for Steps {
        fn on_model(&self, at: [f64; 2]) -> [f64; 2] {
            at
        }

        fn length(&self, world: f64) -> f64 {
            world
        }

        fn ground(&self, at: [f64; 2]) -> Option<f64> {
            Some(if at[0] < 0.0 {
                65.0
            } else if at[0] < 200.0 {
                71.0
            } else {
                65.0
            })
        }

        fn water(&self, at: [f64; 2]) -> bool {
            at[0] >= 200.0
        }

        fn free(&self, at: [f64; 2]) -> bool {
            !self.water(at)
        }
    }

    fn line_over_steps() -> LifeScene {
        LifeScene {
            name: "line".into(),
            way: Way::Route {
                turns: vec![turn(-300.0, 0.0), turn(300.0, 0.0)],
                ends: Ends::BackAndForth,
                speed: 1.0,
            },
            cast: vec![Actor::new(Model::Locomotive, 20.0)],
            marks: vec![Mark::Rails {
                width: 3.0,
                track: Track::Sleepers,
            }],
        }
    }

    /// A terrace is a ramp, gentle, and never goes through what climbs it.
    #[test]
    fn a_line_climbs_a_terrace_by_a_ramp_and_stays_over_the_land() {
        let play = Playback::new(&line_over_steps());
        let course = play.course(&Steps);
        for distance in 0..600 {
            let distance = f64::from(distance);
            let (a, b) = (course.way_at(distance), course.way_at(distance + 1.0));
            assert!((a - b).abs() <= STEEPEST + 0.01, "{distance}");
            let ground = Steps.ground(play.along(distance)).unwrap();
            assert!(a >= ground - 0.6, "{distance} {a} {ground}");
        }
    }

    #[test]
    fn a_carriage_tilts_as_it_climbs_and_lies_level_on_the_flat() {
        let play = Playback::new(&line_over_steps());
        let course = play.course(&Steps);
        let climbing = (0..700).map(|tick| play.place(0, tick, &Steps, &course).pitch);
        let steepest = climbing.fold(0.0f32, |most, pitch| most.max(pitch.abs()));
        assert!((5.0..=14.0).contains(&steepest), "{steepest}");
        assert_eq!(play.place(0, 40, &Steps, &course).pitch, 0.0);
    }

    /// A viaduct stands clear of the water, for boats to pass.
    #[test]
    fn a_line_goes_over_water_on_a_deck_high_enough_for_a_boat() {
        let play = Playback::new(&line_over_steps());
        let course = play.course(&Steps);
        assert!(course.way_at(550.0) >= 65.0 + VIADUCT_CLEARANCE - 0.1);
    }

    #[test]
    fn a_shadow_keeps_to_the_ground_under_what_flies() {
        let mut balloon = Actor::new(Model::Balloon, 10.0);
        balloon.bob = 0.0;
        let mut shadow = Actor::new(Model::BalloonShadow, 8.0);
        shadow.on_the_ground = true;
        let play = walk(Ends::Round, ring(100.0, 50.0), vec![balloon, shadow]);
        let (up, down) = (spot(&play, 0, 40), spot(&play, 1, 40));
        assert_eq!(down[1], 65.0);
        assert!(up[1] >= 114.0);
        assert!((up[0] - down[0]).hypot(up[2] - down[2]) < 0.2);
    }

    /// A file and code are the same road: what comes back from a file plays exactly what
    /// went in, tick by tick.
    #[test]
    fn a_scene_comes_back_from_a_file_and_plays_the_same() {
        let mut bird = Actor::new(Model::Gull, 9.0);
        bird.beside = 3.0;
        bird.offset = [3.0, -2.0];
        bird.drift = 2.5;
        bird.bank = 20.0;
        let scene = LifeScene {
            name: "gulls".into(),
            way: Way::Route {
                turns: ring(80.0, 24.0)
                    .into_iter()
                    .map(|turn| Turn {
                        at: turn.at.map(f64::round),
                        ..turn
                    })
                    .collect(),
                ends: Ends::Round,
                speed: 1.1,
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
            assert_eq!(placed(&going, 0, tick), placed(&coming, 0, tick));
        }
    }

    /// What someone would write by hand: all that has a sensible default can be left out.
    #[test]
    fn a_scene_can_be_written_by_hand() {
        let file = r#"{
            "name": "Le ballon de Lou",
            "way": {"Route": {
                "turns": [
                    {"at": [0, 0], "above": 60},
                    {"at": [200, 40], "above": 60},
                    {"at": [120, 240], "above": 80}
                ],
                "ends": "Round",
                "speed": 0.2
            }},
            "cast": [
                {"model": "Balloon", "size": 42, "bob": 1},
                {"model": "BalloonShadow", "size": 34, "on_the_ground": true}
            ]
        }"#;
        let scene: LifeScene = serde_json::from_str(file).unwrap();
        let play = Playback::new(&scene);
        assert_eq!(play.actors(), 2);
        assert!(spot(&play, 0, 300)[1] > spot(&play, 1, 300)[1]);
    }

    #[test]
    fn a_typo_in_a_file_is_refused_rather_than_ignored() {
        let file = r#"{"name": "x", "way": {"Still": {"at": [0, 0], "facing": 0}},
            "cast": [{"modle": "Gull", "size": 1}]}"#;
        assert!(serde_json::from_str::<LifeScene>(file).is_err());
    }

    fn laid_line(track: Track) -> Vec<Lay> {
        let scene = LifeScene {
            name: "line".into(),
            way: Way::Route {
                turns: vec![turn(0.0, 0.0), turn(60.0, 0.0)],
                ends: Ends::BackAndForth,
                speed: 1.0,
            },
            cast: Vec::new(),
            marks: vec![
                Mark::Rails { width: 3.0, track },
                Mark::Pier {
                    from: [0.0, 30.0],
                    to: [9.0, 30.0],
                    width: 1.0,
                },
            ],
        };
        scene.laid(&LAND)
    }

    #[test]
    fn every_look_of_a_line_lays_what_it_is_made_of_and_a_pier_planks() {
        let has = |cells: &[Lay], laid| cells.iter().any(|lay| lay.laid == laid);
        let ladder = laid_line(Track::Sleepers);
        assert!(has(&ladder, Laid::Sleeper) && has(&ladder, Laid::Rail));
        let pale = laid_line(Track::PaleBallast);
        assert!(has(&pale, Laid::PaleBallast) && has(&pale, Laid::DarkRail));
        let bank = laid_line(Track::Embankment);
        assert!(has(&bank, Laid::Ballast) && has(&bank, Laid::DarkRail));
        let crest = bank.iter().filter_map(|lay| lay.level).max().unwrap();
        assert_eq!(crest, 64 + EMBANKMENT_RISE as i32);
        let planks = ladder.iter().filter(|lay| lay.at.1 == 30);
        assert!(planks.clone().count() > 0);
        assert!(planks.clone().all(|lay| lay.laid == Laid::Planks));
        assert!(planks.clone().all(|lay| lay.level.is_none()));
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
