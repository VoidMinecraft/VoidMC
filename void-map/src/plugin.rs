//! Takes players into the map and back.
//!
//! In the map a player is a spectator who cannot move. [`MapMode::Look`] is an
//! overview: they look around freely, with no delay since the client turns the view.
//! [`MapMode::Pan`] explores a larger model, streamed around the camera: they ride an
//! invisible entity and see through it, and the mouse moves it. Either way the middle
//! of the screen is the pointer.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{
    Billboard, BossBar, BossBarColor, Command, CommandBuilder, CommandContext, CommandRegistry,
    Display, DisplayTransform, EntityBuilder, EntityKind, EntityShownEvent, EnumArg, Glowing,
    Hidden, Invisible, ItemDisplay, ItemDisplayContext, ItemStack, Mannequin, Messages, NoGravity,
    Passengers, PlayerAbilities, Players, RegistryDataStore, ServerConfigResource, Silent,
    TabEntry, Teleport, TextColor, TextDisplay, WorldGen, WorldGenerator,
    components::{
        ChunkStreamBacklog, ChunkStreamingPaused, ClientId, EffectiveViewDistance, LoadedChunks,
        MinecraftEntityId, PlayerDimension, PlayerUuid, Position, Rotation,
        ServerControlledPosition,
    },
    events::{PlayerInputEvent, PlayerQuitEvent},
    network::PacketEvent,
    world::{
        ChunkData, ChunkDimension, ChunkIndex, ChunkLoaderResource, ChunkPos, ChunkPosition,
        DimensionId, load_or_generate,
    },
};
use voidmc_codec::Encode;
use voidmc_protocol::{
    clientbound::{
        ChunkDataAndLight, EntityPositionSync, GameEvent, GameEventType, PlayerRotation,
        ResolvableProfile, SectionBlocksUpdate, SetCamera, SetCenterChunk, SetChunkCacheRadius,
        SystemChat, UnloadChunk,
    },
    serverbound::{Interact, SetPlayerPos, SetPlayerPosAndRot, SetPlayerRotation, SpectateEntity},
    slot::{DataComponent, component_ids},
};
use voidmc_ui::{PackSettled, Text};

use crate::art::{Card, Mark, Style, Subject, WHEEL_MIDDLE_DROP, sign, wheel_face};
use crate::biome::{self, MapLevel};
use crate::cursor::{self, CursorPlugin};
use crate::markers::{Marker, MarkerField, MarkerKind};
use crate::motion::Motion;
use crate::players::PlayersPlugin;
use crate::pointer::{
    CLIENT_SNAP_DISTANCE, COARSE_ABOVE, COARSEST_ABOVE, DETAIL_BELOW, DETAIL_PITCH, DIVE_STOP,
    DIVE_TICKS, FreeLook, MIDDLE_BELOW, Mouse, OVERVIEW_Z, Pan, RECENTRED_PITCH, REST_PITCH,
    REST_YAW, SPECTATOR_EYE_HEIGHT, Zoom, aimed, detail_back, look_direction, near_marker,
    overview_y,
};
use crate::regions::{RegionState, Regions, ZoneLook};
use crate::scene::{
    self, Blocks, ChunkLook, DETAIL, MIDDLE, OVERVIEW, Scale, Scene, SectionChange,
};
use crate::telemetry::Telemetry;
use crate::terrain::Terrain;
use crate::transition::{
    CLOSING_TICKS, FLIGHT_GLIDE_TICKS, Flight, OPENING_TICKS, RISE_TICKS, SWAP_TICKS, TURN_TICKS,
    VEIL_FADE, veil_in, veil_out,
};
use crate::wheel::{CATEGORIES, Wheel, stepped};

/// `WorldGenerator` does not expose its water level. This is the one of
/// `DefaultWorldGenerator`, and of vanilla.
pub const SEA_LEVEL: i32 = 62;
const SPECTATOR: u8 = 3;

/// How far from the middle of the world a player's real chunks stay with them in the map:
/// all that the map shows, and a view distance around it.
const WORLD_KEPT_REACH: i32 = 48;
/// Chunks of nothing between those and the models. The most a client renders, so that the
/// real world never shows on the horizon of a model.
const RENDER_GAP: i32 = 34;
/// Each player's models sit this far east, in chunks only they get. Entities are only
/// shown to players with their chunk loaded, so nobody else sees the markers. They are
/// this close for the client to keep them and the real world at once, in one square: what
/// a player comes back to is already there.
const FIRST_ANCHOR_CHUNK: i32 = WORLD_KEPT_REACH + RENDER_GAP + DETAIL_PAN_REACH;
const CHUNKS_BETWEEN_ANCHORS: i32 = 128;
/// The rows of models, north to south, and how far they stand apart.
const ANCHOR_ROWS: i32 = 9;
const CHUNKS_BETWEEN_ROWS: i32 = 2 * DETAIL_PAN_REACH + 2;
/// Two players on the same anchor see each other's markers. Each more column or row makes
/// the square that a client keeps larger, at a few bytes for each chunk of it.
const ANCHOR_COLUMNS: i32 = 4;

/// How far the overview's chunks go, which is the model plus the empty ring the
/// client needs to draw its last chunks.
const OVERVIEW_REACH: i32 = (OVERVIEW.radius + 15) / 16 + 1;
/// Chunks sent around a panning camera.
const DETAIL_SEND_RADIUS: i32 = 9;
/// The chunks a pan can ask for, on each side of a model's centre.
const fn pan_reach(scale: Scale) -> i32 {
    scale.radius / 16 + 2 + DETAIL_SEND_RADIUS
}
const DETAIL_PAN_REACH: i32 = pan_reach(DETAIL);
const MIDDLE_PAN_REACH: i32 = pan_reach(MIDDLE);
const OVERVIEW_PAN_REACH: i32 = pan_reach(OVERVIEW);
/// The models sit in a row, in chunks east of the detail model. They all stay with the
/// client, so that a zoom past a threshold only moves the camera from one to the next.
const MIDDLE_ORIGIN: i32 = DETAIL_PAN_REACH + MIDDLE_PAN_REACH + 1;
const OVERVIEW_ORIGIN: i32 = MIDDLE_ORIGIN + MIDDLE_PAN_REACH + OVERVIEW_PAN_REACH + 1;
const _: () = assert!(
    DETAIL_PAN_REACH + OVERVIEW_ORIGIN + OVERVIEW_PAN_REACH < CHUNKS_BETWEEN_ANCHORS,
    "a player's models stay off the next player's"
);
const _: () = assert!(
    (2 * DETAIL_SEND_RADIUS + 1) * 16 > CLIENT_SNAP_DISTANCE as i32,
    "a camera changing model has to jump, not glide"
);
/// What the model the player is not on gets of each tick.
const NEIGHBOUR_CHUNKS_PER_TICK: usize = 16;
pub(crate) const CHUNKS_PER_TICK: usize = 64;
/// Blocks a change of look turns on the client in a tick, nearest first. The chunks are
/// left in place: a look only reaches them as blocks, so nothing goes away and comes back.
const REPAINTED_BLOCKS_PER_TICK: usize = 20_000;
/// What a tick gives the real world, sent ahead of a player.
const WORLD_CHUNKS_PER_TICK: usize = 32;

/// A shorter glide moves in visible steps. A longer one only overshoots if the view
/// stops dead, because the client adds what the entity moved on its own to the target
/// of a glide: `Pan` eases the view to a stop for that reason.
const CAMERA_GLIDE_TICKS: u8 = 3;

/// Above the tallest landmark, in its blocks.
const ICON_LIFT: f64 = 10.0;
const ICON_SCALE: f32 = 5.0;
const ICON_HOVER_GROWTH: f32 = 1.6;
/// The head floats over the player's spot, on a pin always drawn right under it.
const MINI_HEAD_LIFT: f64 = 4.0;
const MINI_HEAD_SCALE: f32 = 3.4;
const TIP_SCALE: f32 = 4.0;
/// In front of the camera, in blocks. A wheel fills about half of the screen's height.
const WHEEL_DISTANCE: f64 = 8.0;
const WHEEL_SCALE: f32 = 1.0;
/// What a name of a look gets on screen before the hint comes back, in ticks.
const TOLD_TICKS: u32 = 50;
/// How much of its scale it takes to reach the edge of a head.
const HEAD_HALF: f32 = 0.22;
/// A disc's half, in blocks at scale 1.
const DISC_RADIUS: f32 = 12.0 * DISC_SCALE / PIXELS_PER_BLOCK;
const DISC_SCALE: f32 = 5.0;
const PIN_SCALE: f32 = 7.0;
const GROUND_MARK_SCALE: f32 = 8.0;
/// Over the ground, in its blocks, so that it does not flicker with it.
const GROUND_MARK_LIFT: f64 = 0.15;
const SIGN_LIFT: f64 = 14.0;
const SIGN_SCALE: f32 = 5.0;
/// A pan starts this far north of the player, for each block of height, to keep their
/// marker off the middle of the screen.
const PAN_START_NORTH: f64 = 0.25;
/// A display's text is 40 pixels to the block, before its scale.
const PIXELS_PER_BLOCK: f32 = 40.0;
/// How much of an icon's scale it takes to reach its top.
const ICON_HALF: f32 = 0.4;
/// Between a card and its marker, in pixels of the card.
const CARD_GAP: f32 = 3.0;
/// A text display's line stands this many pixels above the entity.
const LINE_HEIGHT: f32 = 9.0;
/// So that the drawn line, which can walk far, never wraps.
const CARD_LINE_WIDTH: i32 = 4096;
/// Keeps a card the same size on screen, from how far away it is seen.
const CARD_SCALE_PER_BLOCK: f64 = 0.09;
const PLAIN_CARD_BACKGROUND: i32 = 0xD8_10_14_1C_u32 as i32;
const CLEAR: i32 = 0;

/// A spectator only clicks when the look hits an entity. A minecart around the eye is
/// always hit. The eye sits under its floor, looking down, so the cart never shows.
const EYE_BOX_DROP: f64 = 0.02;
/// Nobody sees it. A mannequin needs one anyway.
const EYE_SKIN: &str = "entity/player/wide/steve";
/// Displays vanish past 64 blocks times this, and the camera is further than that.
const VIEW_RANGE: f32 = 8.0;
const GLOW: i32 = 0xFFE9A0;
const HINT_PERIOD: u32 = 30;
const SUMMARY_PERIOD: u32 = 200;
/// A card follows the zoom by steps of this much.
const CARD_ZOOM_STEP: f64 = 0.12;
/// Markers follow the zoom by steps of this much.
const MARKER_ZOOM_STEP: f64 = 0.02;
/// A marker keeps its size on screen from this height, in blocks of its model.
const MARKER_FULL_HEIGHT: f64 = 105.0;
const PAN_MARKER_SCALE: f64 = 2.0;
/// A right click sends one packet per hand.
const CLICK_COOLDOWN: u32 = 3;
/// Too close to where players start for a marker to tell from the "you" pin.
const CLEAR_AROUND_ORIGIN: f64 = 64.0;

/// The fixed scatter of markers around the origin. Those off the model are dropped.
pub const MARKERS: MarkerField = MarkerField {
    seed: 0x766f_6964_6d61_7000,
    count: 10,
    radius: 380,
    min_spacing: 120,
    sea_level: SEA_LEVEL,
};

fn item(name: &str) -> ItemStack {
    ItemStack::of(name, 1).expect("a vanilla item")
}

fn icon(kind: MarkerKind) -> ItemStack {
    item(match kind {
        MarkerKind::Village => "minecraft:emerald",
        MarkerKind::Ruins => "minecraft:bone",
        MarkerKind::Mine => "minecraft:iron_pickaxe",
        MarkerKind::Lighthouse => "minecraft:lantern",
    })
}

/// How the mouse drives the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapMode {
    /// The whole map on screen. The player looks around, with no delay.
    #[default]
    Look,
    /// A larger model. The mouse slides the view, whose look never changes.
    Pan,
}

impl MapMode {
    fn other(self) -> Self {
        match self {
            Self::Look => Self::Pan,
            Self::Pan => Self::Look,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Look => "Vue d'ensemble",
            Self::Pan => "Exploration",
        }
    }
}

const TRAVEL_ACTION: &str = "Clic gauche : s'y rendre";
/// Above the ground of the world, so as not to land in it.
const ARRIVAL_CLEARANCE: f64 = 0.02;
/// A client widens the view of a flying player by a tenth, and of one faster than their
/// walking speed by half the difference, but never of what they see through. A spectator
/// walks at nothing: this speed takes the tenth back, for a view through an entity and
/// through the eye to be the same one. The movement speed of a player is 0.1.
const UNWIDENED_WALKING_SPEED: f32 = 0.1 * 11.0 / 9.0;
/// What the veil waits for, at the least: the client to settle on what it has got.
const VEIL_HOLD_TICKS: u32 = 2;
/// The chunks around the camera that have to be there for the veil to lift, on a model.
const NEAR_MODEL_CHUNKS: i32 = 4;
/// The same around the player, back in the world. The rest follows behind.
const NEAR_WORLD_CHUNKS: i32 = 3;
/// How long the world gets to draw once the camera is back in it, under the veil.
const LANDING_HOLD_TICKS: u32 = SWAP_TICKS;
/// When the world still has not come after this, the player goes on without it.
const LANDING_PATIENCE_TICKS: u32 = 60;

/// The models of the map. The further from the ground a pan is, the larger the blocks
/// of its model, to see further than a render distance can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Model {
    Overview,
    Middle,
    Detail,
}

impl Model {
    fn scale(self) -> Scale {
        match self {
            Self::Overview => OVERVIEW,
            Self::Middle => MIDDLE,
            Self::Detail => DETAIL,
        }
    }

    /// Blocks of this model to a block of the detail model.
    fn ratio(self) -> f64 {
        f64::from(DETAIL.blocks_per_cell) / f64::from(self.scale().blocks_per_cell)
    }

    /// Where its centre sits, in chunks east of a player's anchor.
    fn origin(self) -> i32 {
        match self {
            Self::Overview => OVERVIEW_ORIGIN,
            Self::Middle => MIDDLE_ORIGIN,
            Self::Detail => 0,
        }
    }

    /// The chunks a pan can ask for, on each side of its centre.
    fn reach(self) -> i32 {
        match self {
            Self::Overview => OVERVIEW_PAN_REACH,
            Self::Middle => MIDDLE_PAN_REACH,
            Self::Detail => DETAIL_PAN_REACH,
        }
    }

    /// The models a zoom can move to from this one.
    fn neighbours(self) -> &'static [Self] {
        match self {
            Self::Overview => &[Self::Middle],
            Self::Middle => &[Self::Detail, Self::Overview],
            Self::Detail => &[Self::Middle],
        }
    }

    /// The model for a height, from this one: the gaps stop a held height from flipping.
    fn at_height(self, height: f64) -> Self {
        match self {
            Self::Detail if height > COARSE_ABOVE => Self::Middle,
            Self::Middle if height > COARSEST_ABOVE => Self::Overview,
            Self::Middle if height < DETAIL_BELOW => Self::Detail,
            Self::Overview if height < MIDDLE_BELOW => Self::Middle,
            model => model,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Waypoint {
    marker: Marker,
    /// On the model, from its centre.
    at: [f64; 3],
}

const CLOUDS: ChunkLook = ChunkLook {
    zone: ZoneLook::Clouds,
    blocks: Blocks::Flat,
};

/// One model, with its chunks as they get asked for.
struct Level {
    scene: Scene,
    waypoints: Vec<Waypoint>,
    /// Where each closed region shows what it is, on its land, with its state.
    signs: Vec<([f64; 3], RegionState)>,
    /// Built and encoded once, each with its weight on the wire.
    chunks: HashMap<((i32, i32), ChunkLook), (ChunkDataAndLight, usize)>,
    /// Worked out once for all players.
    repaints: HashMap<((i32, i32), ChunkLook, ChunkLook), Vec<SectionChange>>,
}

impl Level {
    fn new(generator: &dyn WorldGenerator, scale: Scale, regions: &Regions) -> Self {
        let terrain = Terrain::sample(generator, &scale.window());
        let mut scene = Scene::new(&terrain, SEA_LEVEL, scale, regions);
        let places: Vec<_> = MARKERS
            .place(generator)
            .into_iter()
            .filter(|m| f64::from(m.x).hypot(f64::from(m.z)) > CLEAR_AROUND_ORIGIN)
            .filter_map(|m| {
                let cell = scene.cell_of(m.x, m.z)?;
                scene.top(cell.0, cell.1)?;
                Some((m, cell))
            })
            .collect();
        let cells: Vec<_> = places.iter().map(|(_, cell)| *cell).collect();
        for (from, to) in spanning_paths(&cells) {
            scene.pave(from, to);
        }
        let waypoints = places
            .into_iter()
            .map(|(marker, cell)| {
                scene.build(cell, marker.kind);
                let top = scene.top(cell.0, cell.1).unwrap_or(scene::SEA_Y);
                Waypoint {
                    marker,
                    at: [
                        f64::from(cell.0) + 0.5,
                        f64::from(top + 1),
                        f64::from(cell.1) + 0.5,
                    ],
                }
            })
            .collect();
        let signs = regions
            .closed()
            .filter_map(|region| {
                let (x, z) = scene.middle_of_land(regions, region)?;
                let top = scene.top(x, z)?;
                let at = [f64::from(x) + 0.5, f64::from(top), f64::from(z) + 0.5];
                Some((at, region.state))
            })
            .collect();
        Self {
            scene,
            waypoints,
            signs,
            chunks: HashMap::new(),
            repaints: HashMap::new(),
        }
    }

    fn chunk(
        &mut self,
        at: (i32, i32),
        look: ChunkLook,
        info: MapLevel,
    ) -> &(ChunkDataAndLight, usize) {
        self.chunks.entry((at, look)).or_insert_with(|| {
            let packet = self
                .scene
                .chunk(at.0, at.1, info.biome, info.biome_count, look)
                .to_packet();
            let mut bytes = Vec::new();
            packet.encode(&mut bytes);
            let weight = bytes.len();
            (packet, weight)
        })
    }

    fn repaint(&mut self, at: (i32, i32), from: ChunkLook, to: ChunkLook) -> &[SectionChange] {
        let scene = &self.scene;
        self.repaints
            .entry((at, from, to))
            .or_insert_with(|| scene.repaint(at.0, at.1, from, to))
    }

    /// Builds the island's chunks ahead of time: on a tick, it would make that tick
    /// late. The looks with less to them wait for a player to ask for them.
    fn prepare(&mut self, info: MapLevel) {
        let reach = self.scene.chunk_radius();
        for x in -reach..=reach {
            for z in -reach..=reach {
                self.chunk((x, z), ChunkLook::default(), info);
                self.chunk((x, z), CLOUDS, info);
            }
        }
    }
}

/// A model as it is shown, for the tests of what stands on it.
#[cfg(test)]
pub(crate) fn built_scene(
    generator: &dyn WorldGenerator,
    scale: Scale,
    regions: &Regions,
) -> Scene {
    Level::new(generator, scale, regions).scene
}

/// The waypoints of the map, in blocks of the world, for the same tests.
#[cfg(test)]
pub(crate) fn marker_places(generator: &dyn WorldGenerator) -> Vec<[i32; 2]> {
    Level::new(generator, DETAIL, &Regions::demo())
        .waypoints
        .iter()
        .map(|w| [w.marker.x, w.marker.z])
        .collect()
}

/// Links every cell to the others, with the shortest paths that do.
fn spanning_paths(cells: &[(i32, i32)]) -> Vec<((i32, i32), (i32, i32))> {
    let distance = |a: (i32, i32), b: (i32, i32)| (a.0 - b.0).pow(2) + (a.1 - b.1).pow(2);
    let mut linked: Vec<_> = cells.iter().take(1).copied().collect();
    let mut rest: Vec<_> = cells.iter().skip(1).copied().collect();
    let mut paths = Vec::new();
    while !rest.is_empty() {
        let closest = linked
            .iter()
            .flat_map(|a| {
                rest.iter()
                    .enumerate()
                    .map(move |(index, b)| (*a, *b, index))
            })
            .min_by_key(|(a, b, _)| distance(*a, *b));
        let Some((from, to, index)) = closest else {
            break;
        };
        paths.push((from, to));
        linked.push(rest.swap_remove(index));
    }
    paths
}

/// What every player's map shows, built once from the server's generator.
#[derive(Resource)]
pub struct Atlas {
    info: MapLevel,
    regions: Regions,
    overview: Option<Level>,
    middle: Option<Level>,
    detail: Option<Level>,
}

impl Atlas {
    fn level(&self, model: Model) -> Option<&Level> {
        match model {
            Model::Overview => self.overview.as_ref(),
            Model::Middle => self.middle.as_ref(),
            Model::Detail => self.detail.as_ref(),
        }
    }

    fn level_mut(&mut self, model: Model) -> Option<&mut Level> {
        match model {
            Model::Overview => self.overview.as_mut(),
            Model::Middle => self.middle.as_mut(),
            Model::Detail => self.detail.as_mut(),
        }
    }
}

/// The entities a session adds once the player is in place.
#[derive(Debug, Clone, Copy)]
struct Rig {
    eye_box: Entity,
    /// What the view moves on, or rides, for a pan or for a flight.
    camera: Option<Entity>,
    /// What sees for the eye of a look on a flight, riding the camera.
    eye: Option<Entity>,
}

/// An entity that glides, with one riding it that sees from exactly where an eye does. The
/// client eases the height of the eye of whatever it sees through, over a few ticks. From
/// the eye of a player to a thing with no eye, that is a jump of 1.62 blocks at the start
/// of a flight, and the reverse at its end. A display entity has no eye and is the only
/// one to glide this way, so the two go together: a flight that starts or ends in the
/// head of a player is seen through a mannequin on a display entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EyeCamera {
    vehicle: Entity,
    eye: Entity,
}

/// How far over the display entity a rider's eye is, once it sits on it.
const RIDER_EYE_HEIGHT: f64 = SPECTATOR_EYE_HEIGHT - RIDER_SEAT_DROP;
/// A humanoid sits this far below its vehicle's seat.
const RIDER_SEAT_DROP: f64 = 0.6;

/// The markers of the current mode.
#[derive(Debug, Default)]
struct Pins {
    icons: Vec<Entity>,
    you: Option<You>,
    /// What says a region is closed, for the looks that have it.
    signs: Vec<Entity>,
    card: Option<ShownCard>,
}

/// The player's own marker.
#[derive(Debug, Clone)]
struct You {
    parts: Vec<(Entity, Part)>,
    /// Where they stand on the model.
    at: [f64; 3],
}

/// One entity of the player's marker.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Part {
    /// Standing on the spot, whichever way it is seen from.
    Pin,
    /// Lying on the ground, with its top towards this yaw.
    Flat(Mark, f32),
    Disc,
    Head,
    /// Pointing down, right under a head, this far from its middle at scale 1.
    Tip(f32),
}

impl Part {
    /// How large it is at the height the marker scale is 1.
    fn size(self) -> f32 {
        match self {
            Self::Pin => PIN_SCALE,
            Self::Flat(..) => GROUND_MARK_SCALE,
            Self::Disc => DISC_SCALE,
            Self::Head => MINI_HEAD_SCALE,
            Self::Tip(_) => TIP_SCALE,
        }
    }

    fn transform(self, scale: f32) -> DisplayTransform {
        match self {
            Self::Pin => {
                // The tip of the drop is its lowest pixel.
                let rise = Mark::Pin.height() as f32 - LINE_HEIGHT;
                DisplayTransform::default()
                    .translation(0.0, rise * scale / PIXELS_PER_BLOCK, 0.0)
                    .uniform_scale(scale)
            }
            Self::Flat(mark, yaw) => flat_transform(mark, yaw, scale),
            Self::Disc => {
                let centre = LINE_HEIGHT - Mark::Disc.height() as f32 / 2.0;
                DisplayTransform::default()
                    .translation(0.0, -centre * scale / PIXELS_PER_BLOCK, 0.0)
                    .uniform_scale(scale)
            }
            Self::Head => DisplayTransform::default().uniform_scale(scale),
            Self::Tip(under) => {
                let line = LINE_HEIGHT / PIXELS_PER_BLOCK;
                DisplayTransform::default()
                    .translation(0.0, -(under / TIP_SCALE + line) * scale, 0.0)
                    .uniform_scale(scale)
            }
        }
    }
}

/// Lays a glyph on the ground, centred on the entity. Text faces south with its top up:
/// a quarter turn back lays it down with its top to the north, and a turn around the
/// vertical points that top where `yaw` looks.
fn flat_transform(mark: Mark, yaw: f32, scale: f32) -> DisplayTransform {
    let half = |degrees: f32| degrees.to_radians() / 2.0;
    let (tilt_sin, tilt_cos) = half(-90.0).sin_cos();
    let (turn_sin, turn_cos) = half(180.0 - yaw).sin_cos();
    let rotation = [
        turn_cos * tilt_sin,
        turn_sin * tilt_cos,
        -turn_sin * tilt_sin,
        turn_cos * tilt_cos,
    ];
    // The middle of the glyph, above the entity along the top of the text.
    let centre = (LINE_HEIGHT - mark.height() as f32 / 2.0) * scale / PIXELS_PER_BLOCK;
    let (sin, cos) = (180.0 - yaw).to_radians().sin_cos();
    DisplayTransform::default()
        .translation(centre * sin, 0.0, centre * cos)
        .left_rotation(rotation)
        .uniform_scale(scale)
}

/// The square of chunks a client keeps around a centre, and where a player's models go in
/// it. It holds the models and the world at the same time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Site {
    anchor: [i32; 2],
    centre: [i32; 2],
    radius: i32,
}

impl Site {
    fn of(client: u32) -> Self {
        let slot = (client % (ANCHOR_COLUMNS * ANCHOR_ROWS) as u32) as i32;
        let (column, row) = (slot / ANCHOR_ROWS, slot % ANCHOR_ROWS - ANCHOR_ROWS / 2);
        let anchor = [
            FIRST_ANCHOR_CHUNK + column * CHUNKS_BETWEEN_ANCHORS,
            row * CHUNKS_BETWEEN_ROWS,
        ];
        let west = -WORLD_KEPT_REACH;
        let east = anchor[0] + OVERVIEW_ORIGIN + OVERVIEW_PAN_REACH;
        let north = (-WORLD_KEPT_REACH).min(anchor[1] - DETAIL_PAN_REACH);
        let south = WORLD_KEPT_REACH.max(anchor[1] + DETAIL_PAN_REACH);
        Self {
            anchor,
            centre: [(west + east) / 2, (north + south) / 2],
            radius: ((east - west) / 2).max((south - north) / 2) + 1,
        }
    }

    fn holds(&self, chunk: ChunkPos) -> bool {
        (chunk.x - self.centre[0]).abs() <= self.radius
            && (chunk.z - self.centre[1]).abs() <= self.radius
    }
}

/// What a flight of the view on the model comes to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ending {
    /// The view it took over from.
    View,
    /// The world: the player's own place, or this one.
    World(Option<Position>),
}

/// The view on its way across a model.
#[derive(Debug, Clone, Copy)]
struct Travel {
    flight: Flight,
    ending: Ending,
}

/// The card on screen, and what it was drawn from, to tell when it has to change.
#[derive(Debug)]
struct ShownCard {
    entity: Entity,
    text: String,
    style: Style,
    hover: Hover,
    /// The height of the view it was sized for.
    zoom: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hover {
    Waypoint(usize),
    You,
}

/// The selector on a player's screen.
#[derive(Debug, Clone)]
struct OpenWheel {
    wheel: Wheel,
    display: Entity,
    /// How long it has been open.
    ticks: u32,
    /// What it shows, to tell when that has to change.
    face: String,
}

/// A player between the world and the map, seen through a camera of their own.
#[derive(Component)]
struct Passage {
    camera: EyeCamera,
    kind: PassageKind,
    /// The ticks gone by.
    ticks: u32,
    /// How to put the player back once they are down.
    back: Option<Back>,
}

/// What a player keeps being until they are back on their feet, and what they get then.
#[derive(Clone, Copy)]
struct Back {
    home: ChunkPos,
    abilities: Option<PlayerAbilities>,
    tab_game_mode: Option<u8>,
}

#[derive(Clone, Copy)]
enum PassageKind {
    /// Out of the world, into the map that is already coming in behind it.
    Rise,
    /// Waiting under the veil for the world to be back, then down this way.
    Landing(Flight),
    /// On the way down, with the veil going.
    Descent,
}

/// A player in the map, and how to put them back.
#[derive(Component)]
pub struct MapSession {
    mode: MapMode,
    site: Site,
    position: Position,
    rotation: Rotation,
    abilities: Option<PlayerAbilities>,
    tab_game_mode: Option<u8>,
    pins: Pins,
    bar: Entity,
    rig: Option<Rig>,
    mouse: Mouse,
    pan: Pan,
    zoom: Zoom,
    /// The model of a pan, from its height.
    pan_model: Model,
    /// The height of the view the markers are sized for.
    sized_for: f64,
    free: FreeLook,
    telemetry: Telemetry,
    /// The player's head, as an item.
    head: ItemStack,
    looks: MapLooks,
    /// The client has the pack, so it can draw.
    drawn: bool,
    /// The look of the chunks going out.
    painted: ChunkLook,
    /// The chunks the client has in another look, with that look.
    behind: HashMap<ChunkPos, ChunkLook>,
    /// The chunks of the real world the client keeps meanwhile.
    world: HashSet<ChunkPos>,
    /// Where the world should be ready next: the player may well go there.
    heading: Option<ChunkPos>,
    /// The entities to see the model through until the rig is up.
    camera: Option<(Entity, Option<Entity>)>,
    /// On the way across the model.
    travel: Option<Travel>,
    /// What waits over the world to take the view from the map, ready with its look.
    landing: Option<EyeCamera>,
    /// The veil is on, this many ticks since the map took over.
    veiled: Option<u32>,
    /// The client has the model around the camera.
    ready: bool,
    /// Held open by the sprint key.
    wheel: Option<OpenWheel>,
    /// When a choice last went on screen, for the hint to leave it there.
    told: u32,
    sneaking: bool,
    /// Jump, left and right as last reported, to act on a press and not on a repeat.
    keys: [bool; 3],
    /// Where the look is, in the middle of the screen.
    look: Rotation,
    hovered: Option<Hover>,
    selected: Option<usize>,
    streamed: (usize, usize),
    ticks: u32,
    last_click: u32,
}

impl MapSession {
    fn on_model(&self, x: f64, y: f64, z: f64) -> Position {
        let origin = self.site.anchor[0] + self.model().origin();
        Position {
            x: f64::from(origin * 16) + x,
            y,
            z: f64::from(self.site.anchor[1] * 16) + z,
        }
    }

    fn model(&self) -> Model {
        match self.mode {
            MapMode::Look => Model::Overview,
            MapMode::Pan => self.pan_model,
        }
    }

    /// The height of the view over the sea, in blocks of its model.
    fn height(&self) -> f64 {
        self.zoom.height() * self.model().ratio()
    }

    /// Where the player, or the entity they see through, stands on the model.
    fn standing(&self) -> [f64; 3] {
        if let Some(travel) = &self.travel {
            return travel.flight.at();
        }
        match self.mode {
            MapMode::Look => [0.0, overview_y(), OVERVIEW_Z],
            MapMode::Pan => self.pan_view(self.model()),
        }
    }

    /// The view of a pan on a model. The same point of the sea stays in the middle of
    /// the screen at the same size, whichever model it is on.
    fn pan_view(&self, model: Model) -> [f64; 3] {
        let ratio = model.ratio();
        let [x, z] = self.pan.view();
        let height = self.zoom.height() * ratio;
        [
            x * ratio,
            f64::from(scene::SEA_Y) + height,
            z * ratio + detail_back(height),
        ]
    }

    /// The eye of the view, on the model.
    fn eye(&self) -> [f64; 3] {
        let [x, y, z] = self.standing();
        match self.mode {
            MapMode::Look if self.travel.is_none() => [x, y + SPECTATOR_EYE_HEIGHT, z],
            MapMode::Look | MapMode::Pan => [x, y, z],
        }
    }

    /// Where the player stands once any flight is over.
    fn home_position(&self) -> Position {
        let [x, y, z] = match self.mode {
            MapMode::Look => [0.0, overview_y(), OVERVIEW_Z],
            MapMode::Pan => self.pan_view(self.model()),
        };
        self.on_model(x, y, z)
    }

    /// Where a flight out of the map puts the player, and how they look there.
    fn landing_place(&self, to: Option<Position>, flight: Flight) -> (Position, Rotation) {
        match to {
            Some(to) => {
                let yaw = flight.looking()[0];
                (to, Rotation { yaw, pitch: 0.0 })
            }
            None => (self.position, self.rotation),
        }
    }

    /// What a flight would end on if the map were left now.
    fn leaving(&self) -> bool {
        self.travel
            .is_some_and(|travel| travel.ending != Ending::View)
    }

    /// Riding the camera in a pan. In a look, it waits around the player during a dive.
    fn eye_box_position(&self) -> Position {
        let [x, y, z] = match (self.mode, &self.travel) {
            (MapMode::Look, _) => [0.0, overview_y() + SPECTATOR_EYE_HEIGHT, OVERVIEW_Z],
            (MapMode::Pan, Some(travel)) => travel.flight.at(),
            (MapMode::Pan, None) => self.eye(),
        };
        self.on_model(x, y - EYE_BOX_DROP, z)
    }

    /// How the camera entity looks, when there is one.
    fn camera_rotation(&self) -> Rotation {
        match &self.travel {
            Some(travel) => {
                let [yaw, pitch] = travel.flight.looking();
                Rotation { yaw, pitch }
            }
            None => self.look,
        }
    }

    /// Where the entity to see through goes. In a look, it stands in for the eye.
    fn camera_at(&self) -> Position {
        let [x, y, z] = match &self.travel {
            Some(travel) => self.vehicle_at(travel.flight.target()),
            None => self.vehicle_at(self.eye()),
        };
        self.on_model(x, y, z)
    }

    /// Where the camera of a flight goes, for the view to be somewhere. In a pan, the camera
    /// is what the player sees through; in a look, what the eye rides on.
    fn vehicle_at(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        match self.mode {
            MapMode::Look => [x, y - RIDER_EYE_HEIGHT, z],
            MapMode::Pan => [x, y, z],
        }
    }

    /// The camera holds still and nothing reacts to the mouse.
    fn is_frozen(&self) -> bool {
        self.wheel.is_some()
    }

    /// What the pointer has to show.
    pub(crate) fn wanted_cursor(&self) -> cursor::Wanted {
        // A session has no rig while it rises, and the veil comes down at the end of that.
        let rising = self.rig.is_none();
        let landing = self.travel.is_some_and(|travel| {
            matches!(travel.ending, Ending::World(_)) && travel.flight.flying_for() <= VEIL_FADE
        });
        if self.veiled.is_some() || rising || landing {
            cursor::Wanted::Veil
        } else if !self.drawn || self.travel.is_some() || self.is_frozen() {
            cursor::Wanted::Nothing
        } else if self.hovered.is_some() {
            cursor::Wanted::Pointer(cursor::Look::Over)
        } else {
            cursor::Wanted::Pointer(cursor::Look::Free)
        }
    }

    /// Takes in a rotation the client reports, and says how to correct it, if at all.
    fn turn(&mut self, yaw: f32, pitch: f32) -> Option<PlayerRotation> {
        self.rig?;
        self.telemetry.report(Instant::now());
        if let Some(open) = self.wheel.as_mut() {
            let turn = self.mouse.report(yaw, pitch);
            open.wheel.turn(turn.delta);
            return turn.recentre.then_some(PlayerRotation {
                yaw: 0.0,
                relative_yaw: true,
                pitch: RECENTRED_PITCH,
                relative_pitch: false,
            });
        }
        match self.mode {
            // Nothing of a look counts on the way: the view is given back as it should be.
            MapMode::Look if self.travel.is_some() => None,
            MapMode::Look => {
                self.look = Rotation { yaw, pitch };
                self.free.report(yaw, pitch);
                self.telemetry.past_limits(self.free.is_past(), yaw, pitch);
                None
            }
            MapMode::Pan => {
                let turn = self.mouse.report(yaw, pitch);
                if turn.delta != [0.0, 0.0] && self.travel.is_none() {
                    self.pan.apply(turn.delta);
                }
                turn.recentre.then_some(PlayerRotation {
                    yaw: 0.0,
                    relative_yaw: true,
                    pitch: RECENTRED_PITCH,
                    relative_pitch: false,
                })
            }
        }
    }

    fn rest(&self) -> Rotation {
        Rotation {
            yaw: REST_YAW,
            pitch: match self.mode {
                MapMode::Look => REST_PITCH,
                MapMode::Pan => DETAIL_PITCH,
            },
        }
    }

    /// The chunks a player should have, in the order they should get them.
    fn wanted(&self) -> Vec<(Model, (i32, i32))> {
        let chunk = |[x, _, z]: [f64; 3]| ((x / 16.0).floor() as i32, (z / 16.0).floor() as i32);
        let model = self.model();
        let here = chunks_around(self.mode, chunk(self.standing()));
        let mut wanted: Vec<_> = here.into_iter().map(|at| (model, at)).collect();
        if self.mode == MapMode::Pan {
            for neighbour in model.neighbours() {
                let there = chunks_around(self.mode, chunk(self.pan_view(*neighbour)));
                wanted.extend(there.into_iter().map(|at| (*neighbour, at)));
            }
        }
        wanted
    }

    /// Where a chunk of a model goes on the client.
    fn placed(&self, model: Model, (x, z): (i32, i32)) -> ChunkPos {
        ChunkPos::new(
            self.site.anchor[0] + model.origin() + x,
            self.site.anchor[1] + z,
        )
    }

    /// Which chunk of which model this is on the client.
    fn unplaced(&self, chunk: ChunkPos) -> Option<(Model, (i32, i32))> {
        [Model::Detail, Model::Middle, Model::Overview]
            .into_iter()
            .map(|model| (model, chunk.x - self.site.anchor[0] - model.origin()))
            .find(|(model, x)| x.abs() <= model.reach())
            .map(|(model, x)| (model, (x, chunk.z - self.site.anchor[1])))
    }

    /// Far markers are large and near ones smaller, so that they keep their size on screen.
    fn marker_scale(&self) -> f32 {
        match self.mode {
            MapMode::Look => 1.0,
            MapMode::Pan => (PAN_MARKER_SCALE * self.height() / MARKER_FULL_HEIGHT) as f32,
        }
    }

    fn icon_scale(&self, index: usize) -> f32 {
        let base = ICON_SCALE * self.marker_scale();
        let hovered = self.hovered == Some(Hover::Waypoint(index));
        if hovered || self.selected == Some(index) {
            base * ICON_HOVER_GROWTH
        } else {
            base
        }
    }

    /// The size of a part of the player's marker.
    fn part_scale(&self, part: Part) -> f32 {
        let base = part.size() * self.marker_scale();
        if self.hovered == Some(Hover::You) {
            base * ICON_HOVER_GROWTH
        } else {
            base
        }
    }

    fn part_transform(&self, part: Part) -> DisplayTransform {
        part.transform(self.part_scale(part))
    }

    /// How far over the ground the player's marker is aimed at, in blocks of a build.
    fn you_lift(&self) -> f64 {
        if self.looks.me.has_head() {
            MINI_HEAD_LIFT
        } else {
            0.0
        }
    }

    fn sign_scale(&self) -> f32 {
        SIGN_SCALE * self.marker_scale()
    }

    /// The way between the view and the player's marker, close up, in the middle of the
    /// screen. Nothing to fly to with no marker: the view holds where it is.
    fn way_to_marker(&self, length: u32) -> Flight {
        let eye = self.eye();
        let build = f64::from(self.model().scale().build);
        let rest = [self.look.yaw, self.look.pitch];
        match &self.pins.you {
            Some(you) => Flight::dive(eye, rest, you.at, DIVE_STOP * build, length),
            None => Flight::new(eye, eye, rest, length),
        }
    }

    fn place_pan(&mut self, atlas: &Atlas) {
        if let (MapMode::Pan, Some(level)) = (self.mode, atlas.level(Model::Detail)) {
            self.pan = self.pan_start(&level.scene);
            self.pan.drive(self.looks.motion, self.looks.inertia);
        }
    }

    /// Where a pan starts: near the player, with their marker off the middle.
    fn pan_start(&self, scene: &Scene) -> Pan {
        let Some(you) = &self.pins.you else {
            return Pan::default();
        };
        let [x, _, z] = you.at;
        let z = z - PAN_START_NORTH * self.zoom.height();
        match scene.top(x.floor() as i32, z.floor() as i32) {
            Some(_) => Pan::at([x, z]),
            None => Pan::default(),
        }
    }
}

fn on_land(scene: &Scene, [x, z]: [f64; 2]) -> bool {
    scene.top(x.floor() as i32, z.floor() as i32).is_some()
}

/// What `players` needs of a session.
pub(crate) struct MapView<'a> {
    pub scene: &'a Scene,
    /// The centre of the model, in the blocks of the level.
    pub origin: [f64; 2],
    /// On the model.
    pub eye: [f64; 3],
    /// Where the middle of the screen points, when no marker has the look.
    pub aim: Option<[f64; 3]>,
}

impl MapSession {
    pub(crate) fn view<'a>(&self, atlas: &'a Atlas) -> Option<MapView<'a>> {
        let Position { x, z, .. } = self.on_model(0.0, 0.0, 0.0);
        let level = atlas.level(self.model())?;
        let eye = self.eye();
        let direction = look_direction(self.look.yaw, self.look.pitch);
        let free = self.rig.is_some()
            && self.travel.is_none()
            && self.hovered.is_none()
            && !marker_points(level, self)
                .into_iter()
                .any(|point| near_marker(eye, direction, point));
        Some(MapView {
            scene: &level.scene,
            origin: [x, z],
            eye,
            aim: free.then_some(direction),
        })
    }
}

impl Atlas {
    /// The waypoints, in blocks of the world.
    pub(crate) fn places(&self) -> Option<Vec<[i32; 2]>> {
        let waypoints = &self.detail.as_ref()?.waypoints;
        Some(waypoints.iter().map(|w| [w.marker.x, w.marker.z]).collect())
    }

    /// Whether a walker can be on this block of the world. The detailed model says so:
    /// the others, drawn coarser, only show where it stands.
    pub(crate) fn walkable(&self, x: i32, z: i32) -> bool {
        self.detail.as_ref().is_some_and(|level| {
            let cell = level.scene.cell_of(x, z);
            cell.is_some_and(|(x, z)| level.scene.walkable(x, z))
        })
    }
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        let info = biome::register(&mut app.world_mut().resource_mut::<RegistryDataStore>());
        app.insert_resource(Atlas {
            info,
            regions: Regions::demo(),
            overview: None,
            middle: None,
            detail: None,
        })
        .add_systems(Startup, build_atlas)
        .add_systems(
            Update,
            (
                rig_up,
                stream_chunks,
                follow_view,
                tick_sessions,
                fly,
                tick_passages,
                run_wheels,
            ),
        )
        .add_observer(rise)
        .add_observer(close)
        .add_observer(switch_mode)
        .add_observer(leave)
        .add_observer(look_through_camera)
        .add_observer(read_mouse)
        .add_observer(read_mouse_with_position)
        .add_observer(hold_still)
        .add_observer(left_click)
        .add_observer(right_click)
        .add_observer(read_keys)
        .add_observer(note_pack)
        .add_observer(choose_look)
        .add_observer(player_left)
        .add_plugins((PlayersPlugin, CursorPlugin));
        app.world_mut()
            .resource_mut::<CommandRegistry>()
            .register(map_command());
    }
}

fn build_atlas(mut atlas: ResMut<Atlas>, generator: Res<WorldGen>) {
    let generator = generator.0.as_ref();
    let overview = Level::new(generator, OVERVIEW, &atlas.regions);
    let middle = Level::new(generator, MIDDLE, &atlas.regions);
    let detail = Level::new(generator, DETAIL, &atlas.regions);
    tracing::info!(waypoints = overview.waypoints.len(), "map models built");
    let (mut overview, mut middle, mut detail) = (overview, middle, detail);
    let started = Instant::now();
    overview.prepare(atlas.info);
    middle.prepare(atlas.info);
    detail.prepare(atlas.info);
    tracing::info!(
        took_ms = started.elapsed().as_millis(),
        "map chunks encoded"
    );
    atlas.overview = Some(overview);
    atlas.middle = Some(middle);
    atlas.detail = Some(detail);
}

/// Asks for a player to enter the map, or to change mode if they are in it.
#[derive(Event, Debug, Clone, Copy)]
pub struct OpenMap(pub Entity, pub MapMode);

/// The veil is on: the world takes over.
#[derive(Event, Debug, Clone, Copy)]
struct LeaveMap(Entity);

/// Asks for a player to leave the map.
#[derive(Event, Debug, Clone, Copy)]
pub struct CloseMap(pub Entity);

fn display(scale: f32) -> Display {
    Display::default()
        .transform(DisplayTransform::default().uniform_scale(scale))
        .billboard(Billboard::Center)
        .brightness(15, 15)
        .view_range(VIEW_RANGE)
        .glow_color(GLOW)
}

fn spawn_icon(commands: &mut Commands, at: Position, item: ItemStack, scale: f32) -> Entity {
    spawn_item(commands, at, item, ItemDisplayContext::Gui, scale)
}

fn spawn_item(
    commands: &mut Commands,
    at: Position,
    item: ItemStack,
    context: ItemDisplayContext,
    scale: f32,
) -> Entity {
    EntityBuilder::new(EntityKind::ItemDisplay)
        .position(at)
        .with_bundle((display(scale), ItemDisplay::new(item).context(context)))
        .spawn(commands)
        .id()
}

fn spawn_mark(commands: &mut Commands, at: Position, text: String, display: Display) -> Entity {
    EntityBuilder::new(EntityKind::TextDisplay)
        .position(at)
        .with_bundle((display, TextDisplay::new(text).background_color(CLEAR)))
        .spawn(commands)
        .id()
}

/// The player's head. Only an id goes out, so the client looks up the skin itself: an
/// offline account has none, and shows the default skin of its id.
fn player_head(id: Option<&PlayerUuid>) -> ItemStack {
    let head = item("minecraft:player_head");
    let Some(id) = id else {
        return head;
    };
    // An unresolved profile with no name, then an empty list of properties and no skin
    // patch, whose four parts are all optional.
    let mut body = vec![0, 0, 1];
    body.extend_from_slice(id.0.as_bytes());
    body.extend_from_slice(&[0, 0, 0, 0, 0]);
    head.with_component(DataComponent::Raw {
        type_id: component_ids::PROFILE,
        body,
    })
}

/// A card, moved in the plane of the screen so that it stays on top of its marker
/// wherever the view looks at it from.
fn spawn_card(
    commands: &mut Commands,
    at: Position,
    text: TextDisplay,
    scale: f32,
    lift: f32,
) -> Entity {
    let transform = DisplayTransform::default()
        .translation(0.0, lift, 0.0)
        .uniform_scale(scale);
    EntityBuilder::new(EntityKind::TextDisplay)
        .position(at)
        .with_bundle((display(scale).transform(transform), text))
        .spawn(commands)
        .id()
}

fn waypoint_card(atlas: &Atlas, waypoint: &Waypoint, player: &Position) -> Card {
    let marker = &waypoint.marker;
    let region = atlas.regions.at(f64::from(marker.x), f64::from(marker.z));
    let distance = (f64::from(marker.x) - player.x).hypot(f64::from(marker.z) - player.z) as i64;
    let state = match region.state {
        RegionState::Discovered => "§a",
        RegionState::Undiscovered => "§7",
        RegionState::Locked => "§c",
    };
    let action = match region.state {
        RegionState::Discovered => "Clic gauche : sélectionner",
        RegionState::Undiscovered | RegionState::Locked => "Voyage impossible",
    };
    Card {
        subject: Subject::Place(marker.kind),
        title: marker.name.clone(),
        kind: marker.kind.label().into(),
        kicker: format!("{} - {distance} blocs", marker.kind.label()),
        info: format!("{} · {distance} blocs", marker.kind.label()),
        status: format!("{state}{}", region.name),
        description: marker.kind.description().into(),
        action: action.into(),
        open: false,
    }
}

fn you_card() -> Card {
    Card {
        subject: Subject::You,
        title: "Vous êtes ici".into(),
        kind: "Vous".into(),
        kicker: "Votre position".into(),
        info: "Votre position".into(),
        status: String::new(),
        description: String::new(),
        action: String::new(),
        open: false,
    }
}

/// How a client shows a card, with the pack or without.
fn card_display(text: String, drawn: bool, style: Style) -> TextDisplay {
    let background = if drawn { CLEAR } else { PLAIN_CARD_BACKGROUND };
    let display = TextDisplay::new(text)
        .line_width(CARD_LINE_WIDTH)
        .background_color(background)
        .see_through();
    if style.has_background() {
        display
    } else {
        display.shadow()
    }
}

/// Puts the markers of the session's mode on the model.
fn spawn_pins(atlas: &Atlas, session: &mut MapSession, commands: &mut Commands) {
    let Some(level) = atlas.level(session.model()) else {
        return;
    };
    let build = f64::from(session.model().scale().build);
    let mut icons = Vec::with_capacity(level.waypoints.len());
    for waypoint in &level.waypoints {
        let [x, y, z] = waypoint.at;
        let at = session.on_model(x, y + ICON_LIFT * build, z);
        let scale = ICON_SCALE * session.marker_scale();
        icons.push(spawn_icon(commands, at, icon(waypoint.marker.kind), scale));
    }
    let you = model_spot(level, &session.position).map(|at| You {
        parts: spawn_you(session, at, build, commands),
        at,
    });
    let signs = spawn_signs(level, session, build, commands);
    session.pins = Pins {
        icons,
        you,
        signs,
        card: None,
    };
}

/// Draws the player's marker again, and nothing else of the map.
fn respawn_you(session: &mut MapSession, commands: &mut Commands) {
    let Some(you) = session.pins.you.take() else {
        return;
    };
    for (entity, _) in you.parts {
        commands.entity(entity).try_despawn();
    }
    let build = f64::from(session.model().scale().build);
    session.pins.you = Some(You {
        parts: spawn_you(session, you.at, build, commands),
        at: you.at,
    });
}

/// Draws the signs of the closed regions again, and nothing else of the map.
fn respawn_signs(atlas: &Atlas, session: &mut MapSession, commands: &mut Commands) {
    for entity in std::mem::take(&mut session.pins.signs) {
        commands.entity(entity).try_despawn();
    }
    if let Some(level) = atlas.level(session.model()) {
        let build = f64::from(session.model().scale().build);
        session.pins.signs = spawn_signs(level, session, build, commands);
    }
}

/// The entities of the player's marker, as they chose it.
fn spawn_you(
    session: &MapSession,
    [x, y, z]: [f64; 3],
    build: f64,
    commands: &mut Commands,
) -> Vec<(Entity, Part)> {
    let text = |mark: Mark| mark.text(session.drawn);
    let drawn = |part: Part| display(1.0).transform(session.part_transform(part));
    let ground = session.on_model(x, y + GROUND_MARK_LIFT * build, z);
    let lying = |mark: Mark, yaw: f32, commands: &mut Commands| {
        let part = Part::Flat(mark, yaw);
        let display = drawn(part).billboard(Billboard::Fixed);
        (spawn_mark(commands, ground, text(mark), display), part)
    };
    let with_head = |commands: &mut Commands, behind: &[Part]| {
        let at = session.on_model(x, y + MINI_HEAD_LIFT * build, z);
        let mut parts: Vec<_> = behind
            .iter()
            .map(|part| {
                let mark = match part {
                    Part::Tip(_) => Mark::Tip,
                    _ => Mark::Disc,
                };
                (spawn_mark(commands, at, text(mark), drawn(*part)), *part)
            })
            .collect();
        let head = spawn_item(
            commands,
            at,
            session.head.clone(),
            // Facing the camera: as an icon, a head shows its hair more than its face.
            ItemDisplayContext::Fixed,
            session.part_scale(Part::Head),
        );
        parts.push((head, Part::Head));
        parts
    };
    match session.looks.me {
        Me::Pin => {
            let at = session.on_model(x, y, z);
            let entity = spawn_mark(commands, at, text(Mark::Pin), drawn(Part::Pin));
            vec![(entity, Part::Pin)]
        }
        Me::Arrow => vec![lying(Mark::Arrow, session.rotation.yaw, commands)],
        Me::Ring => vec![lying(Mark::Ring, 180.0, commands)],
        Me::MiniHead => with_head(commands, &[Part::Disc]),
        Me::HeadTip => with_head(commands, &[Part::Tip(HEAD_HALF * MINI_HEAD_SCALE)]),
        Me::Bubble => with_head(commands, &[Part::Disc, Part::Tip(DISC_RADIUS)]),
        Me::HeadRing => {
            let mut parts = with_head(commands, &[]);
            parts.push(lying(Mark::Ring, 180.0, commands));
            parts
        }
        Me::HeadHeading => {
            let mut parts = with_head(commands, &[]);
            parts.push(lying(Mark::Arrow, session.rotation.yaw, commands));
            parts
        }
    }
}

/// What stands over each closed region, for the looks that say it with a sign.
fn spawn_signs(
    level: &Level,
    session: &MapSession,
    build: f64,
    commands: &mut Commands,
) -> Vec<Entity> {
    let mark = match session.looks.zone {
        ZoneLook::Palisade => Mark::Lock,
        ZoneLook::Faded => Mark::Unknown,
        ZoneLook::Clouds | ZoneLook::Hatching => return Vec::new(),
    };
    level
        .signs
        .iter()
        .map(|([x, y, z], state)| {
            let at = session.on_model(*x, y + SIGN_LIFT * build, *z);
            let text = sign(mark, state.label(), session.drawn);
            spawn_mark(commands, at, text, display(session.sign_scale()))
        })
        .collect()
}

fn model_spot(level: &Level, position: &Position) -> Option<[f64; 3]> {
    let scene = &level.scene;
    let (x, z) = scene.cell_of(position.x as i32, position.z as i32)?;
    let top = scene.top(x, z)?;
    Some([f64::from(x) + 0.5, f64::from(top + 1), f64::from(z) + 0.5])
}

type EntrantState<'a> = (
    &'a ClientId,
    &'a LoadedChunks,
    &'a Position,
    &'a Rotation,
    Option<&'a PlayerAbilities>,
    Option<&'a mut TabEntry>,
    Option<&'a PlayerUuid>,
    &'a MinecraftEntityId,
    Option<&'a MapLooks>,
    Has<PackLoaded>,
);

/// A player whose place is no longer changing.
type Settled = (Without<Teleport>, Without<Passage>);

/// A player with no map on and none on the way.
type InTheWorld = (Without<MapSession>, Without<Passage>);

type PassageState<'a> = (
    Entity,
    &'a mut Passage,
    &'a MinecraftEntityId,
    Has<PackLoaded>,
    &'a LoadedChunks,
    Option<&'a mut MapSession>,
    Option<&'a mut TabEntry>,
);

/// Sets a player off towards the map. Their camera rises through the world while the map
/// comes in behind it, and they get to the top with nothing left to wait for.
fn rise(
    event: On<OpenMap>,
    mut states: Query<EntrantState, InTheWorld>,
    atlas: Res<Atlas>,
    players: Players,
    mut commands: Commands,
) {
    let (player, mode) = (event.0, event.1);
    let Ok((client, loaded, position, rotation, abilities, tab, uuid, _, looks, drawn)) =
        states.get_mut(player)
    else {
        return;
    };
    if atlas.level(Model::Overview).is_none() || atlas.level(Model::Detail).is_none() {
        return;
    }
    tracing::info!(?player, ?mode, "rising to the map");
    // The client asks this entry, not `Respawn`, whether it is a spectator.
    let tab_game_mode = tab.and_then(|mut entry| entry.game_mode.replace(SPECTATOR));
    let site = Site::of(client.0);
    players.send(
        player,
        SetChunkCacheRadius {
            radius: site.radius,
        },
    );
    players.send(
        player,
        SetCenterChunk {
            chunk_x: site.centre[0],
            chunk_z: site.centre[1],
        },
    );
    // Out of the square, the client lets go of them anyway.
    let (world, gone): (HashSet<_>, HashSet<_>) = loaded
        .0
        .iter()
        .copied()
        .partition(|chunk| site.holds(*chunk));
    for chunk in gone {
        players.send(
            player,
            UnloadChunk {
                chunk_x: chunk.x,
                chunk_z: chunk.z,
            },
        );
    }
    // A spectator has no hand and no hotbar, all the way there and back.
    players.send(
        player,
        GameEvent {
            event: GameEventType::ChangeGameMode,
            value: f32::from(SPECTATOR),
        },
    );

    let bar = commands
        .spawn(
            BossBar::new(mode.label())
                .color(BossBarColor::Green)
                .viewers([player]),
        )
        .id();
    let mut session = MapSession {
        mode,
        site,
        position: *position,
        rotation: *rotation,
        abilities: abilities.copied(),
        tab_game_mode,
        pins: Pins::default(),
        bar,
        rig: None,
        mouse: Mouse::default(),
        pan: Pan::default(),
        zoom: Zoom::default(),
        pan_model: Model::Detail,
        sized_for: Zoom::default().height(),
        free: FreeLook::default(),
        telemetry: Telemetry::default(),
        head: player_head(uuid),
        looks: looks.copied().unwrap_or_default(),
        drawn,
        painted: looks.copied().unwrap_or_default().chunks(),
        behind: HashMap::new(),
        world: world.clone(),
        heading: None,
        camera: None,
        travel: None,
        landing: None,
        veiled: None,
        ready: false,
        wheel: None,
        told: 0,
        sneaking: false,
        keys: [false; 3],
        look: Rotation {
            yaw: REST_YAW,
            pitch: REST_PITCH,
        },
        hovered: None,
        selected: None,
        streamed: (0, 0),
        ticks: 0,
        last_click: 0,
    };
    spawn_pins(&atlas, &mut session, &mut commands);
    session.place_pan(&atlas);
    session.look = session.rest();
    let opening = session.way_to_marker(OPENING_TICKS).reversed();
    let [yaw, pitch] = opening.looking();
    let look = Rotation { yaw, pitch };
    let [x, y, z] = session.vehicle_at(opening.at());
    let at = session.on_model(x, y, z);
    session.camera = Some(match mode {
        MapMode::Look => {
            let eye = spawn_eye_camera(&mut commands, at, look);
            (eye.vehicle, Some(eye.eye))
        }
        MapMode::Pan => (
            spawn_camera(&mut commands, at, look, FLIGHT_GLIDE_TICKS),
            None,
        ),
    });
    session.travel = Some(Travel {
        flight: opening,
        ending: Ending::View,
    });

    let flight = Flight::rise(eye_of(position), *rotation);
    let camera = spawn_eye_camera(&mut commands, under_eye(eye_of(position)), *rotation);
    commands.entity(camera.vehicle).insert(flight);
    commands
        .entity(player)
        .remove::<ChunkStreamBacklog>()
        .insert((
            session,
            ChunkStreamingPaused,
            ServerControlledPosition,
            LoadedChunks(world),
            PlayerAbilities::new()
                .invulnerable(true)
                .allow_flight(true)
                .flying(true)
                .flying_speed(0.0)
                .walking_speed(UNWIDENED_WALKING_SPEED),
            Passage {
                camera,
                kind: PassageKind::Rise,
                ticks: 0,
                back: None,
            },
        ));
}

/// Where a camera goes for what rides it to see from an eye.
fn under_eye(eye: Position) -> Position {
    Position {
        y: eye.y - RIDER_EYE_HEIGHT,
        ..eye
    }
}

fn eye_of(position: &Position) -> Position {
    Position {
        y: position.y + SPECTATOR_EYE_HEIGHT,
        ..*position
    }
}

/// Moves the cameras of the passages. `Players` reads `Position`, so it stays out of this.
fn fly(mut flights: Query<(&mut Flight, &mut Position, &mut Rotation)>) {
    for (mut flight, mut position, mut rotation) in &mut flights {
        flight.tick();
        let [x, y, z] = flight.target();
        position.set_if_neq(under_eye(Position { x, y, z }));
        let [yaw, pitch] = flight.looking();
        rotation.set_if_neq(Rotation { yaw, pitch });
    }
}

/// The yaw a client gives what it sees through, which only ever gets it rounded down to a
/// step of 1.4 degrees. The eye taking over starts from there, not to turn all of a sudden.
fn yaw_as_shown(yaw: f32) -> f32 {
    const STEP: f32 = 360.0 / 256.0;
    (yaw.rem_euclid(360.0) / STEP).floor() * STEP
}

/// The exact place and look a camera ends a flight on. The engine sends a small move with
/// its angles rounded to 1.4 degrees, which would show when the eye takes over.
fn settled(camera: &MinecraftEntityId, at: Position, [yaw, pitch]: [f32; 2]) -> EntityPositionSync {
    EntityPositionSync {
        entity_id: camera.0,
        x: at.x,
        y: at.y,
        z: at.z,
        vx: 0.0,
        vy: 0.0,
        vz: 0.0,
        yaw,
        pitch,
        on_ground: false,
    }
}

fn send_veil(players: &Players, player: Entity) {
    for packet in veil_in() {
        players.send(player, packet);
    }
}

fn tick_passages(
    mut passages: Query<PassageState>,
    ids: Query<&MinecraftEntityId>,
    flights: Query<&Flight>,
    config: Res<ServerConfigResource>,
    players: Players,
    mut commands: Commands,
) {
    for (player, mut passage, id, drawn, loaded, session, tab) in &mut passages {
        passage.ticks += 1;
        let ticks = passage.ticks;
        match passage.kind {
            PassageKind::Rise => {
                if drawn && ticks == RISE_TICKS - VEIL_FADE {
                    send_veil(&players, player);
                }
                let Some(mut session) = session else {
                    continue;
                };
                if ticks == TURN_TICKS
                    && let (Ok(eye), Ok(flight)) = (
                        ids.get(passage.camera.eye),
                        flights.get(passage.camera.vehicle),
                    )
                {
                    let [x, y, z] = flight.end();
                    let at = Position { x, y, z };
                    players.send(player, settled(eye, at, flight.looking()));
                }
                if ticks == SWAP_TICKS
                    && let Ok(eye) = ids.get(passage.camera.eye)
                {
                    players.send(player, SetCamera { camera_id: eye.0 });
                }
                let ahead = session
                    .camera
                    .and_then(|(vehicle, eye)| ids.get(eye.unwrap_or(vehicle)).ok());
                // What is waiting on the model gets its look too, while nobody sees.
                if ticks == RISE_TICKS - TURN_TICKS
                    && let (Some(eye), Some(travel)) = (ahead, session.travel)
                {
                    let [x, y, z] = travel.flight.at();
                    let at = session.on_model(x, y, z);
                    players.send(player, settled(eye, at, travel.flight.looking()));
                }
                if ticks >= RISE_TICKS && session.ready {
                    if let Some(camera) = ahead {
                        players.send(
                            player,
                            SetCamera {
                                camera_id: camera.0,
                            },
                        );
                    }
                    tracing::info!(ticks, "over the map");
                    let home = session.home_position();
                    let rest = session.look;
                    session.veiled = drawn.then_some(0);
                    despawn_eye_camera(passage.camera, &mut commands);
                    commands.entity(player).remove::<Passage>().insert(
                        Teleport::to(home.x, home.y, home.z)
                            .facing(rest.yaw, rest.pitch)
                            .unthrottled()
                            .preload_radius(0),
                    );
                }
            }
            PassageKind::Landing(flight) => {
                let Some(back) = passage.back else {
                    continue;
                };
                let near = back.home.chunks_in_radius(NEAR_WORLD_CHUNKS);
                let whole = near.iter().all(|chunk| loaded.0.contains(chunk));
                if ticks == TURN_TICKS
                    && let Ok(eye) = ids.get(passage.camera.eye)
                {
                    let [x, y, z] = flight.at();
                    players.send(player, settled(eye, Position { x, y, z }, flight.looking()));
                }
                if ticks == LANDING_HOLD_TICKS
                    && let Ok(eye) = ids.get(passage.camera.eye)
                {
                    players.send(player, SetCamera { camera_id: eye.0 });
                }
                let held = ticks > LANDING_HOLD_TICKS;
                if (whole && held) || ticks >= LANDING_PATIENCE_TICKS {
                    tracing::info!(ticks, whole, "world back under the veil");
                    // Again, for a place that only just came and showed the camera.
                    if let Ok(eye) = ids.get(passage.camera.eye) {
                        players.send(player, SetCamera { camera_id: eye.0 });
                    }
                    if drawn {
                        players.send(player, veil_out());
                    }
                    commands.entity(passage.camera.vehicle).try_insert(flight);
                    passage.kind = PassageKind::Descent;
                    passage.ticks = 0;
                }
            }
            PassageKind::Descent => {
                let Ok(flight) = flights.get(passage.camera.vehicle) else {
                    continue;
                };
                let Some(back) = passage.back else {
                    continue;
                };
                if flight.settling()
                    && let Ok(camera) = ids.get(passage.camera.vehicle)
                {
                    let [x, y, z] = flight.end();
                    let at = under_eye(Position { x, y, z });
                    players.send(player, settled(camera, at, flight.looking()));
                }
                // A speed of its own for the client to go by when it becomes a player again:
                // that view takes over on the next tick, and has to be no wider than this one.
                if flight.left() == 1 {
                    commands.entity(player).insert(
                        PlayerAbilities::new()
                            .invulnerable(true)
                            .allow_flight(true)
                            .flying(true)
                            .flying_speed(0.0),
                    );
                }
                if flight.left() == 0 {
                    let [yaw, pitch] = flight.looking();
                    players.send(
                        player,
                        PlayerRotation {
                            yaw: yaw_as_shown(yaw),
                            relative_yaw: false,
                            pitch,
                            relative_pitch: false,
                        },
                    );
                    players.send(
                        player,
                        GameEvent {
                            event: GameEventType::ChangeGameMode,
                            value: f32::from(config.game_mode),
                        },
                    );
                    players.send(player, SetCamera { camera_id: id.0 });
                    despawn_eye_camera(passage.camera, &mut commands);
                    let mut landed = commands.entity(player);
                    landed.remove::<(Passage, ServerControlledPosition)>();
                    match back.abilities {
                        Some(abilities) => landed.insert(abilities),
                        None => landed.remove::<PlayerAbilities>(),
                    };
                    if let Some(mut entry) = tab {
                        entry.game_mode = back.tab_game_mode.or(Some(config.game_mode));
                    }
                }
            }
        }
    }
}

fn despawn_pins(session: &mut MapSession, commands: &mut Commands) {
    let pins = std::mem::take(&mut session.pins);
    let entities = pins
        .icons
        .into_iter()
        .chain(
            pins.you
                .into_iter()
                .flat_map(|you| you.parts.into_iter().map(|(entity, _)| entity)),
        )
        .chain(pins.signs)
        .chain(pins.card.map(|card| card.entity));
    for entity in entities {
        commands.entity(entity).try_despawn();
    }
}

fn despawn_rig(session: &mut MapSession, commands: &mut Commands) {
    if let Some(rig) = session.rig.take() {
        for entity in rig.camera.into_iter().chain(rig.eye).chain([rig.eye_box]) {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Swaps the model under the player. `rig_up` finishes the job once they are in place.
fn switch_mode(
    event: On<OpenMap>,
    mut sessions: Query<(&mut MapSession, &MinecraftEntityId, &LoadedChunks)>,
    atlas: Res<Atlas>,
    players: Players,
    mut commands: Commands,
) {
    let (player, mode) = (event.0, event.1);
    let Ok((mut session, id, loaded)) = sessions.get_mut(player) else {
        return;
    };
    if session.mode == mode || session.travel.is_some() || session.rig.is_none() {
        return;
    }
    tracing::info!(?mode, "map mode");
    despawn_rig(&mut session, &mut commands);
    despawn_pins(&mut session, &mut commands);
    players.send(player, SetCamera { camera_id: id.0 });
    for chunk in loaded.0.difference(&session.world) {
        players.send(
            player,
            UnloadChunk {
                chunk_x: chunk.x,
                chunk_z: chunk.z,
            },
        );
    }
    session.mode = mode;
    session.pan = Pan::default();
    session.mouse = Mouse::default();
    session.zoom = Zoom::default();
    session.pan_model = Model::Detail;
    session.sized_for = session.zoom.height();
    session.free = FreeLook::default();
    session.look = session.rest();
    session.hovered = None;
    session.selected = None;
    spawn_pins(&atlas, &mut session, &mut commands);
    session.place_pan(&atlas);
    let home = session.home_position();
    let rest = session.look;
    commands.entity(player).insert((
        LoadedChunks(session.world.clone()),
        Teleport::to(home.x, home.y, home.z)
            .facing(rest.yaw, rest.pitch)
            .unthrottled()
            .preload_radius(0),
    ));
}

fn chunks_around(mode: MapMode, camera: (i32, i32)) -> Vec<(i32, i32)> {
    let (centre, reach) = match mode {
        MapMode::Look => ((0, 0), OVERVIEW_REACH),
        MapMode::Pan => (camera, DETAIL_SEND_RADIUS),
    };
    let mut chunks: Vec<_> = (centre.0 - reach..=centre.0 + reach)
        .flat_map(|x| (centre.1 - reach..=centre.1 + reach).map(move |z| (x, z)))
        .filter(|(x, z)| match mode {
            MapMode::Look => *x < reach && *z < reach,
            MapMode::Pan => (x - camera.0).pow(2) + (z - camera.1).pow(2) <= reach * reach,
        })
        .collect();
    chunks.sort_by_key(|(x, z)| (x - camera.0).pow(2) + (z - camera.1).pow(2));
    chunks
}

/// Moves a pan from one model to the other, once its height calls for it. The client
/// already has the chunks and the camera is far enough to jump, so it takes one frame.
fn switch_model(atlas: &Atlas, session: &mut MapSession, commands: &mut Commands) {
    let height = session.zoom.height();
    let model = session.pan_model.at_height(height);
    if session.mode == MapMode::Look || model == session.pan_model {
        return;
    }
    tracing::debug!(?model, height, "map model");
    session.pan_model = model;
    session.hovered = None;
    session.selected = None;
    despawn_pins(session, commands);
    spawn_pins(atlas, session, commands);
    session.sized_for = height;
}

/// The real world, sent ahead of a player who may be going there. What the engine does for
/// a player who walks, for one who stands elsewhere. They are told about what goes on
/// there from then on, as the chunks count as theirs, and go without it once they land.
#[derive(SystemParam)]
struct WorldChunks<'w, 's> {
    index: ResMut<'w, ChunkIndex>,
    chunks: Query<'w, 's, (&'static ChunkPosition, &'static ChunkData)>,
    generator: Res<'w, WorldGen>,
    loader: Option<Res<'w, ChunkLoaderResource>>,
    registries: Option<Res<'w, RegistryDataStore>>,
    config: Res<'w, ServerConfigResource>,
}

impl WorldChunks<'_, '_> {
    fn packet(
        &mut self,
        pos: ChunkPos,
        dimension: DimensionId,
        generated: &mut usize,
        commands: &mut Commands,
    ) -> Option<ChunkDataAndLight> {
        let key = (dimension, pos);
        if let Some(entity) = self.index.0.get(&key) {
            let (at, chunk) = self.chunks.get(*entity).ok()?;
            return Some(chunk.to_packet(at.0.x, at.0.z));
        }
        if *generated >= self.config.max_chunk_generations_per_tick {
            return None;
        }
        *generated += 1;
        let loader = self.loader.as_deref();
        let mut chunk = load_or_generate(loader, &self.generator, dimension, &pos);
        if let Some(registries) = &self.registries {
            chunk.normalize_biomes(registries.biome_count());
        }
        let packet = chunk.to_packet(pos.x, pos.z);
        let entity = commands
            .spawn((ChunkPosition(pos), chunk, ChunkDimension(dimension)))
            .id();
        self.index.0.insert(key, entity);
        Some(packet)
    }
}

fn stream_chunks(
    mut sessions: Query<(Entity, &mut MapSession, &LoadedChunks, &PlayerDimension)>,
    mut atlas: ResMut<Atlas>,
    mut world: WorldChunks,
    players: Players,
    mut commands: Commands,
) {
    let info = atlas.info;
    for (player, mut session, loaded, dimension) in &mut sessions {
        let started = Instant::now();
        switch_model(&atlas, &mut session, &mut commands);
        let look = session.looks.chunks();
        let model = session.model();
        let wanted = session.wanted();
        // `Players` reads this component, so it is replaced rather than changed here.
        let mut now_loaded = loaded.0.clone();
        repaint(&mut atlas, &mut session, &loaded.0, player, &players);

        let missing: Vec<_> = wanted
            .into_iter()
            .filter(|(model, at)| !now_loaded.contains(&session.placed(*model, *at)))
            .collect();
        let here = missing.iter().filter(|(on, _)| *on == model).count();
        let [x, _, z] = session.standing();
        let camera = ((x / 16.0).floor() as i32, (z / 16.0).floor() as i32);
        session.ready = !missing.iter().any(|(on, (x, z))| {
            let near = (x - camera.0).abs().max((z - camera.1).abs()) <= NEAR_MODEL_CHUNKS;
            *on == model && near
        });
        let budget = if here > 0 {
            CHUNKS_PER_TICK
        } else {
            NEIGHBOUR_CHUNKS_PER_TICK
        };
        let (mut sent, mut weight_sent) = (0, 0);
        for (on, at) in missing.iter().take(budget) {
            let Some(level) = atlas.level_mut(*on) else {
                continue;
            };
            let (chunk, weight) = level.chunk(*at, look, info);
            let to = session.placed(*on, *at);
            let mut placed = chunk.clone();
            (placed.chunk_x, placed.chunk_z) = (to.x, to.z);
            weight_sent += *weight;
            sent += 1;
            players.send(player, placed);
            now_loaded.insert(to);
        }
        if sent > 0 {
            session.streamed.0 += sent;
            session.streamed.1 += weight_sent;
            session.telemetry.sent(sent, weight_sent, started.elapsed());
        }
        if missing.len() <= budget && session.streamed.0 > 0 {
            let (chunks, bytes) = std::mem::take(&mut session.streamed);
            tracing::info!(
                mode = ?session.mode,
                chunks,
                bytes,
                loaded = now_loaded.len(),
                "map chunks sent"
            );
        }
        let ahead = send_world_ahead(
            &mut session,
            &mut now_loaded,
            dimension.0,
            &mut world,
            (player, &players),
            &mut commands,
        );
        if !missing.is_empty() || ahead {
            commands.entity(player).insert(LoadedChunks(now_loaded));
        }
    }
}

/// Brings the chunks the client holds to the player's look, as blocks: the ones that
/// differ, around the view first. Nothing leaves the client, and stepping through looks
/// faster than this goes only sends what ends up different.
fn repaint(
    atlas: &mut Atlas,
    session: &mut MapSession,
    loaded: &HashSet<ChunkPos>,
    player: Entity,
    players: &Players,
) {
    let look = session.looks.chunks();
    if session.painted != look {
        let was = std::mem::replace(&mut session.painted, look);
        for chunk in loaded.difference(&session.world) {
            session.behind.entry(*chunk).or_insert(was);
        }
        session.behind.retain(|_, from| *from != look);
    }
    session.behind.retain(|chunk, _| loaded.contains(chunk));
    if session.behind.is_empty() {
        return;
    }
    let [x, _, z] = session.standing();
    let (centre, own) = (((x / 16.0) as i32, (z / 16.0) as i32), session.model());
    let mut order: Vec<_> = session
        .behind
        .iter()
        .filter_map(|(chunk, from)| {
            let (model, at) = session.unplaced(*chunk)?;
            let away = (at.0 - centre.0).pow(2) + (at.1 - centre.1).pow(2);
            Some(((model != own, away), model, at, *chunk, *from))
        })
        .collect();
    order.sort_by_key(|(rank, ..)| *rank);
    let mut budget = REPAINTED_BLOCKS_PER_TICK;
    for (_, model, at, chunk, from) in order {
        let Some(level) = atlas.level_mut(model) else {
            continue;
        };
        for change in level.repaint(at, from, look) {
            budget = budget.saturating_sub(change.blocks.len());
            players.send(
                player,
                SectionBlocksUpdate {
                    section: SectionBlocksUpdate::section(chunk.x, change.section, chunk.z),
                    blocks: change.blocks.clone(),
                },
            );
        }
        session.behind.remove(&chunk);
        if budget == 0 {
            break;
        }
    }
}

/// Sends the next chunks of where the player is heading, with the closest first. Says
/// whether it sent any.
fn send_world_ahead(
    session: &mut MapSession,
    loaded: &mut HashSet<ChunkPos>,
    dimension: DimensionId,
    world: &mut WorldChunks,
    (player, players): (Entity, &Players),
    commands: &mut Commands,
) -> bool {
    let Some(heading) = session.heading else {
        return false;
    };
    let (site, reach) = (session.site, world.config.view_distance);
    let (mut sent, mut generated) = (0, 0);
    for chunk in heading.chunks_in_radius(reach) {
        if sent >= WORLD_CHUNKS_PER_TICK {
            break;
        }
        if loaded.contains(&chunk) || !site.holds(chunk) {
            continue;
        }
        let Some(packet) = world.packet(chunk, dimension, &mut generated, commands) else {
            break;
        };
        players.send(player, packet);
        loaded.insert(chunk);
        session.world.insert(chunk);
        sent += 1;
    }
    sent > 0
}

/// An entity to see through, which glides for this many ticks between two places.
fn spawn_camera(commands: &mut Commands, at: Position, look: Rotation, glide: u8) -> Entity {
    EntityBuilder::new(EntityKind::ItemDisplay)
        .position(at)
        .rotation(look.yaw, look.pitch)
        .with_bundle(Display::default().teleport_ticks(glide))
        .spawn(commands)
        .id()
}

fn spawn_eye_camera(commands: &mut Commands, at: Position, look: Rotation) -> EyeCamera {
    let body = ResolvableProfile::textured(EYE_SKIN, false);
    let eye = EntityBuilder::new(EntityKind::Mannequin)
        .position(at)
        .rotation(look.yaw, look.pitch)
        .gravity(false)
        .block_collision(false)
        .settle_ticks(0)
        .with_bundle((Mannequin::new(body), Invisible, NoGravity, Silent))
        .spawn(commands)
        .id();
    let vehicle = spawn_camera(commands, at, look, FLIGHT_GLIDE_TICKS);
    commands.entity(vehicle).insert(Passengers::new([eye]));
    EyeCamera { vehicle, eye }
}

fn despawn_eye_camera(camera: EyeCamera, commands: &mut Commands) {
    commands.entity(camera.eye).try_despawn();
    commands.entity(camera.vehicle).try_despawn();
}

/// Once the player stands in the level: a teleport would throw them off a vehicle.
fn rig_up(mut sessions: Query<(Entity, &mut MapSession), Settled>, mut commands: Commands) {
    for (player, mut session) in &mut sessions {
        if session.rig.is_some() {
            continue;
        }
        let eye_box = EntityBuilder::new(cursor::eye_box_kind(session.drawn))
            .position(session.eye_box_position())
            .gravity(false)
            .spawn(&mut commands)
            .id();
        // Riding the camera, the box glides with it exactly.
        let (waiting, eye) = session.camera.take().unzip();
        let camera = match session.mode {
            MapMode::Pan => {
                let at = session.camera_at();
                let camera = waiting.unwrap_or_else(|| {
                    spawn_camera(&mut commands, at, session.look, CAMERA_GLIDE_TICKS)
                });
                commands
                    .entity(camera)
                    .insert(Passengers::new([player, eye_box]));
                Some(camera)
            }
            MapMode::Look => waiting,
        };
        if session.mode == MapMode::Look && session.travel.is_some() {
            commands.entity(eye_box).insert(Hidden);
        }
        session.rig = Some(Rig {
            eye_box,
            camera,
            eye: eye.flatten(),
        });
    }
}

fn look_through_camera(
    event: On<EntityShownEvent>,
    sessions: Query<&MapSession>,
    passages: Query<&Passage>,
    ids: Query<&MinecraftEntityId>,
    players: Players,
) {
    // The others are taken over from when they have their exact look.
    let camera = match (sessions.get(event.viewer), passages.get(event.viewer)) {
        (Ok(session), Err(_)) => session
            .rig
            .filter(|rig| rig.eye.is_none())
            .and_then(|rig| rig.camera),
        _ => None,
    };
    if camera == Some(event.entity)
        && let Ok(id) = ids.get(event.entity)
    {
        players.send(event.viewer, SetCamera { camera_id: id.0 });
    }
}

fn read_mouse(
    event: On<PacketEvent<SetPlayerRotation>>,
    mut sessions: Query<&mut MapSession>,
    players: Players,
) {
    if let Ok(mut session) = sessions.get_mut(event.entity)
        && let Some(correction) = session.turn(event.packet.yaw, event.packet.pitch)
    {
        players.send(event.entity, correction);
    }
}

/// Every 20 ticks the client sends its position along with any rotation, in this
/// packet instead of the other. A look that stops on that tick is only reported here.
fn read_mouse_with_position(
    event: On<PacketEvent<SetPlayerPosAndRot>>,
    mut sessions: Query<&mut MapSession>,
    players: Players,
) {
    if let Ok(mut session) = sessions.get_mut(event.entity)
        && let Some(correction) = session.turn(event.packet.yaw, event.packet.pitch)
    {
        players.send(event.entity, correction);
    }
}

/// A spectator's scroll wheel changes their flight speed on the client alone.
fn hold_still(
    event: On<PacketEvent<SetPlayerPos>>,
    sessions: Query<&MapSession>,
    abilities: Query<&PlayerAbilities>,
    mut commands: Commands,
) {
    let Ok(session) = sessions.get(event.entity) else {
        return;
    };
    let home = session.home_position();
    // Getting off what the wheel rides on drops a client a little.
    let moved = (event.packet.x - home.x)
        .hypot(event.packet.z - home.z)
        .hypot(event.packet.y - home.y);
    let held = session.mode == MapMode::Look && session.rig.is_some() && session.travel.is_none();
    if held && moved > 0.01 {
        let mut player = commands.entity(event.entity);
        if let Ok(held) = abilities.get(event.entity) {
            player.insert(*held);
        }
        player.insert(Teleport::to(home.x, home.y, home.z).preload_radius(0));
    }
}

fn set_scale(displays: &mut Query<&mut Display>, entity: Entity, scale: f32) {
    if let Ok(mut display) = displays.get_mut(entity) {
        display.transform = DisplayTransform::default().uniform_scale(scale);
    }
}

/// Sizes a marker for the zoom and for whether the look is on it.
fn resize(session: &MapSession, marker: Hover, displays: &mut Query<&mut Display>) {
    match marker {
        Hover::Waypoint(index) => {
            set_scale(
                displays,
                session.pins.icons[index],
                session.icon_scale(index),
            );
        }
        Hover::You => {
            for (entity, part) in session.pins.you.iter().flat_map(|you| &you.parts) {
                if let Ok(mut display) = displays.get_mut(*entity) {
                    display.transform = session.part_transform(*part);
                }
            }
        }
    }
}

/// Where the look finds each marker, the player's own last.
fn marker_points(level: &Level, session: &MapSession) -> Vec<[f64; 3]> {
    let build = f64::from(session.model().scale().build);
    let icons = level
        .waypoints
        .iter()
        .map(|w| [w.at[0], w.at[1] + ICON_LIFT * build, w.at[2]]);
    let you = session
        .pins
        .you
        .iter()
        .map(|you| [you.at[0], you.at[1] + session.you_lift() * build, you.at[2]]);
    icons.chain(you).collect()
}

fn hover_of(level: &Level, session: &MapSession) -> Option<Hover> {
    let points = marker_points(level, session);
    let count = level.waypoints.len();
    let current = session.hovered.map(|hover| match hover {
        Hover::Waypoint(index) => index,
        Hover::You => count,
    });
    let direction = look_direction(session.look.yaw, session.look.pitch);
    let aimed = aimed(session.eye(), direction, points, current)?;
    Some(if aimed < count {
        Hover::Waypoint(aimed)
    } else {
        Hover::You
    })
}

/// What the panel talks about: what the look is on, or else what stays selected.
fn subject(session: &MapSession) -> Option<Hover> {
    session.hovered.or(session.selected.map(Hover::Waypoint))
}

fn card_of(atlas: &Atlas, level: &Level, session: &MapSession, hover: Hover) -> Card {
    match hover {
        Hover::Waypoint(index) => {
            let card = waypoint_card(atlas, &level.waypoints[index], &session.position);
            let open = session.selected == Some(index);
            let action = if open {
                TRAVEL_ACTION
            } else {
                card.action.as_str()
            };
            Card {
                action: action.into(),
                open,
                ..card
            }
        }
        Hover::You => you_card(),
    }
}

/// Where a marker's icon stands on the model, with how far up from there it reaches.
fn marker_top(level: &Level, session: &MapSession, hover: Hover) -> Option<([f64; 3], f32)> {
    let build = f64::from(session.model().scale().build);
    match hover {
        Hover::Waypoint(index) => {
            let [x, y, z] = level.waypoints[index].at;
            let scale = session.icon_scale(index);
            Some(([x, y + ICON_LIFT * build, z], scale * ICON_HALF))
        }
        Hover::You => {
            let [x, y, z] = session.pins.you.as_ref()?.at;
            let scale = session.marker_scale();
            match session.looks.me {
                Me::Pin => {
                    let height = Mark::Pin.height() as f32 / PIXELS_PER_BLOCK;
                    Some(([x, y, z], height * PIN_SCALE * scale))
                }
                Me::Arrow | Me::Ring => Some(([x, y, z], GROUND_MARK_SCALE * scale / 4.0)),
                Me::MiniHead | Me::HeadTip | Me::Bubble | Me::HeadRing | Me::HeadHeading => Some((
                    [x, y + MINI_HEAD_LIFT * build, z],
                    MINI_HEAD_SCALE * scale * HEAD_HALF * 2.0,
                )),
            }
        }
    }
}

/// Keeps the card of the current subject on screen, and only that one.
fn show_card(atlas: &Atlas, level: &Level, session: &mut MapSession, commands: &mut Commands) {
    let (style, drawn) = (session.looks.style, session.drawn);
    let wanted = subject(session).map(|hover| {
        let card = card_of(atlas, level, session, hover);
        let text = if drawn {
            card.drawn(style)
        } else {
            card.plain()
        };
        (hover, card.depth(style), text)
    });
    let zoom = session.zoom.height();
    let same = match (&session.pins.card, &wanted) {
        (Some(shown), Some((hover, _, text))) => {
            let sized = (zoom / shown.zoom - 1.0).abs() < CARD_ZOOM_STEP;
            shown.hover == *hover && sized && shown.text == *text && shown.style == style
        }
        (None, None) => true,
        _ => false,
    };
    if same {
        return;
    }
    if let Some(shown) = session.pins.card.take() {
        commands.entity(shown.entity).try_despawn();
    }
    let Some((hover, depth, text)) = wanted else {
        return;
    };
    let Some(([x, y, z], icon_half)) = marker_top(level, session, hover) else {
        return;
    };
    let eye = session.eye();
    let distance = (eye[0] - x).hypot(eye[1] - y).hypot(eye[2] - z);
    let scale = (distance * CARD_SCALE_PER_BLOCK) as f32;
    // The drawing hangs under the line: its foot has to clear the top of the icon.
    let hanging = if drawn {
        depth as f32 - LINE_HEIGHT
    } else {
        0.0
    };
    let lift = icon_half + (hanging + CARD_GAP) * scale / PIXELS_PER_BLOCK;
    let at = session.on_model(x, y, z);
    let display = card_display(text.clone(), drawn, style);
    session.pins.card = Some(ShownCard {
        entity: spawn_card(commands, at, display, scale, lift),
        text,
        style,
        hover,
        zoom,
    });
}

fn bar_title(level: &Level, session: &MapSession) -> String {
    let mode = session.mode.label();
    match session.hovered {
        Some(Hover::Waypoint(index)) => {
            let marker = &level.waypoints[index].marker;
            format!("{mode} - {}", marker.name)
        }
        _ => mode.to_string(),
    }
}

fn follow_view(
    mut sessions: Query<&mut MapSession>,
    mut positions: Query<&mut Position, Without<MapSession>>,
    mut rotations: Query<&mut Rotation, Without<MapSession>>,
    atlas: Res<Atlas>,
    mut displays: Query<&mut Display>,
    mut bars: Query<&mut BossBar>,
    mut commands: Commands,
) {
    for mut session in &mut sessions {
        let (Some(rig), Some(level)) = (session.rig, atlas.level(session.model())) else {
            continue;
        };
        // The box rides on the client, but the engine goes by this to keep it shown.
        // Not on a flight: the client would see it run ahead, to the end of the segment.
        let box_moves = session.travel.is_none();
        let moves = [
            (rig.camera, session.camera_at()),
            (box_moves.then_some(rig.eye_box), session.eye_box_position()),
        ];
        for (entity, at) in moves {
            if let Some(mut position) = entity.and_then(|entity| positions.get_mut(entity).ok()) {
                position.set_if_neq(at);
            }
        }

        if let Some(mut rotation) = rig.camera.and_then(|camera| rotations.get_mut(camera).ok()) {
            rotation.set_if_neq(session.camera_rotation());
        }

        // The wheel holds on to what the look was on: its card shows the styles.
        let hovered = match (session.travel.is_some(), session.is_frozen()) {
            (true, _) => None,
            (false, true) => session.hovered,
            (false, false) => hover_of(level, &session),
        };
        if hovered != session.hovered {
            let previous = std::mem::replace(&mut session.hovered, hovered);
            tracing::info!(?hovered, "map hover");
            for hover in [previous, hovered].into_iter().flatten() {
                resize(&session, hover, &mut displays);
            }
            if let Some(Hover::Waypoint(index)) = previous {
                commands
                    .entity(session.pins.icons[index])
                    .try_remove::<Glowing>();
            }
            if let Some(Hover::Waypoint(index)) = hovered {
                commands
                    .entity(session.pins.icons[index])
                    .try_insert(Glowing);
            }
        }
        if (session.zoom.height() / session.sized_for - 1.0).abs() > MARKER_ZOOM_STEP {
            session.sized_for = session.zoom.height();
            for index in 0..session.pins.icons.len() {
                resize(&session, Hover::Waypoint(index), &mut displays);
            }
            resize(&session, Hover::You, &mut displays);
            for sign in &session.pins.signs {
                set_scale(&mut displays, *sign, session.sign_scale());
            }
        }
        show_card(&atlas, level, &mut session, &mut commands);
        let title = bar_title(level, &session);
        if let Ok(mut bar) = bars.get_mut(session.bar)
            && bar.title != title
        {
            bar.title = title;
        }
    }
}

fn select(session: &mut MapSession, choice: Option<usize>, displays: &mut Query<&mut Display>) {
    let previous = std::mem::replace(&mut session.selected, choice);
    if let Some(old) = previous.filter(|old| Some(*old) != choice) {
        resize(session, Hover::Waypoint(old), displays);
    }
}

/// Whether this packet is a new click, which always lands on the eye box.
fn counts_as_click(session: &mut MapSession, id: i32, ids: &Query<&MinecraftEntityId>) -> bool {
    let is_eye_box = session
        .rig
        .and_then(|rig| ids.get(rig.eye_box).ok())
        .is_some_and(|eye_box| eye_box.0 == id);
    if !is_eye_box || session.ticks.saturating_sub(session.last_click) < CLICK_COOLDOWN {
        return false;
    }
    session.last_click = session.ticks;
    true
}

/// The server gets this instead of a spectator's camera change, and ignores that part.
#[allow(clippy::too_many_arguments)] // Bevy systems take their world as arguments.
fn left_click(
    event: On<PacketEvent<SpectateEntity>>,
    mut sessions: Query<&mut MapSession>,
    ids: Query<&MinecraftEntityId>,
    atlas: Res<Atlas>,
    mut displays: Query<&mut Display>,
    messages: Messages,
    players: Players,
    mut commands: Commands,
) {
    let player = event.entity;
    let Ok(mut session) = sessions.get_mut(player) else {
        return;
    };
    let busy = session.travel.is_some() || session.rig.is_none();
    if busy || !counts_as_click(&mut session, event.packet.entity_id, &ids) {
        return;
    }
    let Some(Hover::Waypoint(index)) = session.hovered else {
        tracing::info!(?player, "map left click on no marker");
        return;
    };
    let Some(level) = atlas.level(session.model()) else {
        return;
    };
    let marker = &level.waypoints[index].marker;
    let region = atlas.regions.at(f64::from(marker.x), f64::from(marker.z));
    tracing::info!(?player, marker = %marker.name, state = ?region.state, "map left click");
    if region.state != RegionState::Discovered {
        let refusal = format!(
            "{} : {}. Voyage impossible.",
            region.name,
            region.state.label()
        );
        players.send(
            player,
            SystemChat {
                content: Text::new(refusal.clone()).color(TextColor::Red).to_nbt(),
                overlay: true,
            },
        );
        messages.message(player, refusal).send();
    } else if session.selected == Some(index) {
        start_dive(level, &mut session, index, &mut displays, &mut commands);
    } else {
        session.heading = Some(ChunkPos::from_block(
            f64::from(marker.x),
            f64::from(marker.z),
        ));
        select(&mut session, Some(index), &mut displays);
        let said = format!(
            "{} sélectionné. Clic gauche à nouveau : s'y rendre.",
            marker.name
        );
        messages.message(player, said).send();
    }
}

/// Takes the view into a flight. A look has no camera entity: one takes over from the eye,
/// looking the same way, and the player stays where they are.
fn set_off(
    session: &mut MapSession,
    travel: Travel,
    displays: &mut Query<&mut Display>,
    commands: &mut Commands,
) {
    let [yaw, pitch] = travel.flight.looking();
    let [x, y, z] = session.vehicle_at(travel.flight.at());
    let at = session.on_model(x, y, z);
    if let Some(rig) = session.rig.as_mut() {
        if session.mode == MapMode::Look {
            // A camera would see it, standing around the eye.
            commands.entity(rig.eye_box).try_insert(Hidden);
        }
        match rig.camera {
            Some(camera) => {
                if let Ok(mut display) = displays.get_mut(camera) {
                    display.teleport_ticks = FLIGHT_GLIDE_TICKS;
                }
            }
            None => {
                let camera = spawn_eye_camera(commands, at, Rotation { yaw, pitch });
                rig.camera = Some(camera.vehicle);
                rig.eye = Some(camera.eye);
            }
        }
    }
    session.travel = Some(travel);
}

/// Sets off towards a waypoint.
fn start_dive(
    level: &Level,
    session: &mut MapSession,
    index: usize,
    displays: &mut Query<&mut Display>,
    commands: &mut Commands,
) {
    let waypoint = &level.waypoints[index];
    let marker = &waypoint.marker;
    tracing::info!(marker = %marker.name, "map dive");
    let look = match session.mode {
        MapMode::Look => session.look,
        MapMode::Pan => session.rest(),
    };
    let stop = DIVE_STOP * f64::from(session.model().scale().build);
    let eye = session.eye();
    select(session, None, displays);
    let to = Position {
        x: f64::from(marker.x) + 0.5,
        y: f64::from(marker.height) + 1.0 + ARRIVAL_CLEARANCE,
        z: f64::from(marker.z) + 0.5,
    };
    session.heading = Some(ChunkPos::from_block(to.x, to.z));
    let travel = Travel {
        flight: Flight::dive(eye, [look.yaw, look.pitch], waypoint.at, stop, DIVE_TICKS),
        ending: Ending::World(Some(to)),
    };
    set_off(session, travel, displays, commands);
}

/// Gives the view back, from a flight over or called off.
fn hand_view_back(
    session: &mut MapSession,
    player: Entity,
    id: &MinecraftEntityId,
    displays: &mut Query<&mut Display>,
    players: &Players,
    commands: &mut Commands,
) {
    session.travel = None;
    if let Some(rig) = session.rig {
        commands.entity(rig.eye_box).try_remove::<Hidden>();
    }
    match session.mode {
        MapMode::Look => {
            if let Some(rig) = session.rig.as_mut() {
                let camera = rig.camera.take().into_iter().chain(rig.eye.take());
                for entity in camera {
                    commands.entity(entity).try_despawn();
                }
            }
            // The client went on turning behind the entity.
            players.send(
                player,
                PlayerRotation {
                    yaw: yaw_as_shown(session.look.yaw),
                    relative_yaw: false,
                    pitch: session.look.pitch,
                    relative_pitch: false,
                },
            );
            players.send(player, SetCamera { camera_id: id.0 });
        }
        MapMode::Pan => {
            let camera = session.rig.and_then(|rig| rig.camera);
            if let Some(mut display) = camera.and_then(|camera| displays.get_mut(camera).ok()) {
                display.teleport_ticks = CAMERA_GLIDE_TICKS;
            }
        }
    }
}

/// `Interact` is decoded by its entity id only: that field is still good.
fn right_click(
    event: On<PacketEvent<Interact>>,
    mut sessions: Query<&mut MapSession>,
    ids: Query<&MinecraftEntityId>,
    mut displays: Query<&mut Display>,
) {
    let player = event.entity;
    let Ok(mut session) = sessions.get_mut(player) else {
        return;
    };
    if counts_as_click(&mut session, event.packet.entity_id, &ids) {
        tracing::info!(?player, "map right click");
        select(&mut session, None, &mut displays);
    }
}

/// What each key does, named as the player has them bound.
fn hint() -> Text {
    let key = Text::keybind;
    Text::new("")
        .color(TextColor::White)
        .append(key("key.jump"))
        .append(Text::new(" : changer de vue   |   "))
        .append(key("key.left"))
        .append(Text::new(" / "))
        .append(key("key.right"))
        .append(Text::new(" : zoom   |   "))
        .append(key("key.sprint"))
        .append(Text::new(" : apparence   |   "))
        .append(key("key.sneak"))
        .append(Text::new(" : fermer"))
}

fn wheel_hint() -> Text {
    Text::new("Souris : gauche / droite = catégorie, haut / bas = choix   |   Relâcher ")
        .color(TextColor::White)
        .append(Text::keybind("key.sprint"))
        .append(Text::new(" : valider"))
}

/// The labels of the wheel's sectors, clockwise from the top.
const WHEEL_LABELS: [&str; CATEGORIES] = ["Panneau", "Marqueur", "Déplacement", "Zones", "Blocs"];

/// What a sector of the wheel offers, and which of it is on.
fn wheel_choices(category: usize, looks: MapLooks) -> (Vec<(&'static str, LookChoice)>, usize) {
    fn of<T: Copy + PartialEq>(
        all: &[T],
        now: T,
        label: fn(T) -> &'static str,
        choice: fn(Option<T>) -> LookChoice,
    ) -> (Vec<(&'static str, LookChoice)>, usize) {
        let choices = all.iter().map(|one| (label(*one), choice(Some(*one))));
        let index = all.iter().position(|one| *one == now).unwrap_or(0);
        (choices.collect(), index)
    }
    match category {
        0 => of(&Style::ALL, looks.style, Style::label, LookChoice::Style),
        1 => of(&Me::ALL, looks.me, Me::label, LookChoice::Me),
        2 => of(
            &Motion::ALL,
            looks.motion,
            Motion::label,
            LookChoice::Motion,
        ),
        3 => of(
            &ZoneLook::ALL,
            looks.zone,
            ZoneLook::label,
            LookChoice::Zone,
        ),
        _ => of(
            &Blocks::ALL,
            looks.blocks,
            Blocks::label,
            LookChoice::Blocks,
        ),
    }
}

fn open_wheel(session: &mut MapSession, player: Entity, commands: &mut Commands) {
    let look = session.look;
    let eye = session.eye();
    let at = session.on_model(eye[0], eye[1], eye[2]);
    // A look turns on the client: the view holds still through an entity instead.
    let vehicle = session.on_model(eye[0], eye[1] - RIDER_EYE_HEIGHT, eye[2]);
    if let Some(rig) = session.rig.as_mut().filter(|rig| rig.camera.is_none()) {
        let camera = spawn_eye_camera(commands, vehicle, look);
        // A client whose camera is another entity only reports its mouse when it rides.
        // The player sits right where they stood, so that nothing moves when they get off.
        commands
            .entity(camera.vehicle)
            .insert(Passengers::new([camera.eye, player]));
        commands.entity(rig.eye_box).try_insert(Hidden);
        rig.camera = Some(camera.vehicle);
        rig.eye = Some(camera.eye);
    }
    session.mouse = Mouse::default();
    let way = look_direction(look.yaw, look.pitch);
    let [x, y, z] =
        std::array::from_fn(|axis| [at.x, at.y, at.z][axis] + way[axis] * WHEEL_DISTANCE);
    let middle = (WHEEL_MIDDLE_DROP - LINE_HEIGHT) * WHEEL_SCALE / PIXELS_PER_BLOCK;
    let text = TextDisplay::new(String::new())
        .line_width(CARD_LINE_WIDTH)
        .background_color(CLEAR)
        .see_through();
    let display = spawn_card(commands, Position { x, y, z }, text, WHEEL_SCALE, middle);
    session.wheel = Some(OpenWheel {
        wheel: Wheel::default(),
        display,
        ticks: 0,
        face: String::new(),
    });
}

fn close_wheel(
    session: &mut MapSession,
    player: Entity,
    ids: &Query<&MinecraftEntityId>,
    players: &Players,
    commands: &mut Commands,
) {
    let Some(open) = session.wheel.take() else {
        return;
    };
    commands.entity(open.display).try_despawn();
    session.mouse = Mouse::default();
    session.told = 0;
    players.send(
        player,
        SystemChat {
            content: hint().to_nbt(),
            overlay: true,
        },
    );
    if session.mode == MapMode::Look {
        if let Some(rig) = session.rig.as_mut() {
            commands.entity(rig.eye_box).try_remove::<Hidden>();
            for entity in rig.camera.take().into_iter().chain(rig.eye.take()) {
                commands.entity(entity).try_despawn();
            }
        }
        // The client went on turning behind the entity.
        players.send(
            player,
            PlayerRotation {
                yaw: yaw_as_shown(session.look.yaw),
                relative_yaw: false,
                pitch: session.look.pitch,
                relative_pitch: false,
            },
        );
        if let Ok(id) = ids.get(player) {
            players.send(player, SetCamera { camera_id: id.0 });
        }
    }
}

/// Applies what the mouse did to a wheel and draws it.
fn run_wheels(
    mut sessions: Query<(Entity, &mut MapSession)>,
    mut texts: Query<&mut TextDisplay>,
    ids: Query<&MinecraftEntityId>,
    players: Players,
    mut commands: Commands,
) {
    for (player, mut session) in &mut sessions {
        let looks = session.looks;
        let (eye, look) = (session.eye(), session.look);
        let at = session.on_model(eye[0], eye[1], eye[2]);
        let seeing = session.rig.and_then(|rig| rig.eye);
        let Some(open) = session.wheel.as_mut() else {
            continue;
        };
        open.ticks += 1;
        if let Some(seeing) = seeing.and_then(|eye| ids.get(eye).ok()) {
            if open.ticks == TURN_TICKS {
                players.send(player, settled(seeing, at, [look.yaw, look.pitch]));
            }
            if open.ticks == SWAP_TICKS {
                players.send(
                    player,
                    SetCamera {
                        camera_id: seeing.0,
                    },
                );
            }
        }
        let category = open.wheel.category;
        let (choices, index) = wheel_choices(category, looks);
        let steps = open.wheel.take_steps();
        let index = stepped(index, steps, choices.len());
        if steps != 0 {
            commands.trigger(ChooseLook(player, choices[index].1));
        }
        let count = format!("{} / {}", index + 1, choices.len());
        let around = |by: i32| choices[stepped(index, by, choices.len())].0;
        let list = [around(-1), choices[index].0, around(1)];
        let face = wheel_face(category, WHEEL_LABELS, &count, list);
        if face != open.face {
            open.face.clone_from(&face);
            if let Ok(mut text) = texts.get_mut(open.display) {
                text.text = face;
            }
        }
    }
}

fn tick_sessions(
    mut sessions: Query<(Entity, &mut MapSession, &MinecraftEntityId), Without<Passage>>,
    mut displays: Query<&mut Display>,
    ids: Query<&MinecraftEntityId>,
    atlas: Res<Atlas>,
    players: Players,
    mut commands: Commands,
) {
    for (player, mut session, id) in &mut sessions {
        session.ticks += 1;
        session.telemetry.tick(Instant::now());
        let told = session.ticks.saturating_sub(session.told) < TOLD_TICKS && session.told > 0;
        if session.ticks.is_multiple_of(HINT_PERIOD) && !told {
            let content = if session.wheel.is_some() {
                wheel_hint()
            } else {
                hint()
            };
            players.send(
                player,
                SystemChat {
                    content: content.to_nbt(),
                    overlay: true,
                },
            );
        }
        if session.ticks.is_multiple_of(SUMMARY_PERIOD) {
            let summary = session.telemetry.summary();
            tracing::info!(mode = ?session.mode, ?summary, "map session");
        }
        if session.rig.is_none() {
            continue;
        }
        if let Some(held) = session.veiled.as_mut() {
            *held += 1;
        }
        if session.veiled.is_some() {
            if session.ready && session.veiled.is_some_and(|held| held >= VEIL_HOLD_TICKS) {
                tracing::info!(held = ?session.veiled, "map under the veil");
                session.veiled = None;
                players.send(player, veil_out());
            }
            continue;
        }
        if let Some(travel) = session.travel.as_mut() {
            travel.flight.tick();
            let (left, ending) = (travel.flight.left(), travel.ending);
            let flight = travel.flight;
            let eye = session.rig.and_then(|rig| rig.eye);
            let eye = eye.and_then(|eye| ids.get(eye).ok());
            if let Some(eye) = eye.filter(|_| flight.turning()) {
                let [x, y, z] = flight.at();
                let at = session.on_model(x, y, z);
                players.send(player, settled(eye, at, flight.looking()));
            }
            if let Some(eye) = eye.filter(|_| flight.swapping()) {
                players.send(player, SetCamera { camera_id: eye.0 });
            }
            if flight.settling()
                && let Some(camera) = session.rig.and_then(|rig| rig.camera)
                && let Ok(camera) = ids.get(camera)
            {
                let [x, y, z] = session.vehicle_at(flight.end());
                let at = session.on_model(x, y, z);
                players.send(player, settled(camera, at, flight.looking()));
            }
            match ending {
                Ending::View if left == 0 => {
                    let (displays, commands) = (&mut displays, &mut commands);
                    hand_view_back(&mut session, player, id, displays, &players, commands);
                }
                Ending::View => {}
                // Nothing takes over from this camera: no need for it to come to rest.
                Ending::World(to) => {
                    if flight.flying_for() == VEIL_FADE {
                        if session.drawn {
                            send_veil(&players, player);
                        }
                        let (at, look) = session.landing_place(to, flight);
                        let start = Flight::landing(eye_of(&at), look).at();
                        let [x, y, z] = start;
                        let camera = under_eye(Position { x, y, z });
                        session.landing = Some(spawn_eye_camera(&mut commands, camera, look));
                    }
                    if flight.flying_for() + TURN_TICKS == VEIL_FADE
                        && let Some(camera) = session.landing
                        && let Ok(eye) = ids.get(camera.eye)
                    {
                        let (at, look) = session.landing_place(to, flight);
                        players.send(player, settled(eye, at, [look.yaw, look.pitch]));
                    }
                    if flight.flying_for() == 0 {
                        commands.trigger(LeaveMap(player));
                    }
                }
            }
            continue;
        }
        if session.is_frozen() {
            continue;
        }
        match session.mode {
            MapMode::Look => {
                if let Some([yaw, pitch]) = session.free.tick() {
                    players.send(
                        player,
                        PlayerRotation {
                            yaw,
                            relative_yaw: true,
                            pitch,
                            relative_pitch: true,
                        },
                    );
                }
            }
            MapMode::Pan => {
                let [_, out, closer] = session.keys;
                session.zoom.tick(out, closer);
                if let Some(level) = atlas.level(Model::Detail) {
                    session.pan.tick(|at| on_land(&level.scene, at));
                }
            }
        }
    }
}

fn read_keys(
    event: On<PlayerInputEvent>,
    mut sessions: Query<&mut MapSession>,
    ids: Query<&MinecraftEntityId>,
    mut displays: Query<&mut Display>,
    players: Players,
    mut commands: Commands,
) {
    let Ok(mut session) = sessions.get_mut(event.entity) else {
        return;
    };
    let sprint = event.sprint;
    let usable = session.rig.is_some()
        && session.drawn
        && session.travel.is_none()
        && session.veiled.is_none();
    if sprint && session.wheel.is_none() && usable {
        open_wheel(&mut session, event.entity, &mut commands);
        players.send(
            event.entity,
            SystemChat {
                content: wheel_hint().to_nbt(),
                overlay: true,
            },
        );
    } else if !sprint {
        close_wheel(&mut session, event.entity, &ids, &players, &mut commands);
    }
    if session.wheel.is_some() {
        return;
    }
    let keys = [event.jump, event.left, event.right];
    let jump = keys[0] && !session.keys[0];
    if session.keys[1..] != keys[1..] {
        tracing::debug!(height = session.zoom.height(), ?keys, "map zoom keys");
    }
    session.keys = keys;
    let sneak = event.sneak && !session.sneaking;
    session.sneaking = event.sneak;
    let going = session.travel.map(|travel| travel.ending);
    if sneak && matches!(going, Some(Ending::World(Some(_)))) {
        let flight = session.travel.map(|travel| travel.flight.flying_for());
        if flight.is_some_and(|left| left > VEIL_FADE)
            && let Ok(id) = ids.get(event.entity)
        {
            let (displays, commands) = (&mut displays, &mut commands);
            hand_view_back(&mut session, event.entity, id, displays, &players, commands);
        }
    } else if sneak && going != Some(Ending::World(None)) {
        commands.trigger(CloseMap(event.entity));
    } else if jump && session.rig.is_some() && session.travel.is_none() {
        commands.trigger(OpenMap(event.entity, session.mode.other()));
    }
}

/// How a player's map looks, kept between sessions.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MapLooks {
    style: Style,
    me: Me,
    zone: ZoneLook,
    blocks: Blocks,
    motion: Motion,
    inertia: bool,
}

impl MapLooks {
    fn chunks(self) -> ChunkLook {
        ChunkLook {
            zone: self.zone,
            blocks: self.blocks,
        }
    }
}

/// How the map shows the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Me {
    #[default]
    Pin,
    Arrow,
    Ring,
    MiniHead,
    /// The head with a small arrow under it, always just below.
    HeadTip,
    /// The head on a disc, which points down like a speech bubble.
    Bubble,
    HeadRing,
    /// The head over an arrow on the ground, which says where the player was facing.
    HeadHeading,
}

impl Me {
    const ALL: [Self; 8] = [
        Self::Pin,
        Self::Arrow,
        Self::Ring,
        Self::MiniHead,
        Self::HeadTip,
        Self::Bubble,
        Self::HeadRing,
        Self::HeadHeading,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Pin => "Épingle",
            Self::Arrow => "Flèche au sol",
            Self::Ring => "Anneau au sol",
            Self::MiniHead => "Pastille avec tête",
            Self::HeadTip => "Tête et flèche",
            Self::Bubble => "Bulle avec tête",
            Self::HeadRing => "Tête et anneau",
            Self::HeadHeading => "Tête et cap",
        }
    }

    fn next(self) -> Self {
        let index = Self::ALL.iter().position(|me| *me == self).unwrap_or(0);
        Self::ALL[stepped(index, 1, Self::ALL.len())]
    }

    fn has_head(self) -> bool {
        !matches!(self, Self::Pin | Self::Arrow | Self::Ring)
    }
}

/// One of the looks of the map, or the next one of a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookChoice {
    Style(Option<Style>),
    Me(Option<Me>),
    Zone(Option<ZoneLook>),
    Blocks(Option<Blocks>),
    Motion(Option<Motion>),
    /// Turns inertia on or off.
    Inertia,
    /// An option that does not go with its action.
    Refused,
}

/// Changes a player's map, at once if they are in it.
#[derive(Event, Debug, Clone, Copy)]
pub struct ChooseLook(pub Entity, pub LookChoice);

fn choose_look(
    event: On<ChooseLook>,
    mut states: Query<(Option<&MapLooks>, Option<&mut MapSession>)>,
    atlas: Res<Atlas>,
    players: Players,
    mut commands: Commands,
) {
    let player = event.0;
    let Ok((looks, mut session)) = states.get_mut(player) else {
        return;
    };
    let tell = |text: &str, session: Option<&mut Mut<MapSession>>| {
        if let Some(session) = session {
            session.told = session.ticks;
        }
        players.send(
            player,
            SystemChat {
                content: Text::new(text).color(TextColor::White).to_nbt(),
                overlay: true,
            },
        );
    };
    let mut looks = looks.copied().unwrap_or_default();
    let said = match event.1 {
        LookChoice::Style(style) => {
            looks.style = style.unwrap_or(looks.style.next());
            format!("Panneau : {}", looks.style.label())
        }
        LookChoice::Me(me) => {
            looks.me = me.unwrap_or(looks.me.next());
            format!("Marqueur : {}", looks.me.label())
        }
        LookChoice::Zone(zone) => {
            looks.zone = zone.unwrap_or(looks.zone.next());
            format!("Zones : {}", looks.zone.label())
        }
        LookChoice::Blocks(blocks) => {
            looks.blocks = blocks.unwrap_or(looks.blocks.next());
            format!("Blocs : {}", looks.blocks.label())
        }
        LookChoice::Motion(motion) => {
            looks.motion = motion.unwrap_or(looks.motion.next());
            format!("Déplacement : {}", looks.motion.label())
        }
        LookChoice::Inertia => {
            looks.inertia = !looks.inertia;
            let state = if looks.inertia { "oui" } else { "non" };
            format!("Inertie : {state}")
        }
        LookChoice::Refused => {
            tell(
                "Cette option ne va pas avec cette action.",
                session.as_mut(),
            );
            return;
        }
    };
    tracing::debug!(?player, ?looks, "map looks");
    commands.entity(player).insert(looks);
    let on_wheel = session
        .as_ref()
        .is_some_and(|session| session.wheel.is_some());
    if !on_wheel {
        tell(&said, session.as_mut());
    }
    if let Some(mut session) = session {
        let before = std::mem::replace(&mut session.looks, looks);
        if before.me != looks.me {
            respawn_you(&mut session, &mut commands);
        }
        if before.zone != looks.zone {
            respawn_signs(&atlas, &mut session, &mut commands);
        }
        session.pan.drive(looks.motion, looks.inertia);
    }
}

/// The client has the pack, so it can draw cards.
#[derive(Component)]
struct PackLoaded;

fn note_pack(event: On<PackSettled>, mut commands: Commands) {
    if event.loaded {
        commands.entity(event.player).insert(PackLoaded);
    }
}

/// Starts the way out: down to the player's marker, the way they came up.
fn close(
    event: On<CloseMap>,
    mut sessions: Query<&mut MapSession>,
    mut displays: Query<&mut Display>,
    mut commands: Commands,
) {
    let Ok(mut session) = sessions.get_mut(event.0) else {
        return;
    };
    if session.leaving() || session.rig.is_none() {
        return;
    }
    if let Some(open) = session.wheel.take() {
        commands.entity(open.display).try_despawn();
    }
    let mut flight = session.way_to_marker(CLOSING_TICKS);
    if let Some(travel) = session.travel {
        // Called while on the way in: from where that flight got to, and how it looks.
        let build = f64::from(session.model().scale().build);
        let spot = session.pins.you.as_ref().map(|you| you.at);
        let (at, look) = (travel.flight.at(), travel.flight.looking());
        flight = spot.map_or(flight, |spot| {
            Flight::dive(at, look, spot, DIVE_STOP * build, CLOSING_TICKS)
        });
    }
    session.heading = Some(ChunkPos::from_block(session.position.x, session.position.z));
    let travel = Travel {
        flight,
        ending: Ending::World(None),
    };
    set_off(&mut session, travel, &mut displays, &mut commands);
}

fn leave(
    event: On<LeaveMap>,
    mut sessions: Query<(&mut MapSession, &LoadedChunks)>,
    config: Res<ServerConfigResource>,
    players: Players,
    mut commands: Commands,
) {
    let player = event.0;
    let Ok((mut session, loaded)) = sessions.get_mut(player) else {
        return;
    };
    tracing::info!(?player, "leaving the map");
    if let Some(Travel {
        flight,
        ending: Ending::World(to),
    }) = session.travel.take()
    {
        (session.position, session.rotation) = session.landing_place(to, flight);
    }
    for chunk in loaded.0.difference(&session.world) {
        players.send(
            player,
            UnloadChunk {
                chunk_x: chunk.x,
                chunk_z: chunk.z,
            },
        );
    }
    // The engine never saw the player leave this chunk, so it will not say so itself. It
    // goes first: a square that shrinks around the models would let go of the world.
    let home = ChunkPos::from_block(session.position.x, session.position.z);
    players.send(
        player,
        SetCenterChunk {
            chunk_x: home.x,
            chunk_z: home.z,
        },
    );
    players.send(
        player,
        SetChunkCacheRadius {
            radius: config.view_distance,
        },
    );
    despawn_rig(&mut session, &mut commands);
    despawn_pins(&mut session, &mut commands);
    commands.entity(session.bar).try_despawn();
    let Position {
        x: home_x,
        y: home_y,
        z: home_z,
    } = session.position;
    let (yaw, pitch) = (session.rotation.yaw, session.rotation.pitch);
    let flight = Flight::landing(eye_of(&session.position), session.rotation);
    let [x, y, z] = flight.at();
    let start = under_eye(Position { x, y, z });
    let waiting = session.landing.take();
    let camera =
        waiting.unwrap_or_else(|| spawn_eye_camera(&mut commands, start, session.rotation));
    let back = Back {
        home,
        abilities: session.abilities,
        tab_game_mode: session.tab_game_mode,
    };
    let world = std::mem::take(&mut session.world);
    commands
        .entity(player)
        .remove::<(MapSession, ChunkStreamingPaused)>()
        .insert((
            LoadedChunks(world),
            // Not a real distance: makes the next streaming pass start over.
            EffectiveViewDistance(0),
            ChunkStreamBacklog::default(),
            Teleport::to(home_x, home_y, home_z).facing(yaw, pitch),
            Passage {
                camera,
                kind: PassageKind::Landing(flight),
                ticks: waiting.map_or(0, |_| SWAP_TICKS - 1),
                back: Some(back),
            },
        ));
}

fn player_left(
    event: On<PlayerQuitEvent>,
    mut sessions: Query<&mut MapSession>,
    passages: Query<&Passage>,
    mut commands: Commands,
) {
    if let Ok(mut session) = sessions.get_mut(event.entity) {
        despawn_rig(&mut session, &mut commands);
        despawn_pins(&mut session, &mut commands);
        commands.entity(session.bar).try_despawn();
        if let Some((vehicle, eye)) = session.camera {
            for entity in eye.into_iter().chain([vehicle]) {
                commands.entity(entity).try_despawn();
            }
        }
        if let Some(camera) = session.landing {
            despawn_eye_camera(camera, &mut commands);
        }
    }
    if let Ok(passage) = passages.get(event.entity) {
        despawn_eye_camera(passage.camera, &mut commands);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapAction {
    Look,
    Pan,
    Stop,
    Style,
    Me,
    Zone,
    Blocks,
    Move,
}

/// Which option of a look, as a player types it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Flat,
    Real,
    A,
    B,
    C,
    D,
    Inertia,
}

impl Pick {
    fn style(self) -> Option<Style> {
        match self {
            Self::One => Some(Style::Sheet),
            Self::Two => Some(Style::Bare),
            Self::Three => Some(Style::Bubble),
            Self::Four => Some(Style::Title),
            _ => None,
        }
    }

    fn me(self) -> Option<Me> {
        match self {
            Self::One => Some(Me::Pin),
            Self::Two => Some(Me::Arrow),
            Self::Three => Some(Me::Ring),
            Self::Four => Some(Me::MiniHead),
            Self::Five => Some(Me::HeadTip),
            Self::Six => Some(Me::Bubble),
            Self::Seven => Some(Me::HeadRing),
            Self::Eight => Some(Me::HeadHeading),
            _ => None,
        }
    }

    fn zone(self) -> Option<ZoneLook> {
        match self {
            Self::One => Some(ZoneLook::Clouds),
            Self::Two => Some(ZoneLook::Hatching),
            Self::Three => Some(ZoneLook::Palisade),
            Self::Four => Some(ZoneLook::Faded),
            _ => None,
        }
    }

    fn motion(self) -> Option<Motion> {
        match self {
            Self::A => Some(Motion::Damped),
            Self::B => Some(Motion::Slack),
            Self::C => Some(Motion::Lead),
            Self::D => Some(Motion::Predicted),
            _ => None,
        }
    }

    fn blocks(self) -> Option<Blocks> {
        match self {
            Self::Flat => Some(Blocks::Flat),
            Self::Real => Some(Blocks::Real),
            _ => None,
        }
    }
}

/// What an option means for a look: none moves on to the next, a wrong one is refused.
fn choice<T>(
    pick: Option<Pick>,
    meaning: fn(Pick) -> Option<T>,
    look: fn(Option<T>) -> LookChoice,
) -> LookChoice {
    match pick {
        None => look(None),
        Some(pick) => meaning(pick).map_or(LookChoice::Refused, |value| look(Some(value))),
    }
}

fn map_command() -> Command {
    CommandBuilder::new("map")
        .description("Ouvre la carte du monde")
        .arg_optional(
            "action",
            EnumArg::new([
                ("look", MapAction::Look),
                ("pan", MapAction::Pan),
                ("stop", MapAction::Stop),
                ("style", MapAction::Style),
                ("me", MapAction::Me),
                ("zone", MapAction::Zone),
                ("blocks", MapAction::Blocks),
                ("move", MapAction::Move),
            ]),
        )
        .arg_optional(
            "option",
            EnumArg::new([
                ("1", Pick::One),
                ("2", Pick::Two),
                ("3", Pick::Three),
                ("4", Pick::Four),
                ("5", Pick::Five),
                ("6", Pick::Six),
                ("7", Pick::Seven),
                ("8", Pick::Eight),
                ("flat", Pick::Flat),
                ("real", Pick::Real),
                ("a", Pick::A),
                ("b", Pick::B),
                ("c", Pick::C),
                ("d", Pick::D),
                ("inertie", Pick::Inertia),
            ]),
        )
        .handler(run_map)
        .build()
}

/// The engine's arguments form one chain, so an option can follow any action. After
/// one that opens the map, it picks the style.
fn run_map(ctx: &mut CommandContext) {
    let player = ctx.entity;
    let action = ctx.get::<MapAction>("action").copied();
    let pick = ctx.get::<Pick>("option").copied();
    ctx.with_world_mut(|world| {
        let chosen = match action.unwrap_or(MapAction::Look) {
            MapAction::Look => {
                world.trigger(OpenMap(player, MapMode::Look));
                pick.map(|_| choice(pick, Pick::style, LookChoice::Style))
            }
            MapAction::Pan => {
                world.trigger(OpenMap(player, MapMode::Pan));
                pick.map(|_| choice(pick, Pick::style, LookChoice::Style))
            }
            MapAction::Stop => {
                world.trigger(CloseMap(player));
                None
            }
            MapAction::Style => Some(choice(pick, Pick::style, LookChoice::Style)),
            MapAction::Me => Some(choice(pick, Pick::me, LookChoice::Me)),
            MapAction::Zone => Some(choice(pick, Pick::zone, LookChoice::Zone)),
            MapAction::Blocks => Some(choice(pick, Pick::blocks, LookChoice::Blocks)),
            MapAction::Move if pick == Some(Pick::Inertia) => Some(LookChoice::Inertia),
            MapAction::Move => Some(choice(pick, Pick::motion, LookChoice::Motion)),
        };
        if let Some(chosen) = chosen {
            world.trigger(ChooseLook(player, chosen));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use voidmc::{
        DefaultWorldGenerator, ServerConfig,
        commands::plugin::CommandPlugin,
        network::{ClientConnected, IncomingPacket, NetworkPlugin, OutgoingPacket},
        plugins::DefaultPlugins,
        systems::GameSystemsPlugin,
        world::ChunkIndex,
    };

    fn engine() -> App {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, _outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let (_connected_tx, connected_rx) = flume::unbounded::<ClientConnected>();

        let mut app = App::new();
        app.add_plugins(NetworkPlugin::new(
            incoming_rx,
            outgoing_tx,
            disconnect_rx,
            kick_tx,
            connected_rx,
        ))
        .add_plugins(DefaultPlugins)
        .add_plugins(CommandPlugin)
        .add_plugins(GameSystemsPlugin)
        .insert_resource(RegistryDataStore::default())
        .insert_resource(ServerConfigResource::from(&ServerConfig::default()))
        .insert_resource(WorldGen(Box::new(DefaultWorldGenerator::default())))
        .init_resource::<ChunkIndex>()
        .add_plugins(MapPlugin);
        app
    }

    fn session(mode: MapMode) -> MapSession {
        MapSession {
            mode,
            site: Site::of(0),
            position: Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            rotation: Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
            abilities: None,
            tab_game_mode: None,
            pins: Pins::default(),
            bar: Entity::PLACEHOLDER,
            rig: None,
            mouse: Mouse::default(),
            pan: Pan::default(),
            zoom: Zoom::default(),
            pan_model: Model::Detail,
            sized_for: Zoom::default().height(),
            free: FreeLook::default(),
            telemetry: Telemetry::default(),
            head: item("minecraft:player_head"),
            looks: MapLooks::default(),
            drawn: false,
            painted: ChunkLook::default(),
            behind: HashMap::new(),
            world: HashSet::new(),
            heading: None,
            camera: None,
            travel: None,
            landing: None,
            veiled: None,
            ready: false,
            wheel: None,
            told: 0,
            sneaking: false,
            keys: [false; 3],
            look: Rotation {
                yaw: REST_YAW,
                pitch: REST_PITCH,
            },
            hovered: None,
            selected: None,
            streamed: (0, 0),
            ticks: 0,
            last_click: 0,
        }
    }

    /// The whole point of a site: a client holds a player's models and the world together.
    #[test]
    fn every_site_holds_the_models_and_the_world() {
        for client in 0..(ANCHOR_COLUMNS * ANCHOR_ROWS) as u32 {
            let site = Site::of(client);
            let [x, z] = site.anchor;
            for chunk in [
                ChunkPos::new(-WORLD_KEPT_REACH, -WORLD_KEPT_REACH),
                ChunkPos::new(WORLD_KEPT_REACH, WORLD_KEPT_REACH),
                ChunkPos::new(x - DETAIL_PAN_REACH, z - DETAIL_PAN_REACH),
                ChunkPos::new(
                    x + OVERVIEW_ORIGIN + OVERVIEW_PAN_REACH,
                    z + DETAIL_PAN_REACH,
                ),
            ] {
                assert!(site.holds(chunk), "{client} {chunk:?} {site:?}");
            }
            assert!(site.radius < 400, "a client allocates this square");
        }
    }

    #[test]
    fn the_real_world_stays_out_of_the_view_from_a_model() {
        let site = Site::of(0);
        assert!(site.anchor[0] - DETAIL_PAN_REACH - WORLD_KEPT_REACH >= RENDER_GAP);
    }

    #[test]
    fn sites_do_not_overlap() {
        let sites: Vec<_> = (0..(ANCHOR_COLUMNS * ANCHOR_ROWS) as u32)
            .map(Site::of)
            .collect();
        for (index, one) in sites.iter().enumerate() {
            for other in &sites[index + 1..] {
                let apart = (one.anchor[0] - other.anchor[0]).abs() >= CHUNKS_BETWEEN_ANCHORS
                    || (one.anchor[1] - other.anchor[1]).abs() >= CHUNKS_BETWEEN_ROWS;
                assert!(apart, "{one:?} {other:?}");
            }
        }
    }

    /// Bevy only reports conflicting system params when the schedule first runs.
    #[test]
    fn the_plugin_ticks_with_the_full_engine() {
        let mut app = engine();
        app.update();
        app.update();
        let atlas = app.world().resource::<Atlas>();
        let overview = atlas.overview.as_ref().unwrap();
        assert!(!overview.waypoints.is_empty());
        assert!(
            overview
                .waypoints
                .iter()
                .all(|w| w.at[0].abs() < f64::from(OVERVIEW.radius)
                    && w.at[2].abs() < f64::from(OVERVIEW.radius))
        );
    }

    #[test]
    fn the_hint_names_the_player_s_own_keys() {
        let hint = format!("{:?}", hint().to_compound());
        for action in [
            "key.jump",
            "key.left",
            "key.right",
            "key.sprint",
            "key.sneak",
        ] {
            assert!(hint.contains(action), "{action}: {hint}");
        }
    }

    fn map_registry() -> CommandRegistry {
        let mut registry = CommandRegistry::new();
        registry.register(map_command());
        registry
    }

    #[test]
    fn map_completes_its_actions_and_its_styles() {
        let world = World::new();
        let registry = map_registry();
        let actions = registry.complete("map ", &world).expect("actions");
        assert_eq!(
            actions.matches,
            [
                "look", "pan", "stop", "style", "me", "zone", "blocks", "move"
            ]
        );
        for action in ["style", "me", "zone", "blocks", "move"] {
            let options = registry
                .complete(&format!("map {action} "), &world)
                .expect("options");
            assert_eq!(
                options.matches,
                [
                    "1", "2", "3", "4", "5", "6", "7", "8", "flat", "real", "a", "b", "c", "d",
                    "inertie"
                ]
            );
        }
    }

    /// A look that stopped on the client's position tick used to go unseen, so that
    /// the marker under the crosshair stayed unhovered until the mouse moved again.
    #[test]
    fn a_look_reported_with_the_position_counts() {
        let mut app = engine();
        let world = app.world_mut();
        let mut session = session(MapMode::Look);
        session.rig = Some(Rig {
            eye_box: Entity::PLACEHOLDER,
            camera: None,
            eye: None,
        });
        let player = world.spawn(session).id();
        world.trigger(PacketEvent {
            client_id: 0,
            entity: player,
            packet: SetPlayerPosAndRot {
                x: 0.0,
                y: 64.0,
                z: 0.0,
                yaw: 170.0,
                pitch: 30.0,
                flags: 0,
            },
        });
        let look = world.get::<MapSession>(player).unwrap().look;
        assert_eq!((look.yaw, look.pitch), (170.0, 30.0));
    }

    #[test]
    fn the_pointer_gives_way_to_the_veil_and_to_a_busy_view() {
        use cursor::{Look, Wanted};
        let mut session = session(MapMode::Look);
        session.drawn = true;
        assert_eq!(session.wanted_cursor(), Wanted::Veil, "still rising");
        session.rig = Some(Rig {
            eye_box: Entity::PLACEHOLDER,
            camera: None,
            eye: None,
        });
        assert_eq!(session.wanted_cursor(), Wanted::Pointer(Look::Free));
        session.hovered = Some(Hover::Waypoint(0));
        assert_eq!(session.wanted_cursor(), Wanted::Pointer(Look::Over));
        session.wheel = Some(OpenWheel {
            wheel: Wheel::default(),
            display: Entity::PLACEHOLDER,
            face: String::new(),
            ticks: 0,
        });
        assert_eq!(session.wanted_cursor(), Wanted::Nothing);
        let mut flight = Flight::new([0.0; 3], [0.0; 3], [0.0; 2], VEIL_FADE + 1);
        session.travel = Some(Travel {
            flight,
            ending: Ending::World(None),
        });
        assert_eq!(
            session.wanted_cursor(),
            Wanted::Nothing,
            "the veil is yet to come"
        );
        flight.tick();
        session.travel = Some(Travel {
            flight,
            ending: Ending::World(None),
        });
        assert_eq!(session.wanted_cursor(), Wanted::Veil);
        session.drawn = false;
        session.travel = None;
        session.wheel = None;
        assert_eq!(session.wanted_cursor(), Wanted::Nothing);
    }

    #[test]
    fn paths_link_every_place() {
        let cells = [(0, 0), (10, 0), (10, 10), (-40, 3)];
        let paths = spanning_paths(&cells);
        assert_eq!(paths.len(), 3);
        assert!(paths.contains(&((0, 0), (10, 0))));
        assert!(paths.contains(&((10, 0), (10, 10))));
        assert!(paths.contains(&((0, 0), (-40, 3))));
    }

    #[test]
    fn the_overview_keeps_all_its_chunks_wherever_the_camera_is() {
        let chunks = chunks_around(MapMode::Look, (0, 3));
        assert_eq!(chunks.len(), (OVERVIEW_REACH as usize * 2).pow(2));
        assert_eq!(chunks[0], (0, 3));
    }

    #[test]
    fn a_panning_camera_gets_the_chunks_around_it_nearest_first() {
        let chunks = chunks_around(MapMode::Pan, (20, -7));
        assert_eq!(chunks[0], (20, -7));
        assert!(chunks.contains(&(20 + DETAIL_SEND_RADIUS, -7)));
        assert!(!chunks.contains(&(20 + DETAIL_SEND_RADIUS + 1, -7)));
    }

    #[test]
    fn an_option_that_does_not_fit_its_action_is_refused() {
        let chosen = choice(Some(Pick::Real), Pick::me, LookChoice::Me);
        assert_eq!(chosen, LookChoice::Refused);
        let chosen = choice(Some(Pick::Real), Pick::blocks, LookChoice::Blocks);
        assert_eq!(chosen, LookChoice::Blocks(Some(Blocks::Real)));
        let chosen = choice(None, Pick::me, LookChoice::Me);
        assert_eq!(chosen, LookChoice::Me(None));
    }

    /// A zoom past the threshold used to send the camera gliding from one height to the
    /// other, and the model it landed on had to come over the wire.
    #[test]
    fn the_same_point_stays_in_the_middle_on_both_models() {
        let session = session(MapMode::Pan);
        let [dx, dy, dz] = session.pan_view(Model::Detail);
        let [ox, oy, oz] = session.pan_view(Model::Overview);
        let ratio = Model::Overview.ratio();
        let sea = f64::from(scene::SEA_Y);
        assert!((dx * ratio - ox).abs() < 1e-9);
        assert!(((dy - sea) * ratio - (oy - sea)).abs() < 1e-9);
        assert!((dz * ratio - oz).abs() < 1e-9);
    }

    #[test]
    fn a_pan_gets_both_models_its_own_first() {
        let session = session(MapMode::Pan);
        let wanted = session.wanted();
        let model = session.model();
        let own = wanted.iter().take_while(|(on, _)| *on == model).count();
        assert!(own > 0 && own < wanted.len());
        assert!(
            wanted[own..]
                .iter()
                .all(|(on, _)| model.neighbours().contains(on))
        );
    }

    #[test]
    fn the_two_models_never_share_a_chunk() {
        let session = session(MapMode::Pan);
        let row = [
            (Model::Detail, DETAIL_PAN_REACH),
            (Model::Middle, MIDDLE_PAN_REACH),
            (Model::Overview, OVERVIEW_PAN_REACH),
        ];
        for pair in row.windows(2) {
            let edge = session.placed(pair[0].0, (pair[0].1, 0));
            let next = session.placed(pair[1].0, (-pair[1].1, 0));
            assert!(edge.x < next.x, "{edge:?} {next:?}");
        }
    }

    #[test]
    fn a_zoom_goes_through_every_model_and_a_held_height_stays_on_one() {
        let out = Model::Detail.at_height(COARSE_ABOVE + 1.0);
        assert_eq!(out, Model::Middle);
        assert_eq!(out.at_height(COARSEST_ABOVE + 1.0), Model::Overview);
        assert_eq!(out.at_height(COARSE_ABOVE - 1.0), Model::Middle);
        assert_eq!(
            Model::Overview.at_height(COARSEST_ABOVE - 1.0),
            Model::Overview
        );
        assert_eq!(Model::Middle.at_height(DETAIL_BELOW - 1.0), Model::Detail);
    }

    #[test]
    fn a_card_says_what_a_waypoint_is_and_what_can_be_done() {
        let atlas = Atlas {
            info: MapLevel {
                biome: voidmc::BiomeId::plains(),
                biome_count: 1,
            },
            regions: Regions::demo(),
            overview: None,
            middle: None,
            detail: None,
        };
        let waypoint = Waypoint {
            marker: Marker {
                name: "Port-Noir".into(),
                kind: MarkerKind::Lighthouse,
                x: 340,
                z: -60,
                height: 70,
            },
            at: [0.0; 3],
        };
        let player = Position {
            x: 340.0,
            y: 64.0,
            z: 40.0,
        };
        let card = Card {
            open: true,
            ..waypoint_card(&atlas, &waypoint, &player)
        }
        .plain();
        for part in ["Port-Noir", "Phare", "Confins de l'Est", "100 blocs"] {
            assert!(card.contains(part), "{part}: {card}");
        }
    }
}
