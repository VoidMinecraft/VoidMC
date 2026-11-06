//! Who walks the world, as small figures on the model.
//!
//! [`Characters`] is the list the map reads: a name, a skin, a place in the real world.
//! It does not say who plays them. [`Simulation`] fills it with walkers for now, and a
//! system that copies connected players into it would show them the same way.
//!
//! A figure is a `minecraft:player` no client plays, one per character and per session,
//! since each session has its own model. A mannequin would do, but a client only draws one
//! up to 64 blocks times the size of its box, and a player ten times as far: only that
//! lets a figure be as small as the model calls for. The client animates it from how it
//! moves, and it stays out of the way of the pointer: the eye sits inside its box, so the
//! look always hits that first. Hovering one is decided here, the same way as for a
//! marker, and only when no marker wants the look.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use uuid::Uuid;
use voidmc::{
    AllSkinLayers, Attributes, Billboard, Display, DisplayTransform, EntityAttribute,
    EntityBuilder, EntityKind, Glowing, NoGravity, Players, Silent, TextDisplay, WorldGen,
    components::{EntityUuid, Position, Rotation},
};
use voidmc_protocol::clientbound::{
    PlayerInfoActions, PlayerInfoEntry, PlayerInfoRemove, PlayerInfoUpdate, ResolvableProfile,
};

use crate::plugin::{Atlas, MapSession, MapView};
use crate::pointer::{AimReach, aimed_within};
use crate::scene::{SEA_Y, Scale, Scene, WORLD_RADIUS};

/// Walkers when `VOID_MAP_WALKERS` does not say otherwise.
const WALKERS: usize = 20;
const SEED: u64 = 0x766f_6964_7761_6c6b;
/// Keeps the ids of figures apart from those of characters.
const FIGURE_IDS: u64 = 0x6669_6775_7265_7321;

/// A vanilla player's walk, in blocks per tick.
const WALK_SPEED: f64 = 4.317 / 20.0;
/// How far a walker turns in a tick, in degrees.
const TURN_SPEED: f32 = 24.0;
/// How long a walker stays at a place, in ticks.
const REST_TICKS: std::ops::Range<u64> = 40..240;
/// How far from the middle of a place a walker stops, in blocks: past its building, and
/// not all on the same spot.
const STOP_AROUND: std::ops::Range<u64> = 26..48;
/// Spots tried around a place. Those a walker cannot reach are dropped.
const STOP_TRIES: usize = 24;
/// The grid ways are searched on, in blocks: a cell of the detailed model, whose bridges
/// are the narrowest thing to follow.
const WAY_GRID: i32 = 3;
/// How far ground reaches, in cells of that grid.
const GROUND_REACH: i32 = WORLD_RADIUS as i32 / WAY_GRID + 1;
const GROUND_SIDE: i32 = GROUND_REACH * 2 + 1;
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

/// A client draws a player of size 1 this far, and no further: 64 blocks times ten times
/// the average side of its box, which is one block at that size.
#[cfg(test)]
const DRAWN_PER_SCALE: f64 = 640.0;
/// How much larger than life a figure is, on every model: at the true scale, a person
/// would be a few pixels. From the heights of a zoom, this makes one a large pawn close up
/// and a speck from the farthest.
const LARGER_THAN_LIFE: f64 = 8.0;
const PLAYER_HEIGHT: f64 = 1.8;
/// Just under a whole block: what still falls in the same one.
const CELL_INSIDE: f64 = 0.999;
/// How fast a figure climbs a terrace of the model, in blocks per tick.
const CLIMB_SPEED: f64 = 0.2;
/// The furthest ahead a moving figure looks for the ground, in blocks of the model.
const STEP_AHEAD: f64 = 1.3;
/// How close to the middle of the screen a figure has to be to be named, at the least.
const FIGURE_REACH: AimReach = AimReach {
    enter: 1.2,
    leave: 1.8,
    switch: 0.5,
};

/// As for `Display::teleport_ticks`: what a living entity glides over on the client.
const GLIDE_TICKS: u8 = 3;
const LABEL_VIEW_RANGE: f32 = 8.0;
const LABEL_BACKGROUND: i32 = 0xC0_10_14_1C_u32 as i32;
/// Keeps a name the same size on screen, from how far away it is seen.
const LABEL_SCALE_PER_BLOCK: f64 = 0.08;
/// Between a head and a name, in heights of the figure.
const LABEL_GAP: f64 = 0.15;

const SUMMARY_PERIOD: u32 = 200;

/// The default skins of the client, which picks one from the hash of an id.
const DEFAULT_SKINS: u128 = 18;

const NAMES: [&str; 20] = [
    "Ambre", "Basile", "Camille", "Dorian", "Elsa", "Fanny", "Gaspard", "Hugo", "Iris", "Jules",
    "Katell", "Lou", "Maël", "Nina", "Oscar", "Pia", "Quentin", "Rose", "Sacha", "Tess",
];

/// Someone in the world. Nothing here says whether a person plays them.
#[derive(Debug, Clone, PartialEq)]
pub struct Character {
    pub name: String,
    pub skin: ResolvableProfile,
    /// In the real world.
    pub position: Position,
    /// The way they face, in Minecraft degrees.
    pub yaw: f32,
}

/// Everyone the map shows, by a stable id: an account's UUID, for a real player.
#[derive(Resource, Debug, Default)]
pub struct Characters(pub BTreeMap<u128, Character>);

/// A small, reproducible source of numbers.
#[derive(Debug, Clone)]
struct Dice(u64);

impl Dice {
    /// Splitmix64.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn within(&mut self, range: std::ops::Range<u64>) -> u64 {
        range.start + self.next() % (range.end - range.start)
    }

    /// Between 0 and 1.
    fn share(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Walker {
    id: u128,
    /// The place they are at, or walking to.
    place: usize,
    /// The place they left and where they stopped there, to go back to.
    left: (usize, [f64; 2]),
    /// The turns of their way, the last one being where they stop.
    path: Vec<[f64; 2]>,
    /// The turn they are walking to.
    leg: usize,
    resting: u64,
    /// The block whose ground they stand on.
    block: [i32; 2],
}

/// Somewhere walkers go.
#[derive(Debug, Clone, PartialEq)]
struct Place {
    /// Where a walker can stop, all on one piece of ground. None if the place has no
    /// walkable ground around it.
    stops: Vec<[f64; 2]>,
    /// The places that can be walked to from here.
    ways: Vec<usize>,
    /// The way here from anywhere on its piece of ground.
    posts: Signposts,
}

/// Where walkers can be, as cells of [`WAY_GRID`] blocks. What is not on it, they never
/// step on: a way only goes through it, and a step checks it again.
#[derive(Debug, Clone, PartialEq)]
struct Ground {
    /// Which piece of ground each cell is on, from 1, or 0 where nobody can walk.
    pieces: Vec<u32>,
    /// How many cells each piece has, the first one standing for no piece.
    sizes: Vec<u32>,
}

/// The way to one place from everywhere: the step to take on each cell of the ground, as
/// an index in [`AROUND`].
#[derive(Debug, Clone, PartialEq)]
struct Signposts(Vec<u8>);

const NO_STEP: u8 = u8::MAX;
const HERE: u8 = u8::MAX - 1;

impl Signposts {
    /// The cells from one to the place, both included.
    fn towards_root(&self, mut cell: [i32; 2]) -> Option<Vec<[i32; 2]>> {
        let mut cells = vec![cell];
        loop {
            match *self.0.get(index_of(cell)?)? {
                NO_STEP => return None,
                HERE => return Some(cells),
                step => {
                    let [dx, dz] = AROUND[usize::from(step)];
                    cell = [cell[0] + dx, cell[1] + dz];
                    cells.push(cell);
                }
            }
        }
    }
}

fn cell_at([x, z]: [f64; 2]) -> [i32; 2] {
    let cell = |value: f64| (value / f64::from(WAY_GRID)).floor() as i32;
    [cell(x), cell(z)]
}

fn middle_of([x, z]: [i32; 2]) -> [f64; 2] {
    let middle = |cell: i32| (f64::from(cell) + 0.5) * f64::from(WAY_GRID);
    [middle(x), middle(z)]
}

fn index_of([x, z]: [i32; 2]) -> Option<usize> {
    let range = -GROUND_REACH..=GROUND_REACH;
    (range.contains(&x) && range.contains(&z))
        .then(|| ((z + GROUND_REACH) * GROUND_SIDE + x + GROUND_REACH) as usize)
}

fn cell_of_index(index: usize) -> [i32; 2] {
    let index = index as i32;
    [
        index % GROUND_SIDE - GROUND_REACH,
        index / GROUND_SIDE - GROUND_REACH,
    ]
}

impl Ground {
    /// Reads `walkable`, in world blocks, once for each cell.
    fn new(walkable: &impl Fn(i32, i32) -> bool) -> Self {
        let count = (GROUND_SIDE * GROUND_SIDE) as usize;
        let open = |index: usize| {
            let [x, z] = cell_of_index(index);
            u32::from(walkable(x * WAY_GRID, z * WAY_GRID))
        };
        let mut ground = Self {
            pieces: (0..count).map(open).collect(),
            sizes: vec![0],
        };
        // All open cells start on piece 1, and each flood takes its own out of it.
        let mut next = 2;
        for index in 0..count {
            if ground.pieces[index] != 1 {
                continue;
            }
            let mut size = 1;
            ground.pieces[index] = next;
            let mut queue = VecDeque::from([cell_of_index(index)]);
            while let Some(cell) = queue.pop_front() {
                let steps: Vec<_> = ground.steps_from(cell).collect();
                for neighbour in steps {
                    if let Some(at) = index_of(neighbour).filter(|at| ground.pieces[*at] == 1) {
                        ground.pieces[at] = next;
                        size += 1;
                        queue.push_back(neighbour);
                    }
                }
            }
            ground.sizes.push(size);
            next += 1;
        }
        for piece in ground.pieces.iter_mut().filter(|piece| **piece > 1) {
            *piece -= 1;
        }
        ground
    }

    fn piece(&self, cell: [i32; 2]) -> u32 {
        index_of(cell).map_or(0, |index| self.pieces[index])
    }

    fn walkable(&self, cell: [i32; 2]) -> bool {
        self.piece(cell) != 0
    }

    /// Whether a walker can be at this place of the world.
    fn holds(&self, at: [f64; 2]) -> bool {
        self.walkable(cell_at(at))
    }

    /// The cells one step from another. A diagonal does not squeeze between two corners.
    fn steps_from(&self, [x, z]: [i32; 2]) -> impl Iterator<Item = [i32; 2]> + '_ {
        AROUND
            .into_iter()
            .filter(move |[dx, dz]| {
                self.walkable([x + dx, z + dz])
                    && self.walkable([x + dx, z])
                    && self.walkable([x, z + dz])
            })
            .map(move |[dx, dz]| [x + dx, z + dz])
    }

    /// Whether a straight line stays on the ground, through every cell it touches.
    fn clear(&self, from: [f64; 2], to: [f64; 2]) -> bool {
        let grid = f64::from(WAY_GRID);
        let (mut cell, last) = (cell_at(from), cell_at(to));
        let delta = [to[0] - from[0], to[1] - from[1]];
        let step = delta.map(|value| if value < 0.0 { -1 } else { 1 });
        // How far along the line, from 0 to 1, each axis next crosses into a cell.
        let crossing = |cell: i32, axis: usize| {
            let edge = f64::from(cell + i32::from(step[axis] > 0)) * grid;
            if delta[axis] == 0.0 {
                f64::INFINITY
            } else {
                (edge - from[axis]) / delta[axis]
            }
        };
        while self.walkable(cell) {
            if cell == last {
                return true;
            }
            let (along_x, along_z) = (crossing(cell[0], 0), crossing(cell[1], 1));
            if along_x <= along_z {
                cell[0] += step[0];
            }
            if along_z <= along_x {
                cell[1] += step[1];
                // Exactly through a corner: a walker may end up on either side.
                if along_x == along_z
                    && !(self.walkable([cell[0] - step[0], cell[1]])
                        && self.walkable([cell[0], cell[1] - step[1]]))
                {
                    return false;
                }
            }
        }
        false
    }

    /// Every cell a place can be reached from, each with the step towards it. Searched
    /// once per place, so that a walker setting off costs nothing to speak of.
    fn signposts(&self, root: [i32; 2]) -> Signposts {
        let mut steps = vec![NO_STEP; self.pieces.len()];
        let Some(at) = index_of(root).filter(|_| self.walkable(root)) else {
            return Signposts(steps);
        };
        steps[at] = HERE;
        let mut queue = VecDeque::from([root]);
        while let Some(cell) = queue.pop_front() {
            for next in self.steps_from(cell) {
                let Some(slot) = index_of(next).map(|index| &mut steps[index]) else {
                    continue;
                };
                if *slot == NO_STEP {
                    let back = [cell[0] - next[0], cell[1] - next[1]];
                    *slot = AROUND.iter().position(|step| *step == back).unwrap_or(0) as u8;
                    queue.push_back(next);
                }
            }
        }
        Signposts(steps)
    }

    /// The turns of a way between two spots near the same signposts, the last one being
    /// `to`, in lines that stay on the ground. `None` if no way leads there.
    fn route(&self, posts: &Signposts, from: [f64; 2], to: [f64; 2]) -> Option<Vec<[f64; 2]>> {
        let mut going = posts.towards_root(cell_at(from))?;
        let mut coming = posts.towards_root(cell_at(to))?;
        // Where both meet is as far as either has to go.
        let mut meeting = None;
        while going.last().is_some() && going.last() == coming.last() {
            meeting = going.pop();
            coming.pop();
        }
        coming.reverse();
        let cells = going.into_iter().chain(meeting).chain(coming);
        let path: Vec<_> = std::iter::once(from)
            .chain(cells.map(middle_of))
            .chain([to])
            .collect();
        Some(self.straightened(&path))
    }

    /// Drops the turns a straight line can do without, and the start of the way.
    fn straightened(&self, path: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let mut turns = Vec::new();
        let mut at = 0;
        while at + 1 < path.len() {
            let here = path[at];
            at = (at + 2..path.len())
                .take_while(|far| self.clear(here, path[*far]))
                .last()
                .unwrap_or(at + 1);
            turns.push(path[at]);
        }
        turns
    }

    /// The cell closest to a place on the largest piece of ground around it.
    fn anchor(&self, [x, z]: [f64; 2]) -> Option<[i32; 2]> {
        let reach = STOP_AROUND.end as i32 / WAY_GRID;
        let [cell_x, cell_z] = cell_at([x, z]);
        (-reach..=reach)
            .flat_map(|dz| (-reach..=reach).map(move |dx| [dx, dz]))
            .filter(|[dx, dz]| self.walkable([cell_x + dx, cell_z + dz]))
            .max_by_key(|[dx, dz]| {
                let size = self.sizes[self.piece([cell_x + dx, cell_z + dz]) as usize];
                (size, -(dx * dx + dz * dz))
            })
            .map(|[dx, dz]| [cell_x + dx, cell_z + dz])
    }
}

/// Walkers going from place to place, on foot, on walkable land and bridges.
#[derive(Debug, Clone)]
pub struct Walk {
    ground: Ground,
    places: Vec<Place>,
    walkers: Vec<Walker>,
    dice: Dice,
}

fn yaw_towards(dx: f64, dz: f64) -> f32 {
    (-dx).atan2(dz).to_degrees() as f32
}

fn turned(from: f32, to: f32) -> f32 {
    let off = (to - from + 180.0).rem_euclid(360.0) - 180.0;
    from + off.clamp(-TURN_SPEED, TURN_SPEED)
}

fn between(from: [f64; 2], to: [f64; 2], along: f64) -> [f64; 2] {
    [
        from[0] + (to[0] - from[0]) * along,
        from[1] + (to[1] - from[1]) * along,
    ]
}

/// Spots to stop at around a place, all on the piece of ground of `anchor`.
fn stops_around(
    ground: &Ground,
    anchor: [i32; 2],
    [x, z]: [f64; 2],
    dice: &mut Dice,
) -> Vec<[f64; 2]> {
    let piece = ground.piece(anchor);
    let mut stops: Vec<_> = (0..STOP_TRIES)
        .map(|_| {
            let angle = dice.share() * std::f64::consts::TAU;
            let around = dice.within(STOP_AROUND) as f64;
            [x + angle.cos() * around, z + angle.sin() * around]
        })
        .filter(|stop| ground.piece(cell_at(*stop)) == piece)
        .collect();
    if stops.is_empty() {
        stops.push(middle_of(anchor));
    }
    stops
}

/// Where a walker standing at a place goes next, and how. The way is empty when there is
/// nowhere to go.
fn next_stop(
    ground: &Ground,
    places: &[Place],
    dice: &mut Dice,
    from: usize,
    here: [f64; 2],
) -> (usize, Vec<[f64; 2]>) {
    let to = pick(&places[from].ways, dice).copied().unwrap_or(from);
    let place = &places[to];
    let way = pick(&place.stops, dice).and_then(|stop| ground.route(&place.posts, here, *stop));
    (to, way.unwrap_or_default())
}

fn pick<'a, T>(from: &'a [T], dice: &mut Dice) -> Option<&'a T> {
    from.get(dice.next() as usize % from.len().max(1))
}

impl Walk {
    /// Puts `count` walkers on the ways between places, and in `characters`.
    pub fn start(
        count: usize,
        places: &[[i32; 2]],
        walkable: impl Fn(i32, i32) -> bool,
        surface: impl Fn(i32, i32) -> i32,
        characters: &mut Characters,
    ) -> Self {
        let ground = Ground::new(&walkable);
        let mut dice = Dice(SEED);
        let middles: Vec<[f64; 2]> = places
            .iter()
            .map(|[x, z]| [f64::from(*x), f64::from(*z)])
            .collect();
        let anchors: Vec<_> = middles.iter().map(|at| ground.anchor(*at)).collect();
        let piece_of = |place: usize| anchors[place].map(|cell| ground.piece(cell));
        let places: Vec<Place> = (0..middles.len())
            .map(|from| Place {
                stops: anchors[from].map_or_else(Vec::new, |anchor| {
                    stops_around(&ground, anchor, middles[from], &mut dice)
                }),
                ways: (0..middles.len())
                    .filter(|to| *to != from)
                    .filter(|to| piece_of(from).is_some() && piece_of(from) == piece_of(*to))
                    .collect(),
                posts: anchors[from].map_or(Signposts(Vec::new()), |at| ground.signposts(at)),
            })
            .collect();
        let homes: Vec<usize> = (0..places.len())
            .filter(|place| !places[*place].stops.is_empty())
            .collect();
        let mut walkers = Vec::with_capacity(count);
        for index in (0..count).take_while(|_| !homes.is_empty()) {
            let id = u128::from(SEED) << 32 | index as u128;
            let from = homes[index % homes.len()];
            let Some(&stop) = pick(&places[from].stops, &mut dice) else {
                continue;
            };
            let (place, path) = next_stop(&ground, &places, &mut dice, from, stop);
            // Somewhere on the way already, so that the map is not empty for a minute.
            let leg = dice.next() as usize % path.len().max(1);
            let leg_start = leg.checked_sub(1).map_or(stop, |turn| path[turn]);
            let leg_end = path.get(leg).copied().unwrap_or(stop);
            let [x, z] = between(leg_start, leg_end, dice.share());
            let block = [x.floor() as i32, z.floor() as i32];
            let name = match index / NAMES.len() {
                0 => NAMES[index].to_string(),
                round => format!("{}{}", NAMES[index % NAMES.len()], round + 1),
            };
            characters.0.insert(
                id,
                Character {
                    name,
                    skin: default_skin(index),
                    position: Position {
                        x,
                        y: f64::from(surface(block[0], block[1]) + 1),
                        z,
                    },
                    yaw: yaw_towards(leg_end[0] - x, leg_end[1] - z),
                },
            );
            let resting = if path.is_empty() {
                dice.within(REST_TICKS)
            } else {
                0
            };
            walkers.push(Walker {
                id,
                place,
                left: (from, stop),
                path,
                leg,
                resting,
                block,
            });
        }
        Self {
            ground,
            places,
            walkers,
            dice,
        }
    }

    /// A tick of everyone's walk.
    pub fn step(&mut self, characters: &mut Characters, surface: impl Fn(i32, i32) -> i32) {
        let Self {
            ground,
            places,
            walkers,
            dice,
        } = self;
        for walker in walkers {
            let Some(character) = characters.0.get_mut(&walker.id) else {
                continue;
            };
            let here = [character.position.x, character.position.z];
            if walker.resting > 0 {
                walker.resting -= 1;
                if walker.resting == 0 {
                    walker.left = (walker.place, here);
                    (walker.place, walker.path) =
                        next_stop(ground, places, dice, walker.place, here);
                    walker.leg = 0;
                    if walker.path.is_empty() {
                        walker.resting = dice.within(REST_TICKS);
                    }
                }
                continue;
            }
            let mut left = WALK_SPEED;
            while left > 0.0 && walker.resting == 0 {
                let [x, z] = walker.path[walker.leg];
                let (dx, dz) = (x - character.position.x, z - character.position.z);
                let length = dx.hypot(dz);
                if length > left {
                    let next = [
                        character.position.x + dx / length * left,
                        character.position.z + dz / length * left,
                    ];
                    if !ground.holds(next) {
                        walker.turn_back(ground, places, dice, here);
                        break;
                    }
                    character.position.x = next[0];
                    character.position.z = next[1];
                    character.yaw = turned(character.yaw, yaw_towards(dx, dz));
                    break;
                }
                character.position.x = x;
                character.position.z = z;
                left -= length;
                walker.leg += 1;
                if walker.leg == walker.path.len() {
                    walker.resting = dice.within(REST_TICKS);
                }
            }
            let block = [
                character.position.x.floor() as i32,
                character.position.z.floor() as i32,
            ];
            if block != walker.block {
                walker.block = block;
                character.position.y = f64::from(surface(block[0], block[1]) + 1);
            }
        }
    }
}

impl Walker {
    /// Stops short of what cannot be walked on, and goes back the way they came.
    fn turn_back(&mut self, ground: &Ground, places: &[Place], dice: &mut Dice, here: [f64; 2]) {
        let (place, stop) = self.left;
        self.place = place;
        let way = ground.route(&places[place].posts, here, stop);
        self.path = way.unwrap_or_default();
        self.leg = 0;
        if self.path.is_empty() {
            self.resting = dice.within(REST_TICKS);
        }
    }
}

/// The walkers, once the map knows its places.
#[derive(Resource, Debug)]
struct Simulation {
    count: usize,
    walk: Option<Walk>,
}

/// Where a place of the world stands on a model, in blocks from its centre: on ground a
/// figure may be on, or nowhere.
fn place_on(scene: &Scene, position: &Position) -> Option<[f64; 3]> {
    let (block_x, block_z) = (position.x.floor() as i32, position.z.floor() as i32);
    let (x, z) = scene.cell_of(block_x, block_z)?;
    let scale = scene.scale();
    let per_cell = f64::from(scale.blocks_per_cell);
    let col = u16::try_from(x + scale.radius).ok()?;
    let row = u16::try_from(z + scale.radius).ok()?;
    let (corner_x, corner_z) = scale.window().block_at(col, row);
    let model_x = f64::from(x) + (position.x - f64::from(corner_x)) / per_cell;
    let model_z = f64::from(z) + (position.z - f64::from(corner_z)) / per_cell;
    let (stand_x, stand_z) = scene.stand_near(x, z)?;
    let inside = |at: f64, cell: i32| at.clamp(f64::from(cell), f64::from(cell) + CELL_INSIDE);
    Some([
        inside(model_x, stand_x),
        f64::from(scene.top(stand_x, stand_z)? + 1),
        inside(model_z, stand_z),
    ])
}

/// The height a figure moving from `from` to `at` in a tick has to be at: where it stands,
/// or where it will be by the time it has climbed a terrace, if that is higher. So a
/// terrace never goes through its legs, and it does not float long before reaching it.
fn height_ahead(scene: &Scene, from: [f64; 3], at: [f64; 3]) -> f64 {
    let (dx, dz) = (at[0] - from[0], at[2] - from[2]);
    let climb_ticks = f64::from(scene.scale().rise) / CLIMB_SPEED + 1.0;
    let moved = dx.hypot(dz);
    if moved == 0.0 {
        return at[1];
    }
    let ahead = (moved * climb_ticks).min(STEP_AHEAD);
    let (x, z) = (
        (at[0] + dx / moved * ahead).floor() as i32,
        (at[2] + dz / moved * ahead).floor() as i32,
    );
    let free = scene.stand_near(x, z).filter(|cell| *cell == (x, z));
    let top = free.and_then(|(x, z)| scene.top(x, z));
    top.map_or(at[1], |top| at[1].max(f64::from(top + 1)))
}

/// The size of a figure on a model. The same person is the same share of the land on all
/// of them, so a figure follows the zoom as a block does, and a model taking over from
/// another shows it where it was, as large as it was.
fn size_on(scale: Scale) -> f64 {
    LARGER_THAN_LIFE / f64::from(scale.blocks_per_cell)
}

/// One of the skins of the client, so that no account and no download is involved.
fn default_skin(index: usize) -> ResolvableProfile {
    ResolvableProfile {
        id: Some(Uuid::from_u128(index as u128 % DEFAULT_SKINS)),
        ..ResolvableProfile::default()
    }
}

/// What the client hashes an id into, as `java.util.UUID` does.
fn java_hash(id: Uuid) -> u32 {
    let (high, low) = id.as_u64_pair();
    let both = high ^ low;
    (both >> 32) as u32 ^ both as u32
}

/// The id of a character's figure: never that of an account, which a client would take
/// for the player it stands for, but with its hash, and so with its default skin. The hash
/// settles 32 of its bits, so the character has to show in all the others.
fn figure_id(character: u128, skin: &ResolvableProfile) -> Uuid {
    let mut dice = Dice((character >> 64) as u64 ^ character as u64 ^ FIGURE_IDS);
    let id = Uuid::from_u64_pair(dice.next(), dice.next());
    let wanted = skin.id.map_or(0, java_hash);
    Uuid::from_u128(id.as_u128() ^ u128::from(java_hash(id) ^ wanted))
}

/// What a client has to know of a figure before it can show one. It is not listed, and an
/// empty name leaves nothing over its head.
fn figure_entry(id: Uuid, character: &Character) -> PlayerInfoEntry {
    PlayerInfoEntry {
        uuid: id,
        properties: character.skin.properties.clone(),
        ..PlayerInfoEntry::default()
    }
}

/// How far from the middle of the screen a figure can be named: about its body, which
/// takes more of the screen the closer it is.
fn reach_for(size: f64, distance: f64) -> AimReach {
    let body = (PLAYER_HEIGHT * size / 2.0 / distance.max(1.0))
        .atan()
        .to_degrees();
    let enter = FIGURE_REACH.enter.max(body);
    AimReach {
        enter,
        leave: enter * FIGURE_REACH.leave / FIGURE_REACH.enter,
        switch: FIGURE_REACH.switch,
    }
}

fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn climbed(from: f64, to: f64) -> f64 {
    from + (to - from).clamp(-CLIMB_SPEED, CLIMB_SPEED)
}

/// A character on the model of one session.
#[derive(Debug, Clone, Copy)]
struct Figure {
    entity: Entity,
    /// On the model, from its centre.
    at: [f64; 3],
    size: f64,
}

impl Figure {
    /// So far up its body, from 0 at its feet to 1 at the top of its head.
    fn up(&self, share: f64) -> [f64; 3] {
        let [x, y, z] = self.at;
        [x, y + PLAYER_HEIGHT * self.size * share, z]
    }
}

/// The character the look is on, and their name over them.
#[derive(Debug, Clone, Copy)]
struct Label {
    id: u128,
    entity: Entity,
}

/// What a session shows of the characters.
#[derive(Component, Debug, Default)]
struct Figures {
    /// What the figures are placed for: another model moves them all.
    model: Option<Scale>,
    shown: BTreeMap<u128, Figure>,
    label: Option<Label>,
    /// The figures the client was told about, which outlive what it shows.
    introduced: BTreeSet<Uuid>,
}

impl Figures {
    fn clear(&mut self, commands: &mut Commands) {
        let figures = std::mem::take(&mut self.shown);
        let label = self.drop_label();
        for entity in figures.values().map(|f| f.entity).chain(label) {
            commands.entity(entity).try_despawn();
        }
    }

    fn drop_label(&mut self) -> Option<Entity> {
        self.label.take().map(|label| label.entity)
    }

    /// The id of a character's figure, once the client of a session knows it.
    fn introduce(
        &mut self,
        id: u128,
        character: &Character,
        player: Entity,
        players: &Players,
    ) -> Uuid {
        let figure = figure_id(id, &character.skin);
        if self.introduced.insert(figure) {
            let entry = figure_entry(figure, character);
            players.send(
                player,
                PlayerInfoUpdate::single(PlayerInfoActions::ADD_PLAYER, entry),
            );
        }
        figure
    }
}

fn spawn_figure(
    commands: &mut Commands,
    at: Position,
    id: Uuid,
    character: &Character,
    size: f64,
) -> Entity {
    EntityBuilder::new(EntityKind::Player)
        .position(at)
        .rotation(character.yaw, 0.0)
        .gravity(false)
        .block_collision(false)
        .settle_ticks(0)
        .with_bundle((
            EntityUuid(id),
            AllSkinLayers,
            Attributes::for_kind(EntityKind::Player).base(EntityAttribute::Scale, size),
            NoGravity,
            Silent,
        ))
        .spawn(commands)
        .id()
}

fn spawn_label(commands: &mut Commands, at: Position, name: &str, scale: f32) -> Entity {
    EntityBuilder::new(EntityKind::TextDisplay)
        .position(at)
        .with_bundle((
            Display::default()
                .transform(DisplayTransform::default().uniform_scale(scale))
                .billboard(Billboard::Center)
                .brightness(15, 15)
                .view_range(LABEL_VIEW_RANGE)
                .teleport_ticks(GLIDE_TICKS),
            TextDisplay::new(name).background_color(LABEL_BACKGROUND),
        ))
        .spawn(commands)
        .id()
}

/// What the characters cost, between two summaries.
#[derive(Resource, Debug, Default)]
struct Cost {
    ticks: u32,
    moves: u32,
    spent: Duration,
    longest: Duration,
}

impl Cost {
    fn add(&mut self, took: Duration) {
        self.spent += took;
        self.longest = self.longest.max(took);
    }
}

fn walk(
    mut simulation: ResMut<Simulation>,
    mut characters: ResMut<Characters>,
    atlas: Res<Atlas>,
    generator: Res<WorldGen>,
    mut cost: ResMut<Cost>,
) {
    let started = Instant::now();
    let generator = generator.0.as_ref();
    let surface = |x, z| generator.surface_height_at(x, z);
    let count = simulation.count;
    match &mut simulation.walk {
        Some(walk) => walk.step(&mut characters, surface),
        walk => {
            let Some(places) = atlas.places() else {
                return;
            };
            let walkable = |x, z| atlas.walkable(x, z);
            let started = Walk::start(count, &places, walkable, surface, &mut characters);
            let ways = started.places.iter().map(|p| p.ways.len()).sum::<usize>() / 2;
            tracing::info!(walkers = count, places = places.len(), ways, "map walkers");
            *walk = Some(started);
        }
    }
    cost.add(started.elapsed());
}

fn world_spot(view: &MapView, [x, y, z]: [f64; 3]) -> Position {
    Position {
        x: view.origin[0] + x,
        y,
        z: view.origin[1] + z,
    }
}

type Bodies<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Position,
        &'static mut Rotation,
        Option<&'static mut Attributes>,
    ),
>;

/// Keeps every session's figures on the characters.
fn show(
    characters: Res<Characters>,
    atlas: Res<Atlas>,
    mut sessions: Query<(Entity, &MapSession, &mut Figures)>,
    mut bodies: Bodies,
    mut cost: ResMut<Cost>,
    players: Players,
    mut commands: Commands,
) {
    let started = Instant::now();
    for (player, session, mut figures) in &mut sessions {
        let Some(view) = session.view(&atlas) else {
            continue;
        };
        let figures = &mut *figures;
        let model = view.scene.scale();
        let size = size_on(model);
        let moved_model = figures.model.replace(model) != Some(model);
        if moved_model && let Some(label) = figures.drop_label() {
            commands.entity(label).try_despawn();
        }

        let places: BTreeMap<u128, [f64; 3]> = characters
            .0
            .iter()
            .filter_map(|(id, c)| Some((*id, place_on(view.scene, &c.position)?)))
            .collect();
        figures.shown.retain(|id, figure| {
            let stays = places.contains_key(id);
            if !stays {
                commands.entity(figure.entity).try_despawn();
            }
            stays
        });

        for (id, [x, y, z]) in &places {
            let character = &characters.0[id];
            let Some(figure) = figures.shown.get_mut(id) else {
                let at = [*x, *y, *z];
                let spot = world_spot(&view, at);
                let figure = figures.introduce(*id, character, player, &players);
                let entity = spawn_figure(&mut commands, spot, figure, character, size);
                figures.shown.insert(*id, Figure { entity, at, size });
                continue;
            };
            figure.at = if moved_model {
                [*x, *y, *z]
            } else {
                let wanted_y = height_ahead(view.scene, figure.at, [*x, *y, *z]);
                [*x, climbed(figure.at[1], wanted_y), *z]
            };
            if let Ok((mut position, mut rotation, _)) = bodies.get_mut(figure.entity) {
                if position.set_if_neq(world_spot(&view, figure.at)) {
                    cost.moves += 1;
                }
                rotation.set_if_neq(Rotation {
                    yaw: character.yaw,
                    pitch: 0.0,
                });
            }
            if figure.size != size
                && let Ok((_, _, Some(mut attributes))) = bodies.get_mut(figure.entity)
            {
                figure.size = size;
                attributes.set_base(EntityAttribute::Scale, size);
            }
        }

        hover(&view, figures, &characters, &mut bodies, &mut commands);
    }
    cost.add(started.elapsed());
    cost.ticks += 1;
    if cost.ticks >= SUMMARY_PERIOD {
        let cost = std::mem::take(&mut *cost);
        tracing::info!(
            characters = characters.0.len(),
            sessions = sessions.iter().len(),
            mean_us = cost.spent.as_micros() as u64 / u64::from(cost.ticks),
            longest_us = cost.longest.as_micros() as u64,
            moves_per_tick = cost.moves / cost.ticks,
            "map characters"
        );
    }
}

/// Names the character the look is on, if no marker has it.
fn hover(
    view: &MapView,
    figures: &mut Figures,
    characters: &Characters,
    bodies: &mut Bodies,
    commands: &mut Commands,
) {
    let ids: Vec<u128> = figures.shown.keys().copied().collect();
    let current = figures
        .label
        .and_then(|label| ids.iter().position(|id| *id == label.id));
    let reach = reach_for(size_on(view.scene.scale()), view.eye[1] - f64::from(SEA_Y));
    let picked = view.aim.and_then(|aim| {
        let points = figures.shown.values().map(|figure| figure.up(0.5));
        aimed_within(reach, view.eye, aim, points, current).map(|index| ids[index])
    });
    let over = |figure: &Figure| world_spot(view, figure.up(1.0 + LABEL_GAP));

    if picked != figures.label.map(|label| label.id) {
        if let Some(label) = figures.label.take() {
            commands.entity(label.entity).try_despawn();
            if let Some(figure) = figures.shown.get(&label.id) {
                commands.entity(figure.entity).try_remove::<Glowing>();
            }
        }
        if let Some((id, figure)) = picked.and_then(|id| Some((id, figures.shown.get(&id)?))) {
            let name = characters.0.get(&id).map_or("", |c| c.name.as_str());
            let scale = (apart(figure.at, view.eye) * LABEL_SCALE_PER_BLOCK) as f32;
            let entity = spawn_label(commands, over(figure), name, scale);
            commands.entity(figure.entity).try_insert(Glowing);
            tracing::info!(name, "map character hover");
            figures.label = Some(Label { id, entity });
        }
    } else if let Some(label) = figures.label
        && let Some(figure) = figures.shown.get(&label.id)
        && let Ok((mut position, ..)) = bodies.get_mut(label.entity)
    {
        position.set_if_neq(over(figure));
    }
}

fn give_figures(
    sessions: Query<Entity, (With<MapSession>, Without<Figures>)>,
    mut commands: Commands,
) {
    for player in &sessions {
        commands.entity(player).insert(Figures::default());
    }
}

/// On leaving the map, and on leaving the server, which takes the player away too.
fn take_figures(
    event: On<bevy_ecs::lifecycle::Remove, MapSession>,
    mut figures: Query<&mut Figures>,
    players: Players,
    mut commands: Commands,
) {
    if let Ok(mut figures) = figures.get_mut(event.entity) {
        figures.clear(&mut commands);
        let uuids = std::mem::take(&mut figures.introduced)
            .into_iter()
            .collect();
        players.send(event.entity, PlayerInfoRemove { uuids });
        commands.entity(event.entity).try_remove::<Figures>();
    }
}

pub struct PlayersPlugin;

impl Plugin for PlayersPlugin {
    fn build(&self, app: &mut App) {
        let count = std::env::var("VOID_MAP_WALKERS")
            .ok()
            .and_then(|count| count.parse().ok())
            .unwrap_or(WALKERS);
        app.insert_resource(Simulation { count, walk: None })
            .init_resource::<Characters>()
            .init_resource::<Cost>()
            .add_systems(Update, (give_figures, walk, show).chain())
            .add_observer(take_figures);
    }
}

#[cfg(test)]
mod tests {
    use voidmc::BiomeId;

    use super::*;
    use crate::plugin::SEA_LEVEL;
    use crate::pointer::{COARSE_ABOVE, COARSEST_ABOVE, FARTHEST_ZOOM};
    use crate::regions::Regions;
    use crate::scene::{DETAIL, MIDDLE, OVERVIEW};
    use crate::terrain::Terrain;
    use crate::testing::Flat;

    fn walkers(count: usize) -> (Walk, Characters) {
        let mut characters = Characters::default();
        let places = [[0, 0], [200, 0], [0, 200]];
        let walk = Walk::start(count, &places, |_, _| true, |_, _| 70, &mut characters);
        (walk, characters)
    }

    fn positions(characters: &Characters) -> Vec<Position> {
        characters.0.values().map(|c| c.position).collect()
    }

    #[test]
    fn walkers_go_at_a_walking_pace() {
        let (mut walk, mut characters) = walkers(20);
        let before = positions(&characters);
        walk.step(&mut characters, |_, _| 70);
        for (from, to) in before.iter().zip(positions(&characters)) {
            let step = (to.x - from.x).hypot(to.z - from.z);
            assert!((step - WALK_SPEED).abs() < 1e-9, "{step}");
        }
    }

    #[test]
    fn walkers_stand_on_the_ground_under_them() {
        let (mut walk, mut characters) = walkers(20);
        for _ in 0..50 {
            walk.step(&mut characters, |x, _| 60 + x.rem_euclid(5));
        }
        for character in characters.0.values() {
            let x = character.position.x.floor() as i32;
            assert_eq!(character.position.y, f64::from(61 + x.rem_euclid(5)));
        }
    }

    #[test]
    fn walkers_arrive_rest_and_leave_again() {
        let (mut walk, mut characters) = walkers(1);
        let first = walk.walkers[0].place;
        let mut rested = false;
        for _ in 0..8000 {
            walk.step(&mut characters, |_, _| 70);
            rested |= walk.walkers[0].resting > 0;
            if rested && walk.walkers[0].place != first {
                return;
            }
        }
        panic!("the walker stayed at {first}, rested: {rested}");
    }

    #[test]
    fn walkers_face_where_they_go() {
        let (mut walk, mut characters) = walkers(1);
        for _ in 0..20 {
            walk.step(&mut characters, |_, _| 70);
        }
        let walker = &walk.walkers[0];
        let character = characters.0.values().next().unwrap();
        let [x, z] = walker.path[walker.leg];
        let (dx, dz) = (x - character.position.x, z - character.position.z);
        let off = (yaw_towards(dx, dz) - character.yaw + 180.0).rem_euclid(360.0) - 180.0;
        assert!(off.abs() < 1.0, "{off}");
    }

    #[test]
    fn a_turn_takes_the_short_way_at_its_own_pace() {
        assert_eq!(turned(170.0, -170.0), 170.0 + 20.0);
        assert_eq!(turned(0.0, 90.0), TURN_SPEED);
    }

    #[test]
    fn no_way_crosses_the_water() {
        let mut characters = Characters::default();
        let places = [[0, 0], [100, 0], [0, 100]];
        let land = |x: i32, _: i32| !(40..60).contains(&x);
        let walk = Walk::start(4, &places, land, |_, _| 70, &mut characters);
        assert_eq!(walk.places[0].ways, [2]);
        assert_eq!(walk.places[1].ways, [] as [usize; 0]);
        assert_eq!(walk.places[2].ways, [0]);
    }

    #[test]
    fn a_way_goes_around_a_lake_in_a_few_straight_lines() {
        let lake = |x: i32, z: i32| (x - 60).abs() < 20 && z.abs() < 80;
        let ground = Ground::new(&|x, z| !lake(x, z));
        let posts = ground.signposts(cell_at([120.0, 0.0]));
        let way = ground.route(&posts, [0.0, 0.0], [120.0, 0.0]).unwrap();
        assert!(way.len() <= 6, "{way:?}");
        let mut from = [0.0, 0.0];
        for turn in way {
            assert!(ground.clear(from, turn), "{from:?} {turn:?}");
            from = turn;
        }
    }

    #[test]
    fn a_line_is_clear_only_if_it_touches_nothing_else() {
        let ground = Ground::new(&|x, z| !(x == 30 && z == 30));
        assert!(ground.clear([0.5, 0.5], [60.5, 0.5]));
        assert!(!ground.clear([0.5, 30.5], [60.5, 31.5]));
        // Through the very corner of the hole.
        assert!(!ground.clear([27.0, 27.0], [36.0, 36.0]));
        // By it, and as close as a fine look along the line can tell.
        let (from, to) = ([20.0, 45.0], [45.0, 20.1]);
        let fine = (0..=10_000).all(|step| ground.holds(between(from, to, f64::from(step) / 1e4)));
        assert_eq!(ground.clear(from, to), fine);
    }

    #[test]
    fn a_way_between_two_spots_does_not_go_by_the_place_first() {
        let ground = Ground::new(&|_, _| true);
        let posts = ground.signposts(cell_at([0.0, 0.0]));
        let way = ground
            .route(&posts, [100.5, 100.5], [100.5, 130.5])
            .unwrap();
        assert_eq!(way.len(), 1, "{way:?}");
    }

    #[test]
    fn a_diagonal_does_not_squeeze_between_two_corners() {
        let open = |x: i32, z: i32| (x < 0 && z < 0) || (x >= 0 && z >= 0);
        let ground = Ground::new(&open);
        let posts = ground.signposts(cell_at([3.0, 3.0]));
        assert_eq!(ground.route(&posts, [-3.0, -3.0], [3.0, 3.0]), None);
    }

    #[test]
    fn a_bridge_is_a_way_across() {
        let land = |x: i32, z: i32| !(40..60).contains(&x) || (-3..3).contains(&z);
        let mut characters = Characters::default();
        let walk = Walk::start(1, &[[0, 30], [100, 30]], land, |_, _| 70, &mut characters);
        assert_eq!(walk.places[0].ways, [1]);
    }

    #[test]
    fn a_place_with_no_ground_gets_nobody() {
        let mut characters = Characters::default();
        let walk = Walk::start(3, &[[0, 0]], |_, _| false, |_, _| 70, &mut characters);
        assert!(walk.walkers.is_empty());
        assert!(characters.0.is_empty());
    }

    #[test]
    fn a_place_nothing_leads_to_keeps_its_walkers_around_it() {
        let land = |x: i32, z: i32| x.abs() < 60 && z.abs() < 60;
        let mut characters = Characters::default();
        let mut walk = Walk::start(1, &[[0, 0]], land, |_, _| 70, &mut characters);
        for _ in 0..4000 {
            walk.step(&mut characters, |_, _| 70);
        }
        let at = characters.0.values().next().unwrap().position;
        assert!(at.x.hypot(at.z) <= STOP_AROUND.end as f64);
    }

    #[test]
    fn walkers_only_stop_where_they_can_walk() {
        let land = |x: i32, _: i32| x >= 0;
        let mut characters = Characters::default();
        let walk = Walk::start(1, &[[0, 0]], land, |_, _| 70, &mut characters);
        assert!(!walk.places[0].stops.is_empty());
        assert!(walk.places[0].stops.iter().all(|stop| stop[0] >= 0.0));
    }

    #[test]
    fn a_walker_turns_back_before_an_edge() {
        let (mut walk, mut characters) = walkers(1);
        // The ground ends across the way it was given.
        let hole = index_of(cell_at([120.0, 0.0])).unwrap();
        let character = characters.0.values_mut().next().unwrap();
        character.position = Position {
            x: 100.0,
            y: 70.0,
            z: 0.5,
        };
        let walker = &mut walk.walkers[0];
        walker.left = (0, [90.0, 0.5]);
        walker.path = vec![[150.0, 0.5]];
        walker.leg = 0;
        walker.resting = 0;
        for cell in (-GROUND_REACH..=GROUND_REACH).map(|z| hole as i32 + z * GROUND_SIDE) {
            walk.ground.pieces[cell as usize] = 0;
        }
        let mut furthest: f64 = 0.0;
        for _ in 0..300 {
            walk.step(&mut characters, |_, _| 70);
            let at = characters.0.values().next().unwrap().position;
            assert!(walk.ground.holds([at.x, at.z]));
            furthest = furthest.max(at.x);
        }
        let at = characters.0.values().next().unwrap().position;
        assert!(furthest > 115.0, "{furthest}");
        assert!(at.x < furthest - 5.0, "{} {furthest}", at.x);
    }

    #[test]
    fn walkers_are_the_same_from_one_start_to_the_next() {
        let (mut one, mut first) = walkers(5);
        let (mut other, mut second) = walkers(5);
        for _ in 0..500 {
            one.step(&mut first, |_, _| 70);
            other.step(&mut second, |_, _| 70);
        }
        assert_eq!(positions(&first), positions(&second));
    }

    #[test]
    fn more_walkers_than_names_stay_told_apart() {
        let (_, characters) = walkers(25);
        let mut names: Vec<_> = characters.0.values().map(|c| &c.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 25);
        assert!(names.iter().all(|name| name.len() <= 16));
    }

    fn scene(scale: Scale) -> Scene {
        let generator = Flat::new(90, BiomeId::plains());
        let terrain = Terrain::sample(&generator, &scale.window());
        Scene::new(&terrain, SEA_LEVEL, scale, &Regions::demo())
    }

    fn at(x: f64, z: f64) -> Position {
        Position { x, y: 70.0, z }
    }

    #[test]
    fn a_step_in_the_world_is_a_share_of_a_block_on_the_model() {
        for scale in [OVERVIEW, DETAIL] {
            let scene = scene(scale);
            let per_cell = f64::from(scale.blocks_per_cell);
            // Across a cell boundary too: the place must not jump from one to the other.
            for start in [-40.0, 0.0, 17.5] {
                let from = place_on(&scene, &at(start, 3.0)).unwrap();
                let to = place_on(&scene, &at(start + 5.0, 3.0)).unwrap();
                assert!(((to[0] - from[0]) - 5.0 / per_cell).abs() < 1e-9);
                assert_eq!(from[2], to[2]);
            }
        }
    }

    #[test]
    fn the_model_stands_for_the_world_around_the_origin() {
        let scene = scene(OVERVIEW);
        let [x, _, z] = place_on(&scene, &at(80.0, -160.0)).unwrap();
        assert!((x - 10.0).abs() <= 1.0, "{x}");
        assert!((z + 20.0).abs() <= 1.0, "{z}");
    }

    #[test]
    fn a_figure_stands_on_the_top_of_its_column() {
        let scene = scene(OVERVIEW);
        let [x, y, z] = place_on(&scene, &at(8.0, 8.0)).unwrap();
        let top = scene.top(x.floor() as i32, z.floor() as i32).unwrap();
        assert_eq!(y, f64::from(top + 1));
    }

    /// The map as it is shown: real terrain, paths and landmarks, on every model.
    fn real_scenes() -> Vec<Scene> {
        let generator = voidmc::DefaultWorldGenerator::default();
        [OVERVIEW, MIDDLE, DETAIL]
            .into_iter()
            .map(|scale| crate::plugin::built_scene(&generator, scale, &Regions::demo()))
            .collect()
    }

    #[test]
    fn on_the_real_map_walkers_keep_to_what_can_be_walked_on_every_model() {
        let scenes = real_scenes();
        let detail = &scenes[2];
        let walkable = |x: i32, z: i32| {
            detail
                .cell_of(x, z)
                .is_some_and(|(x, z)| detail.walkable(x, z))
        };
        let places = crate::plugin::marker_places(&voidmc::DefaultWorldGenerator::default());
        let mut characters = Characters::default();
        let mut walk = Walk::start(20, &places, walkable, |_, _| 70, &mut characters);
        assert_eq!(walk.walkers.len(), 20);
        assert!(walk.places.iter().any(|place| place.ways.len() >= 4));
        let (mut shown, mut hidden) = ([0u32; 3], [0u32; 3]);
        for _ in 0..6000 {
            walk.step(&mut characters, |_, _| 70);
            for character in characters.0.values() {
                let position = character.position;
                let (block_x, block_z) = (position.x.floor() as i32, position.z.floor() as i32);
                assert!(walkable(block_x, block_z), "{position:?}");
                for (index, scene) in scenes.iter().enumerate() {
                    match place_on(scene, &position) {
                        Some(_) => shown[index] += 1,
                        None => hidden[index] += 1,
                    }
                }
            }
        }
        assert_eq!(hidden[2], 0);
        for index in 0..2 {
            let share = f64::from(hidden[index]) / f64::from(shown[index] + hidden[index]);
            assert!(share < 0.02, "{index}: {share}");
        }
    }

    #[test]
    fn a_moving_figure_is_up_a_terrace_before_it_reaches_it() {
        let scene = scene(OVERVIEW);
        let (x, z) = (-30..30)
            .flat_map(|x| (-30..30).map(move |z| (x, z)))
            .find(|(x, z)| scene.walkable(*x, *z) && scene.top(*x + 1, *z) > scene.top(*x, *z))
            .expect("a step on the model");
        let foot = f64::from(scene.top(x, z).unwrap() + 1);
        let walking = WALK_SPEED / f64::from(OVERVIEW.blocks_per_cell);
        let place = |into_the_cell: f64| [f64::from(x) + into_the_cell, foot, f64::from(z) + 0.5];
        let near = height_ahead(&scene, place(0.9 - walking), place(0.9));
        assert!(near > foot);
        let far = height_ahead(&scene, place(0.1 - walking), place(0.1));
        assert_eq!(far, foot);
        assert_eq!(height_ahead(&scene, place(0.9), place(0.9)), foot);
    }

    #[test]
    fn a_figure_is_named_only_when_the_look_is_about_on_it() {
        let eye = [0.0, 100.0, 0.0];
        let down = [0.0, -1.0, 0.0];
        let off = |degrees: f64| [degrees.to_radians().tan() * 100.0, 0.0, 0.0];
        let picked = |at, current| aimed_within(FIGURE_REACH, eye, down, [at], current);
        assert_eq!(picked(off(1.0), None), Some(0));
        assert_eq!(picked(off(2.0), None), None);
        assert_eq!(picked(off(1.5), Some(0)), Some(0));
        assert_eq!(picked(off(2.0), Some(0)), None);
    }

    #[test]
    fn nobody_shows_off_the_model() {
        assert_eq!(place_on(&scene(OVERVIEW), &at(100_000.0, 0.0)), None);
    }

    #[test]
    fn a_figure_is_the_same_share_of_the_land_on_every_model() {
        let world_size = |scale: Scale| size_on(scale) * f64::from(scale.blocks_per_cell);
        assert_eq!(world_size(DETAIL), world_size(MIDDLE));
        assert_eq!(world_size(DETAIL), world_size(OVERVIEW));
    }

    /// Past a height, a view stands on the next model, as far from it as the heights say.
    #[test]
    fn a_figure_is_drawn_from_as_far_as_each_model_is_seen() {
        for (scale, farthest) in [
            (DETAIL, COARSE_ABOVE),
            (MIDDLE, COARSEST_ABOVE),
            (OVERVIEW, FARTHEST_ZOOM),
        ] {
            let on_model = farthest * f64::from(DETAIL.blocks_per_cell);
            let distance = on_model / f64::from(scale.blocks_per_cell);
            assert!(size_on(scale) * DRAWN_PER_SCALE > distance * 2.0);
        }
    }

    #[test]
    fn a_figure_stays_within_what_the_attribute_allows() {
        for scale in [DETAIL, MIDDLE, OVERVIEW] {
            assert!((0.0625..=16.0).contains(&size_on(scale)));
        }
    }

    #[test]
    fn a_figure_keeps_the_default_skin_of_its_character_but_not_its_id() {
        for index in 0..30 {
            let skin = default_skin(index);
            let id = figure_id(0x1234 + index as u128, &skin);
            assert_eq!(java_hash(id), java_hash(skin.id.unwrap()));
            assert_ne!(Some(id), skin.id);
        }
    }

    #[test]
    fn two_characters_with_the_same_skin_are_two_figures() {
        let skin = default_skin(3);
        assert_ne!(figure_id(3, &skin), figure_id(3 + 18, &skin));
    }

    #[test]
    fn a_figure_close_up_can_be_named_all_along_its_body() {
        assert_eq!(reach_for(0.75, 100.0).enter, FIGURE_REACH.enter);
        let close = reach_for(2.0, 20.0);
        assert!(close.enter > 4.0, "{}", close.enter);
        assert!(close.leave > close.enter);
    }

    #[test]
    fn a_figure_climbs_a_terrace_step_by_step() {
        assert_eq!(climbed(70.0, 73.0), 70.0 + CLIMB_SPEED);
        assert_eq!(climbed(70.0, 70.1), 70.1);
        assert_eq!(climbed(73.0, 70.0), 73.0 - CLIMB_SPEED);
    }
}
