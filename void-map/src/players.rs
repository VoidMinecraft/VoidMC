//! Who walks the world, as small figures on the model.
//!
//! [`Characters`] is the list the map reads: a name, a skin, a place in the real world.
//! It does not say who plays them. [`Simulation`] fills it with walkers for now, and a
//! system that copies connected players into it would show them the same way.
//!
//! A figure is a `minecraft:mannequin`, one per character and per session, since each
//! session has its own model. The client animates it from how it moves, and it stays
//! out of the way of the pointer: the eye sits inside its box, so the look always hits
//! that first. Hovering one is decided here, the same way as for a marker, and only
//! when no marker wants the look.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::{Duration, Instant};

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use voidmc::{
    Attributes, Billboard, Display, DisplayTransform, EntityAttribute, EntityBuilder, EntityKind,
    Glowing, Mannequin, NoGravity, Silent, TextDisplay, WorldGen,
    components::{Position, Rotation},
};
use voidmc_protocol::clientbound::ResolvableProfile;

use crate::plugin::{Atlas, MapSession, MapView};
use crate::pointer::aimed;
use crate::scene::{Scale, Scene, WORLD_RADIUS};

/// Walkers when `VOID_MAP_WALKERS` does not say otherwise.
const WALKERS: usize = 20;
const SEED: u64 = 0x766f_6964_7761_6c6b;

/// A vanilla player's walk, in blocks per tick.
const WALK_SPEED: f64 = 4.317 / 20.0;
/// How far a walker turns in a tick, in degrees.
const TURN_SPEED: f32 = 24.0;
/// How long a walker stays at a place, in ticks.
const REST_TICKS: std::ops::Range<u64> = 40..240;
/// How far from the middle of a place a walker stops, in blocks: past its building, and
/// not all on the same spot.
const STOP_AROUND: std::ops::Range<u64> = 26..48;
/// Spots tried around a place. Those in the water are dropped.
const STOP_TRIES: usize = 24;
/// The grid ways are searched on, in blocks: a cell of the detailed model, whose bridges
/// are the narrowest thing to follow.
const WAY_GRID: i32 = 3;
/// Between two looks at the ground of a way, in blocks.
const LAND_SAMPLE: f64 = 2.0;

/// A figure this large is drawn this far, and no further. Vanilla ties the distance to
/// the size of the box, and at the true scale of a model a figure would vanish 8 to 20
/// blocks from the eye, which is always further. So each figure takes the size its
/// distance calls for, which also keeps them all the same size on screen.
const DRAWN_PER_SCALE: f64 = 64.0;
/// So that a figure moving away stays drawn until the next step of its size. It also
/// sets how large they all look: from above, not much more than a head shows.
const DRAWN_MARGIN: f64 = 1.3;
/// Vanilla's limits for the `scale` attribute.
const SCALE_LIMITS: (f64, f64) = (0.0625, 16.0);
/// A figure follows its distance by steps of this much.
const SIZE_STEP: f64 = 0.05;
const PLAYER_HEIGHT: f64 = 1.8;
/// How fast a figure climbs a terrace of the model, in blocks per tick.
const CLIMB_SPEED: f64 = 0.2;

/// As for `Display::teleport_ticks`: what a living entity glides over on the client.
const GLIDE_TICKS: u8 = 3;
const LABEL_VIEW_RANGE: f32 = 8.0;
const LABEL_BACKGROUND: i32 = 0xC0_10_14_1C_u32 as i32;
/// Keeps a name the same size on screen, from how far away it is seen.
const LABEL_SCALE_PER_BLOCK: f64 = 0.08;
/// Between a head and a name, in heights of the figure.
const LABEL_GAP: f64 = 0.15;

const SUMMARY_PERIOD: u32 = 200;

/// Default skins of the client, so that no account and no download is involved.
const SKINS: [(&str, bool); 18] = [
    ("entity/player/wide/steve", false),
    ("entity/player/slim/alex", true),
    ("entity/player/wide/ari", false),
    ("entity/player/slim/efe", true),
    ("entity/player/wide/kai", false),
    ("entity/player/slim/makena", true),
    ("entity/player/wide/noor", false),
    ("entity/player/slim/sunny", true),
    ("entity/player/wide/zuri", false),
    ("entity/player/slim/steve", true),
    ("entity/player/wide/alex", false),
    ("entity/player/slim/ari", true),
    ("entity/player/wide/efe", false),
    ("entity/player/slim/kai", true),
    ("entity/player/wide/makena", false),
    ("entity/player/slim/noor", true),
    ("entity/player/wide/sunny", false),
    ("entity/player/slim/zuri", true),
];

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
    middle: [f64; 2],
    /// Where a walker can stop, on land.
    stops: Vec<[f64; 2]>,
    ways: Vec<Way>,
}

/// A place that can be walked to, and how, with both ends in the middle of places.
#[derive(Debug, Clone, PartialEq)]
struct Way {
    to: usize,
    turns: Vec<[f64; 2]>,
}

/// Walkers going from place to place, on foot, on dry land and bridges.
#[derive(Debug, Clone)]
pub struct Walk {
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

fn distance(from: [f64; 2], to: [f64; 2]) -> f64 {
    (to[0] - from[0]).hypot(to[1] - from[1])
}

fn dry(from: [f64; 2], to: [f64; 2], land: &impl Fn(i32, i32) -> bool) -> bool {
    let samples = (distance(from, to) / LAND_SAMPLE).ceil().max(1.0) as u32;
    (0..=samples).all(|sample| {
        let [x, z] = between(from, to, f64::from(sample) / f64::from(samples));
        land(x.floor() as i32, z.floor() as i32)
    })
}

/// Every land cell a place can be reached from, each with the next one towards it.
fn flood(from: [i32; 2], land: &impl Fn(i32, i32) -> bool) -> HashMap<[i32; 2], [i32; 2]> {
    let limit = WORLD_RADIUS as i32 / WAY_GRID;
    let mut towards = HashMap::from([(from, from)]);
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        for (dx, dz) in [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            let next = [cell[0] + dx, cell[1] + dz];
            if next[0].abs() <= limit
                && next[1].abs() <= limit
                && !towards.contains_key(&next)
                && land(next[0] * WAY_GRID, next[1] * WAY_GRID)
            {
                towards.insert(next, cell);
                queue.push_back(next);
            }
        }
    }
    towards
}

/// Drops the turns a straight line on dry land can do without.
fn straightened(path: &[[f64; 2]], land: &impl Fn(i32, i32) -> bool) -> Vec<[f64; 2]> {
    let mut turns = Vec::new();
    let mut at = 0;
    while let Some(here) = path.get(at).copied() {
        turns.push(here);
        at = (at + 1..path.len())
            .rev()
            .find(|far| dry(here, path[*far], land))
            .unwrap_or(at + 1);
    }
    turns
}

fn cell_of_place([x, z]: [f64; 2]) -> [i32; 2] {
    let cell = |value: f64| (value / f64::from(WAY_GRID)).round() as i32;
    [cell(x), cell(z)]
}

fn ways_from(from: usize, places: &[[f64; 2]], land: &impl Fn(i32, i32) -> bool) -> Vec<Way> {
    let start = cell_of_place(places[from]);
    let towards = flood(start, land);
    let block = |[x, z]: [i32; 2]| [f64::from(x * WAY_GRID), f64::from(z * WAY_GRID)];
    (0..places.len())
        .filter(|to| *to != from)
        .filter_map(|to| {
            let mut cell = cell_of_place(places[to]);
            if !towards.contains_key(&cell) {
                return None;
            }
            let mut path = vec![block(cell)];
            while cell != start {
                cell = towards[&cell];
                path.push(block(cell));
            }
            path.reverse();
            Some(Way {
                to,
                turns: straightened(&path, land),
            })
        })
        .collect()
}

fn stops_around(
    [x, z]: [f64; 2],
    dice: &mut Dice,
    land: &impl Fn(i32, i32) -> bool,
) -> Vec<[f64; 2]> {
    let stops: Vec<_> = (0..STOP_TRIES)
        .map(|_| {
            let angle = dice.share() * std::f64::consts::TAU;
            let around = dice.within(STOP_AROUND) as f64;
            [x + angle.cos() * around, z + angle.sin() * around]
        })
        .filter(|[x, z]| land(x.floor() as i32, z.floor() as i32))
        .collect();
    if stops.is_empty() {
        vec![[x, z]]
    } else {
        stops
    }
}

/// Where a walker goes from a place, or around it when no way leaves it.
fn next_stop(places: &[Place], dice: &mut Dice, from: usize) -> (usize, Vec<[f64; 2]>) {
    let start = &places[from];
    let Some(way) = pick(&start.ways, dice) else {
        return (
            from,
            pick(&start.stops, dice).into_iter().copied().collect(),
        );
    };
    let end = &places[way.to];
    let clear = STOP_AROUND.end as f64;
    let mut path: Vec<_> = way
        .turns
        .iter()
        .copied()
        .filter(|turn| distance(*turn, start.middle) > clear && distance(*turn, end.middle) > clear)
        .collect();
    path.extend(pick(&end.stops, dice));
    (way.to, path)
}

fn pick<'a, T>(from: &'a [T], dice: &mut Dice) -> Option<&'a T> {
    from.get(dice.next() as usize % from.len().max(1))
}

impl Walk {
    /// Puts `count` walkers on the ways between places, and in `characters`.
    pub fn start(
        count: usize,
        places: &[[i32; 2]],
        land: impl Fn(i32, i32) -> bool,
        surface: impl Fn(i32, i32) -> i32,
        characters: &mut Characters,
    ) -> Self {
        let middles: Vec<[f64; 2]> = places
            .iter()
            .map(|[x, z]| [f64::from(*x), f64::from(*z)])
            .collect();
        let mut dice = Dice(SEED);
        let places: Vec<Place> = (0..middles.len())
            .map(|from| Place {
                middle: middles[from],
                stops: stops_around(middles[from], &mut dice, &land),
                ways: ways_from(from, &middles, &land),
            })
            .collect();
        let mut walkers = Vec::with_capacity(count);
        for index in (0..count).take_while(|_| !places.is_empty()) {
            let id = u128::from(SEED) << 32 | index as u128;
            let from = index % places.len();
            let (place, path) = next_stop(&places, &mut dice, from);
            // Somewhere on the way already, so that the map is not empty for a minute.
            let leg = dice.next() as usize % path.len();
            let leg_start = leg
                .checked_sub(1)
                .map_or(places[from].middle, |turn| path[turn]);
            let [x, z] = between(leg_start, path[leg], dice.share());
            let block = [x.floor() as i32, z.floor() as i32];
            let (body, slim) = SKINS[index % SKINS.len()];
            let name = match index / NAMES.len() {
                0 => NAMES[index].to_string(),
                round => format!("{}{}", NAMES[index % NAMES.len()], round + 1),
            };
            characters.0.insert(
                id,
                Character {
                    name,
                    skin: ResolvableProfile::textured(body, slim),
                    position: Position {
                        x,
                        y: f64::from(surface(block[0], block[1]) + 1),
                        z,
                    },
                    yaw: yaw_towards(path[leg][0] - x, path[leg][1] - z),
                },
            );
            walkers.push(Walker {
                id,
                place,
                path,
                leg,
                resting: 0,
                block,
            });
        }
        Self {
            places,
            walkers,
            dice,
        }
    }

    /// A tick of everyone's walk.
    pub fn step(&mut self, characters: &mut Characters, surface: impl Fn(i32, i32) -> i32) {
        let Self {
            places,
            walkers,
            dice,
        } = self;
        for walker in walkers {
            let Some(character) = characters.0.get_mut(&walker.id) else {
                continue;
            };
            if walker.resting > 0 {
                walker.resting -= 1;
                if walker.resting == 0 {
                    (walker.place, walker.path) = next_stop(places, dice, walker.place);
                    walker.leg = 0;
                }
                continue;
            }
            let mut left = WALK_SPEED;
            while left > 0.0 && walker.resting == 0 {
                let [x, z] = walker.path[walker.leg];
                let (dx, dz) = (x - character.position.x, z - character.position.z);
                let length = dx.hypot(dz);
                if length > left {
                    character.position.x += dx / length * left;
                    character.position.z += dz / length * left;
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

/// The walkers, once the map knows its places.
#[derive(Resource, Debug)]
struct Simulation {
    count: usize,
    walk: Option<Walk>,
}

/// Where a place of the world stands on a model, in blocks from its centre.
fn place_on(scene: &Scene, position: &Position) -> Option<[f64; 3]> {
    let (block_x, block_z) = (position.x.floor() as i32, position.z.floor() as i32);
    let (x, z) = scene.cell_of(block_x, block_z)?;
    let top = scene.top(x, z)?;
    let scale = scene.scale();
    let per_cell = f64::from(scale.blocks_per_cell);
    let col = u16::try_from(x + scale.radius).ok()?;
    let row = u16::try_from(z + scale.radius).ok()?;
    let (corner_x, corner_z) = scale.window().block_at(col, row);
    Some([
        f64::from(x) + (position.x - f64::from(corner_x)) / per_cell,
        f64::from(top + 1),
        f64::from(z) + (position.z - f64::from(corner_z)) / per_cell,
    ])
}

/// The smallest size that still gets drawn from this far.
fn size_for(distance: f64) -> f64 {
    (distance * DRAWN_MARGIN / DRAWN_PER_SCALE).clamp(SCALE_LIMITS.0, SCALE_LIMITS.1)
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
}

impl Figures {
    fn clear(&mut self, commands: &mut Commands) {
        let figures = std::mem::take(&mut self.shown);
        let label = self.label.take().map(|label| label.entity);
        for entity in figures.values().map(|f| f.entity).chain(label) {
            commands.entity(entity).try_despawn();
        }
    }
}

fn spawn_figure(commands: &mut Commands, at: Position, character: &Character, size: f64) -> Entity {
    EntityBuilder::new(EntityKind::Mannequin)
        .position(at)
        .rotation(character.yaw, 0.0)
        .gravity(false)
        .block_collision(false)
        .settle_ticks(0)
        .with_bundle((
            Mannequin::new(character.skin.clone()),
            Attributes::for_kind(EntityKind::Mannequin).base(EntityAttribute::Scale, size),
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
            let land = |x, z| atlas.on_land(x, z);
            let started = Walk::start(count, &places, land, surface, &mut characters);
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

/// Keeps every session's figures on the characters.
fn show(
    characters: Res<Characters>,
    atlas: Res<Atlas>,
    mut sessions: Query<(&MapSession, &mut Figures)>,
    mut bodies: Query<(&mut Position, &mut Rotation)>,
    mut attributes: Query<&mut Attributes>,
    mut cost: ResMut<Cost>,
    mut commands: Commands,
) {
    let started = Instant::now();
    for (session, mut figures) in &mut sessions {
        let Some(view) = session.view(&atlas) else {
            continue;
        };
        let figures = &mut *figures;
        let model = view.scene.scale();
        if figures.model != Some(model) {
            figures.clear(&mut commands);
            figures.model = Some(model);
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
                let size = size_for(apart(at, view.eye));
                let spot = world_spot(&view, at);
                let entity = spawn_figure(&mut commands, spot, character, size);
                figures.shown.insert(*id, Figure { entity, at, size });
                continue;
            };
            figure.at = [*x, climbed(figure.at[1], *y), *z];
            if let Ok((mut position, mut rotation)) = bodies.get_mut(figure.entity) {
                if position.set_if_neq(world_spot(&view, figure.at)) {
                    cost.moves += 1;
                }
                rotation.set_if_neq(Rotation {
                    yaw: character.yaw,
                    pitch: 0.0,
                });
            }
            let wanted = size_for(apart(figure.at, view.eye));
            if (wanted / figure.size - 1.0).abs() > SIZE_STEP
                && let Ok(mut attributes) = attributes.get_mut(figure.entity)
            {
                figure.size = wanted;
                attributes.set_base(EntityAttribute::Scale, wanted);
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
    bodies: &mut Query<(&mut Position, &mut Rotation)>,
    commands: &mut Commands,
) {
    let ids: Vec<u128> = figures.shown.keys().copied().collect();
    let current = figures
        .label
        .and_then(|label| ids.iter().position(|id| *id == label.id));
    let picked = view.aim.and_then(|aim| {
        let points = figures.shown.values().map(|figure| figure.up(0.5));
        aimed(view.eye, aim, points, current).map(|index| ids[index])
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
        && let Ok((mut position, _)) = bodies.get_mut(label.entity)
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
    mut commands: Commands,
) {
    if let Ok(mut figures) = figures.get_mut(event.entity) {
        figures.clear(&mut commands);
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
    use crate::regions::Regions;
    use crate::scene::{DETAIL, OVERVIEW};
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
        let reached = |from: usize| {
            walk.places[from]
                .ways
                .iter()
                .map(|w| w.to)
                .collect::<Vec<_>>()
        };
        assert_eq!(reached(0), [2]);
        assert_eq!(reached(1), [] as [usize; 0]);
        assert_eq!(reached(2), [0]);
    }

    #[test]
    fn a_way_goes_around_a_lake_in_a_few_straight_lines() {
        let lake = |x: i32, z: i32| (x - 60).abs() < 20 && z.abs() < 80;
        let land = |x: i32, z: i32| !lake(x, z);
        let mut characters = Characters::default();
        let walk = Walk::start(1, &[[0, 0], [120, 0]], land, |_, _| 70, &mut characters);
        let way = &walk.places[0].ways[0];
        assert!(way.turns.len() <= 6, "{:?}", way.turns);
        for pair in way.turns.windows(2) {
            assert!(dry(pair[0], pair[1], &land), "{pair:?}");
        }
    }

    #[test]
    fn a_bridge_is_a_way_across() {
        let land = |x: i32, z: i32| !(40..60).contains(&x) || (-3..3).contains(&z);
        let mut characters = Characters::default();
        let walk = Walk::start(1, &[[0, 30], [100, 30]], land, |_, _| 70, &mut characters);
        assert_eq!(walk.places[0].ways.len(), 1);
    }

    #[test]
    fn a_place_nothing_leads_to_keeps_its_walkers_around_it() {
        let mut characters = Characters::default();
        let mut walk = Walk::start(1, &[[0, 0]], |_, _| false, |_, _| 70, &mut characters);
        for _ in 0..4000 {
            walk.step(&mut characters, |_, _| 70);
        }
        let at = characters.0.values().next().unwrap().position;
        assert!(at.x.hypot(at.z) <= STOP_AROUND.end as f64);
    }

    #[test]
    fn walkers_only_stop_on_land() {
        let land = |x: i32, _: i32| x >= 0;
        let mut characters = Characters::default();
        let walk = Walk::start(1, &[[0, 0]], land, |_, _| 70, &mut characters);
        assert!(!walk.places[0].stops.is_empty());
        assert!(walk.places[0].stops.iter().all(|stop| stop[0] >= 0.0));
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

    #[test]
    fn nobody_shows_off_the_model() {
        assert_eq!(place_on(&scene(OVERVIEW), &at(100_000.0, 0.0)), None);
    }

    #[test]
    fn a_figure_is_large_enough_to_be_drawn_from_where_it_is_seen() {
        for distance in [60.0, 150.0, 300.0] {
            assert!(size_for(distance) * DRAWN_PER_SCALE >= distance);
        }
    }

    #[test]
    fn a_figure_stays_within_what_the_attribute_allows() {
        assert_eq!(size_for(0.0), SCALE_LIMITS.0);
        assert_eq!(size_for(1e6), SCALE_LIMITS.1);
    }

    #[test]
    fn a_figure_climbs_a_terrace_step_by_step() {
        assert_eq!(climbed(70.0, 73.0), 70.0 + CLIMB_SPEED);
        assert_eq!(climbed(70.0, 70.1), 70.1);
        assert_eq!(climbed(73.0, 70.0), 73.0 - CLIMB_SPEED);
    }
}
