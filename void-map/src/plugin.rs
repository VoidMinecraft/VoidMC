//! Takes players into the map and back.
//!
//! In the map a player is a spectator who cannot move. [`MapMode::Look`] is an
//! overview: they look around freely, with no delay since the client turns the view.
//! [`MapMode::Pan`] explores a larger model, streamed around the camera: they ride an
//! invisible entity and see through it, and the mouse moves it. Either way the middle
//! of the screen is the pointer.

use std::collections::{HashMap, HashSet};

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::prelude::*;
use voidmc::{
    Billboard, BossBar, BossBarColor, Command, CommandBuilder, CommandContext, CommandRegistry,
    Display, DisplayTransform, EntityBuilder, EntityKind, EntityShownEvent, EnumArg, Glowing,
    ItemDisplay, ItemDisplayContext, ItemStack, Messages, Passengers, PlayerAbilities, Players,
    RegistryDataStore, ServerConfigResource, TabEntry, Teleport, TextColor, TextDisplay, WorldGen,
    WorldGenerator,
    components::{
        ChunkStreamBacklog, ChunkStreamingPaused, ClientId, EffectiveViewDistance, LoadedChunks,
        MinecraftEntityId, PlayerUuid, Position, Rotation, ServerControlledPosition,
    },
    events::{PlayerInputEvent, PlayerQuitEvent},
    network::PacketEvent,
    world::ChunkPos,
};
use voidmc_codec::Encode;
use voidmc_protocol::{
    clientbound::{
        Chunk, GameEvent, GameEventType, PlayerRotation, Respawn, SetCamera, SetCenterChunk,
        SystemChat, UnloadChunk,
    },
    serverbound::{Interact, SetPlayerPos, SetPlayerPosAndRot, SetPlayerRotation, SpectateEntity},
    slot::{DataComponent, component_ids},
};
use voidmc_ui::{PackSettled, Text};

use crate::art::{Card, Style, Subject};
use crate::dimension::{self, DIMENSION, MapLevel};
use crate::markers::{Marker, MarkerField, MarkerKind};
use crate::pointer::{
    DEFAULT_ZOOM, DETAIL_PITCH, Mouse, OVERVIEW_Z, Pan, RECENTRED_PITCH, REST_PITCH, REST_YAW,
    SPECTATOR_EYE_HEIGHT, ZOOMS, aimed, detail_back, detail_y, look_direction, overview_y, stop,
};
use crate::regions::{RegionState, Regions};
use crate::scene::{self, DETAIL, OVERVIEW, Scale, Scene};
use crate::terrain::Terrain;

/// `WorldGenerator` does not expose its water level. This is the one of
/// `DefaultWorldGenerator`, and of vanilla.
pub const SEA_LEVEL: i32 = 62;
const SPECTATOR: u8 = 3;
const OVERWORLD_TYPE: i32 = 0;
const OVERWORLD: &str = "minecraft:overworld";
const OVERWORLD_SEA_LEVEL: i32 = 63;

/// Each player's model sits this far out, in its own chunks. Entities are only
/// shown to players with their chunk loaded, so nobody else sees the markers.
const FIRST_ANCHOR_CHUNK: i32 = 60_000;
const CHUNKS_BETWEEN_ANCHORS: i32 = 64;
const ANCHOR_SLOTS: u32 = 1024;

/// How far the overview's chunks go, which is the model plus the empty ring the
/// client needs to draw its last chunks.
const OVERVIEW_REACH: i32 = (OVERVIEW.radius + 15) / 16 + 1;
/// Chunks sent around a panning camera, and how far they stay before being unloaded.
const DETAIL_SEND_RADIUS: i32 = 9;
const DETAIL_KEEP_RADIUS: i32 = 11;
pub(crate) const CHUNKS_PER_TICK: usize = 64;

/// A longer glide overshoots: the client adds what the entity moved on its own to the
/// target of a glide, so that a fast pan ended past the rim, off by tens of blocks and
/// out of step with the server until the next long move. Entities still move smoothly
/// between ticks with one.
const CAMERA_GLIDE_TICKS: u8 = 1;

/// Above the tallest landmark, in its blocks.
const ICON_LIFT: f64 = 10.0;
const ICON_SCALE: f32 = 5.0;
const ICON_HOVER_GROWTH: f32 = 1.6;
/// The head floats over the player's spot, on a pin always drawn right under it.
const HEAD_LIFT: f64 = 7.0;
const HEAD_SCALE: f32 = 6.0;
/// How much of its scale it takes to reach the edge of a head.
const HEAD_HALF: f32 = 0.22;
const PIN_SCALE: f32 = 8.0;
const PIN: &str = "§c▼";
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

/// A spectator only clicks when the look hits an entity, and only sees the
/// crosshair when that entity has a menu. A chest minecart around the eye is always
/// hit. The eye sits under its floor, looking down, so the cart never shows.
const EYE_BOX_KIND: EntityKind = EntityKind::ChestMinecart;
const EYE_BOX_DROP: f64 = 0.02;
/// Displays vanish past 64 blocks times this, and the camera is further than that.
const VIEW_RANGE: f32 = 8.0;
const GLOW: i32 = 0xFFE9A0;
const HINT_PERIOD: u32 = 30;
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
    fn scale(self) -> Scale {
        match self {
            Self::Look => OVERVIEW,
            Self::Pan => DETAIL,
        }
    }

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

#[derive(Debug, Clone, PartialEq)]
struct Waypoint {
    marker: Marker,
    /// On the model, from its centre.
    at: [f64; 3],
}

/// One model, with its chunks as they get asked for.
struct Level {
    scene: Scene,
    waypoints: Vec<Waypoint>,
    /// Each with its weight on the wire.
    chunks: HashMap<(i32, i32), (Chunk, usize)>,
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
        Self {
            scene,
            waypoints,
            chunks: HashMap::new(),
        }
    }

    fn chunk(&mut self, at: (i32, i32), info: MapLevel) -> &(Chunk, usize) {
        self.chunks.entry(at).or_insert_with(|| {
            let chunk = self.scene.chunk(at.0, at.1, info.biome, info.biome_count);
            let mut bytes = Vec::new();
            chunk.to_packet().encode(&mut bytes);
            let weight = bytes.len();
            (chunk, weight)
        })
    }
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
    detail: Option<Level>,
}

impl Atlas {
    fn level(&self, mode: MapMode) -> Option<&Level> {
        match mode {
            MapMode::Look => self.overview.as_ref(),
            MapMode::Pan => self.detail.as_ref(),
        }
    }

    fn level_mut(&mut self, mode: MapMode) -> Option<&mut Level> {
        match mode {
            MapMode::Look => self.overview.as_mut(),
            MapMode::Pan => self.detail.as_mut(),
        }
    }
}

/// The entities a session adds once the player is in place.
#[derive(Debug, Clone, Copy)]
struct Rig {
    eye_box: Entity,
    /// Pan mode only: what the player rides and sees through.
    camera: Option<Entity>,
}

/// The markers of the current mode.
#[derive(Debug, Default)]
struct Pins {
    icons: Vec<Entity>,
    you: Option<You>,
    card: Option<ShownCard>,
}

/// The player's own marker.
#[derive(Debug, Clone, Copy)]
struct You {
    head: Entity,
    pin: Entity,
    /// Where they stand on the model.
    at: [f64; 3],
}

/// The card on screen, and what it was drawn from, to tell when it has to change.
#[derive(Debug)]
struct ShownCard {
    entity: Entity,
    text: String,
    hover: Hover,
    zoom: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hover {
    Waypoint(usize),
    You,
}

/// A player in the map, and how to put them back.
#[derive(Component)]
pub struct MapSession {
    mode: MapMode,
    anchor: [i32; 2],
    position: Position,
    rotation: Rotation,
    abilities: Option<PlayerAbilities>,
    tab_game_mode: Option<u8>,
    pins: Pins,
    bar: Entity,
    rig: Option<Rig>,
    mouse: Mouse,
    pan: Pan,
    zoom: usize,
    /// The zoom the markers are sized for.
    sized_for: usize,
    /// The player's head, as an item.
    head: ItemStack,
    /// Jump, left and right as last reported, to act on a press and not on a repeat.
    keys: [bool; 3],
    /// Where the look is, in the middle of the screen.
    look: Rotation,
    hovered: Option<Hover>,
    selected: Option<usize>,
    /// The chunk the client was last told to centre on.
    centre: Option<(i32, i32)>,
    streamed: (usize, usize),
    ticks: u32,
    last_click: u32,
}

impl MapSession {
    fn on_model(&self, x: f64, y: f64, z: f64) -> Position {
        Position {
            x: f64::from(self.anchor[0] * 16) + x,
            y,
            z: f64::from(self.anchor[1] * 16) + z,
        }
    }

    /// Where the player, or the entity they see through, stands on the model.
    fn standing(&self) -> [f64; 3] {
        match self.mode {
            MapMode::Look => [0.0, overview_y(), OVERVIEW_Z],
            MapMode::Pan => {
                let [x, z] = self.pan.offset();
                [x, detail_y(self.zoom), z + detail_back(self.zoom)]
            }
        }
    }

    /// The eye of the view, on the model.
    fn eye(&self) -> [f64; 3] {
        let [x, y, z] = self.standing();
        match self.mode {
            MapMode::Look => [x, y + SPECTATOR_EYE_HEIGHT, z],
            MapMode::Pan => [x, y, z],
        }
    }

    fn camera_position(&self) -> Position {
        let [x, y, z] = self.standing();
        self.on_model(x, y, z)
    }

    fn eye_box_position(&self) -> Position {
        let [x, y, z] = self.eye();
        self.on_model(x, y - EYE_BOX_DROP, z)
    }

    /// Takes in a rotation the client reports, and says how to correct it, if at all.
    fn turn(&mut self, yaw: f32, pitch: f32, atlas: &Atlas) -> Option<PlayerRotation> {
        self.rig?;
        match self.mode {
            MapMode::Look => {
                self.look = Rotation { yaw, pitch };
                let [held_yaw, held_pitch] = stop(yaw, pitch);
                (held_yaw.is_some() || held_pitch.is_some()).then_some(PlayerRotation {
                    yaw: held_yaw.unwrap_or(0.0),
                    relative_yaw: held_yaw.is_none(),
                    pitch: held_pitch.unwrap_or(0.0),
                    relative_pitch: held_pitch.is_none(),
                })
            }
            MapMode::Pan => {
                let turn = self.mouse.report(yaw, pitch);
                if turn.delta != [0.0, 0.0] {
                    let scene = &atlas.level(self.mode)?.scene;
                    self.pan.apply(turn.delta, |[x, z]| {
                        scene.top(x.floor() as i32, z.floor() as i32).is_some()
                    });
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

    /// The chunk under the camera, from the chunk at the model's centre.
    fn camera_chunk(&self) -> (i32, i32) {
        let [x, _, z] = self.standing();
        ((x / 16.0).floor() as i32, (z / 16.0).floor() as i32)
    }

    /// Far markers are large and near ones smaller, so that they keep their size on screen.
    fn marker_scale(&self) -> f32 {
        let zoom = match self.mode {
            MapMode::Look => 1.0,
            MapMode::Pan => ZOOMS[self.zoom] / ZOOMS[ZOOMS.len() - 1],
        };
        self.mode.scale().build as f32 * zoom as f32
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

    fn head_scale(&self) -> f32 {
        let base = HEAD_SCALE * self.marker_scale();
        if self.hovered == Some(Hover::You) {
            base * ICON_HOVER_GROWTH
        } else {
            base
        }
    }

    fn pin_transform(&self) -> DisplayTransform {
        pin_transform(self.head_scale(), PIN_SCALE * self.marker_scale())
    }

    fn place_pan(&mut self, atlas: &Atlas) {
        if let (MapMode::Pan, Some(level)) = (self.mode, atlas.level(self.mode)) {
            self.pan = self.pan_start(&level.scene);
        }
    }

    /// Where a pan starts: near the player, with their marker off the middle.
    fn pan_start(&self, scene: &Scene) -> Pan {
        let Some(you) = self.pins.you else {
            return Pan::default();
        };
        let [x, _, z] = you.at;
        let z = z - PAN_START_NORTH * ZOOMS[self.zoom];
        match scene.top(x.floor() as i32, z.floor() as i32) {
            Some(_) => Pan::at([x, z]),
            None => Pan::default(),
        }
    }
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        let info = dimension::register(&mut app.world_mut().resource_mut::<RegistryDataStore>());
        app.insert_resource(Atlas {
            info,
            regions: Regions::demo(),
            overview: None,
            detail: None,
        })
        .add_systems(Startup, build_atlas)
        .add_systems(Update, (rig_up, stream_chunks, follow_view, show_hint))
        .add_observer(enter)
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
        .add_observer(choose_style)
        .add_observer(player_left);
        app.world_mut()
            .resource_mut::<CommandRegistry>()
            .register(map_command());
    }
}

fn build_atlas(mut atlas: ResMut<Atlas>, generator: Res<WorldGen>) {
    let generator = generator.0.as_ref();
    let overview = Level::new(generator, OVERVIEW, &atlas.regions);
    let detail = Level::new(generator, DETAIL, &atlas.regions);
    tracing::info!(waypoints = overview.waypoints.len(), "map models built");
    atlas.overview = Some(overview);
    atlas.detail = Some(detail);
}

/// Asks for a player to enter the map, or to change mode if they are in it.
#[derive(Event, Debug, Clone, Copy)]
pub struct OpenMap(pub Entity, pub MapMode);

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

/// Under the head in the plane of the screen, from any angle.
fn pin_transform(head_scale: f32, pin_scale: f32) -> DisplayTransform {
    let drop = head_scale * HEAD_HALF + pin_scale * LINE_HEIGHT / PIXELS_PER_BLOCK;
    DisplayTransform::default()
        .translation(0.0, -drop, 0.0)
        .uniform_scale(pin_scale)
}

fn spawn_pin(commands: &mut Commands, at: Position, transform: DisplayTransform) -> Entity {
    EntityBuilder::new(EntityKind::TextDisplay)
        .position(at)
        .with_bundle((
            display(1.0).transform(transform),
            TextDisplay::new(PIN).background_color(CLEAR),
        ))
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
    Card {
        subject: Subject::Place(marker.kind),
        title: marker.name.clone(),
        kind: marker.kind.label().into(),
        info: format!("{} · {distance} blocs", marker.kind.label()),
        status: format!("{state}{}", region.name),
        description: marker.kind.description().into(),
        open: false,
    }
}

fn you_card() -> Card {
    Card {
        subject: Subject::You,
        title: "Vous êtes ici".into(),
        kind: "Vous".into(),
        info: "Votre position".into(),
        status: String::new(),
        description: String::new(),
        open: false,
    }
}

/// How a client shows a card, with the pack or without.
fn card_display(text: String, drawn: bool) -> TextDisplay {
    let background = if drawn { CLEAR } else { PLAIN_CARD_BACKGROUND };
    TextDisplay::new(text)
        .line_width(CARD_LINE_WIDTH)
        .background_color(background)
        .see_through()
}

/// Puts the markers of the session's mode on the model.
fn spawn_pins(atlas: &Atlas, session: &mut MapSession, commands: &mut Commands) {
    let Some(level) = atlas.level(session.mode) else {
        return;
    };
    let build = f64::from(session.mode.scale().build);
    let mut icons = Vec::with_capacity(level.waypoints.len());
    for waypoint in &level.waypoints {
        let [x, y, z] = waypoint.at;
        let at = session.on_model(x, y + ICON_LIFT * build, z);
        let scale = ICON_SCALE * session.marker_scale();
        icons.push(spawn_icon(commands, at, icon(waypoint.marker.kind), scale));
    }
    let head = session.head.clone();
    let head_scale = session.head_scale();
    let pin = session.pin_transform();
    let you = model_spot(level, &session.position).map(|at @ [x, y, z]| You {
        head: spawn_item(
            commands,
            session.on_model(x, y + HEAD_LIFT * build, z),
            head,
            // Facing the camera: as an icon, a head shows its hair more than its face.
            ItemDisplayContext::Fixed,
            head_scale,
        ),
        pin: spawn_pin(commands, session.on_model(x, y + HEAD_LIFT * build, z), pin),
        at,
    });
    session.pins = Pins {
        icons,
        you,
        card: None,
    };
}

fn model_spot(level: &Level, position: &Position) -> Option<[f64; 3]> {
    let scene = &level.scene;
    let (x, z) = scene.cell_of(position.x as i32, position.z as i32)?;
    let top = scene.top(x, z)?;
    Some([f64::from(x) + 0.5, f64::from(top + 1), f64::from(z) + 0.5])
}

type EntrantState<'a> = (
    &'a ClientId,
    &'a Position,
    &'a Rotation,
    Option<&'a PlayerAbilities>,
    Option<&'a mut TabEntry>,
    Option<&'a PlayerUuid>,
);

fn enter(
    event: On<OpenMap>,
    mut states: Query<EntrantState, Without<MapSession>>,
    atlas: Res<Atlas>,
    config: Res<ServerConfigResource>,
    players: Players,
    mut commands: Commands,
) {
    let (player, mode) = (event.0, event.1);
    if atlas.level(mode).is_none() {
        return;
    }
    let Ok((client, position, rotation, abilities, tab, id)) = states.get_mut(player) else {
        return;
    };
    // The client asks this entry, not `Respawn`, whether it is a spectator.
    let tab_game_mode = tab.and_then(|mut entry| entry.game_mode.replace(SPECTATOR));
    let slot = (client.0 % ANCHOR_SLOTS) as i32;
    let anchor = [
        FIRST_ANCHOR_CHUNK + slot * CHUNKS_BETWEEN_ANCHORS,
        FIRST_ANCHOR_CHUNK,
    ];

    players.send(
        player,
        Respawn {
            dimension_type: atlas.info.dimension_type,
            dimension_name: DIMENSION.into(),
            hashed_seed: 0,
            game_mode: SPECTATOR,
            previous_game_mode: config.game_mode as i8,
            is_debug: false,
            is_flat: true,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: scene::SEA_Y,
            data_to_keep: Respawn::KEEP_ALL,
        },
    );
    players.send(
        player,
        GameEvent {
            event: GameEventType::StartWaitingForLevelChunks,
            value: 0.0,
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
        anchor,
        position: *position,
        rotation: *rotation,
        abilities: abilities.copied(),
        tab_game_mode,
        pins: Pins::default(),
        bar,
        rig: None,
        mouse: Mouse::default(),
        pan: Pan::default(),
        zoom: DEFAULT_ZOOM,
        sized_for: DEFAULT_ZOOM,
        head: player_head(id),
        keys: [false; 3],
        look: Rotation {
            yaw: REST_YAW,
            pitch: REST_PITCH,
        },
        hovered: None,
        selected: None,
        centre: None,
        streamed: (0, 0),
        ticks: 0,
        last_click: 0,
    };
    spawn_pins(&atlas, &mut session, &mut commands);
    session.place_pan(&atlas);
    session.look = session.rest();
    let home = session.camera_position();
    let rest = session.look;
    commands
        .entity(player)
        .remove::<ChunkStreamBacklog>()
        .insert((
            session,
            ChunkStreamingPaused,
            ServerControlledPosition,
            LoadedChunks(HashSet::new()),
            PlayerAbilities::new()
                .invulnerable(true)
                .allow_flight(true)
                .flying(true)
                .flying_speed(0.0)
                .walking_speed(0.0),
            Teleport::to(home.x, home.y, home.z)
                .facing(rest.yaw, rest.pitch)
                .unthrottled()
                .preload_radius(0),
        ));
}

fn despawn_pins(session: &mut MapSession, commands: &mut Commands) {
    let pins = std::mem::take(&mut session.pins);
    let entities = pins
        .icons
        .into_iter()
        .chain(pins.you.into_iter().flat_map(|you| [you.head, you.pin]))
        .chain(pins.card.map(|card| card.entity));
    for entity in entities {
        commands.entity(entity).try_despawn();
    }
}

fn despawn_rig(session: &mut MapSession, commands: &mut Commands) {
    if let Some(rig) = session.rig.take() {
        for entity in rig.camera.into_iter().chain([rig.eye_box]) {
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
    if session.mode == mode {
        return;
    }
    tracing::info!(?mode, "map mode");
    despawn_rig(&mut session, &mut commands);
    despawn_pins(&mut session, &mut commands);
    players.send(player, SetCamera { camera_id: id.0 });
    for chunk in &loaded.0 {
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
    session.zoom = DEFAULT_ZOOM;
    session.sized_for = DEFAULT_ZOOM;
    session.look = session.rest();
    session.hovered = None;
    session.selected = None;
    session.centre = None;
    spawn_pins(&atlas, &mut session, &mut commands);
    session.place_pan(&atlas);
    let home = session.camera_position();
    let rest = session.look;
    commands.entity(player).insert((
        LoadedChunks(HashSet::new()),
        Teleport::to(home.x, home.y, home.z)
            .facing(rest.yaw, rest.pitch)
            .unthrottled()
            .preload_radius(0),
    ));
}

/// Whether a chunk belongs on the client, from the chunk at the model's centre.
fn wanted(mode: MapMode, chunk: (i32, i32), camera: (i32, i32), radius: i32) -> bool {
    match mode {
        MapMode::Look => {
            let range = -OVERVIEW_REACH..OVERVIEW_REACH;
            range.contains(&chunk.0) && range.contains(&chunk.1)
        }
        MapMode::Pan => {
            let (x, z) = (chunk.0 - camera.0, chunk.1 - camera.1);
            x * x + z * z <= radius * radius
        }
    }
}

fn chunks_around(mode: MapMode, camera: (i32, i32)) -> Vec<(i32, i32)> {
    let (centre, reach) = match mode {
        MapMode::Look => ((0, 0), OVERVIEW_REACH),
        MapMode::Pan => (camera, DETAIL_SEND_RADIUS),
    };
    let mut chunks: Vec<_> = (centre.0 - reach..=centre.0 + reach)
        .flat_map(|x| (centre.1 - reach..=centre.1 + reach).map(move |z| (x, z)))
        .filter(|chunk| wanted(mode, *chunk, camera, DETAIL_SEND_RADIUS))
        .collect();
    chunks.sort_by_key(|(x, z)| (x - camera.0).pow(2) + (z - camera.1).pow(2));
    chunks
}

fn stream_chunks(
    mut sessions: Query<(Entity, &mut MapSession, &LoadedChunks)>,
    mut atlas: ResMut<Atlas>,
    players: Players,
    mut commands: Commands,
) {
    let info = atlas.info;
    for (player, mut session, loaded) in &mut sessions {
        let (mode, anchor) = (session.mode, session.anchor);
        let Some(level) = atlas.level_mut(mode) else {
            continue;
        };
        let camera = session.camera_chunk();
        if session.centre != Some(camera) {
            session.centre = Some(camera);
            players.send(
                player,
                SetCenterChunk {
                    chunk_x: anchor[0] + camera.0,
                    chunk_z: anchor[1] + camera.1,
                },
            );
        }

        let stale: Vec<_> = loaded
            .0
            .iter()
            .copied()
            .filter(|chunk| {
                let on_model = (chunk.x - anchor[0], chunk.z - anchor[1]);
                !wanted(mode, on_model, camera, DETAIL_KEEP_RADIUS)
            })
            .collect();
        // `Players` reads this component, so it is replaced rather than changed here.
        let mut now_loaded = loaded.0.clone();
        for chunk in &stale {
            now_loaded.remove(chunk);
            players.send(
                player,
                UnloadChunk {
                    chunk_x: chunk.x,
                    chunk_z: chunk.z,
                },
            );
        }

        let missing: Vec<_> = chunks_around(mode, camera)
            .into_iter()
            .filter(|(x, z)| !now_loaded.contains(&ChunkPos::new(anchor[0] + x, anchor[1] + z)))
            .collect();
        for at in missing.iter().take(CHUNKS_PER_TICK) {
            let (chunk, weight) = level.chunk(*at, info);
            let (x, z) = (anchor[0] + at.0, anchor[1] + at.1);
            let mut placed = chunk.clone();
            (placed.x, placed.z) = (x, z);
            players.send(player, placed.to_packet());
            session.streamed.0 += 1;
            session.streamed.1 += *weight;
            now_loaded.insert(ChunkPos::new(x, z));
        }
        if missing.len() <= CHUNKS_PER_TICK && session.streamed.0 > 0 {
            let (chunks, bytes) = std::mem::take(&mut session.streamed);
            tracing::info!(
                ?mode,
                chunks,
                bytes,
                loaded = now_loaded.len(),
                "map chunks sent"
            );
        }
        if !stale.is_empty() || !missing.is_empty() {
            commands.entity(player).insert(LoadedChunks(now_loaded));
        }
    }
}

/// Once the player stands in the level: a teleport would throw them off a vehicle.
fn rig_up(
    mut sessions: Query<(Entity, &mut MapSession), Without<Teleport>>,
    mut commands: Commands,
) {
    for (player, mut session) in &mut sessions {
        if session.rig.is_some() {
            continue;
        }
        let eye_box = EntityBuilder::new(EYE_BOX_KIND)
            .position(session.eye_box_position())
            .gravity(false)
            .spawn(&mut commands)
            .id();
        // Riding the camera, the box glides with it exactly.
        let camera = (session.mode == MapMode::Pan).then(|| {
            EntityBuilder::new(EntityKind::ItemDisplay)
                .position(session.camera_position())
                .rotation(session.look.yaw, session.look.pitch)
                .with_bundle((
                    Display::default().teleport_ticks(CAMERA_GLIDE_TICKS),
                    Passengers::new([player, eye_box]),
                ))
                .spawn(&mut commands)
                .id()
        });
        session.rig = Some(Rig { eye_box, camera });
    }
}

fn look_through_camera(
    event: On<EntityShownEvent>,
    sessions: Query<&MapSession>,
    ids: Query<&MinecraftEntityId>,
    players: Players,
) {
    let Ok(session) = sessions.get(event.viewer) else {
        return;
    };
    if session.rig.and_then(|rig| rig.camera) == Some(event.entity)
        && let Ok(id) = ids.get(event.entity)
    {
        players.send(event.viewer, SetCamera { camera_id: id.0 });
    }
}

fn read_mouse(
    event: On<PacketEvent<SetPlayerRotation>>,
    mut sessions: Query<&mut MapSession>,
    atlas: Res<Atlas>,
    players: Players,
) {
    if let Ok(mut session) = sessions.get_mut(event.entity)
        && let Some(correction) = session.turn(event.packet.yaw, event.packet.pitch, &atlas)
    {
        players.send(event.entity, correction);
    }
}

/// Every 20 ticks the client sends its position along with any rotation, in this
/// packet instead of the other. A look that stops on that tick is only reported here.
fn read_mouse_with_position(
    event: On<PacketEvent<SetPlayerPosAndRot>>,
    mut sessions: Query<&mut MapSession>,
    atlas: Res<Atlas>,
    players: Players,
) {
    if let Ok(mut session) = sessions.get_mut(event.entity)
        && let Some(correction) = session.turn(event.packet.yaw, event.packet.pitch, &atlas)
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
    let home = session.camera_position();
    let moved = (event.packet.x - home.x).hypot(event.packet.z - home.z);
    if session.mode == MapMode::Look && session.rig.is_some() && moved > 0.01 {
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
            if let Some(you) = session.pins.you {
                set_scale(displays, you.head, session.head_scale());
                if let Ok(mut pin) = displays.get_mut(you.pin) {
                    pin.transform = session.pin_transform();
                }
            }
        }
    }
}

fn hover_of(level: &Level, session: &MapSession) -> Option<Hover> {
    let build = f64::from(session.mode.scale().build);
    let icons = level
        .waypoints
        .iter()
        .map(|w| [w.at[0], w.at[1] + ICON_LIFT * build, w.at[2]]);
    let you = session
        .pins
        .you
        .iter()
        .map(|you| [you.at[0], you.at[1] + HEAD_LIFT * build, you.at[2]]);
    let points: Vec<_> = icons.chain(you).collect();
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
        Hover::Waypoint(index) => Card {
            open: session.selected == Some(index),
            ..waypoint_card(atlas, &level.waypoints[index], &session.position)
        },
        Hover::You => you_card(),
    }
}

/// Where a marker's icon stands on the model, with how far up from there it reaches.
fn marker_top(level: &Level, session: &MapSession, hover: Hover) -> Option<([f64; 3], f32)> {
    let build = f64::from(session.mode.scale().build);
    match hover {
        Hover::Waypoint(index) => {
            let [x, y, z] = level.waypoints[index].at;
            let scale = session.icon_scale(index);
            Some(([x, y + ICON_LIFT * build, z], scale * ICON_HALF))
        }
        Hover::You => {
            let [x, y, z] = session.pins.you?.at;
            Some((
                [x, y + HEAD_LIFT * build, z],
                session.head_scale() * HEAD_HALF,
            ))
        }
    }
}

/// Keeps the card of the current subject on screen, and only that one.
fn show_card(
    atlas: &Atlas,
    level: &Level,
    session: &mut MapSession,
    style: Style,
    drawn: bool,
    commands: &mut Commands,
) {
    let wanted = subject(session).map(|hover| {
        let card = card_of(atlas, level, session, hover);
        let text = if drawn {
            card.drawn(style)
        } else {
            card.plain()
        };
        (hover, card.depth(style), text)
    });
    let zoom = session.zoom;
    let same = match (&session.pins.card, &wanted) {
        (Some(shown), Some((hover, _, text))) => {
            shown.hover == *hover && shown.zoom == zoom && shown.text == *text
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
    let display = card_display(text.clone(), drawn);
    session.pins.card = Some(ShownCard {
        entity: spawn_card(commands, at, display, scale, lift),
        text,
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
    mut sessions: Query<(&mut MapSession, Has<PackLoaded>, Option<&MapStyle>)>,
    mut positions: Query<&mut Position, Without<MapSession>>,
    atlas: Res<Atlas>,
    mut displays: Query<&mut Display>,
    mut bars: Query<&mut BossBar>,
    mut commands: Commands,
) {
    for (mut session, drawn, style) in &mut sessions {
        let (Some(rig), Some(level)) = (session.rig, atlas.level(session.mode)) else {
            continue;
        };
        // The box rides on the client, but the engine goes by this to keep it shown.
        let moves = [
            (rig.camera, session.camera_position()),
            (Some(rig.eye_box), session.eye_box_position()),
        ];
        for (entity, at) in moves {
            if let Some(mut position) = entity.and_then(|entity| positions.get_mut(entity).ok()) {
                position.set_if_neq(at);
            }
        }

        let hovered = hover_of(level, &session);
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
        if session.sized_for != session.zoom {
            session.sized_for = session.zoom;
            for index in 0..session.pins.icons.len() {
                resize(&session, Hover::Waypoint(index), &mut displays);
            }
            resize(&session, Hover::You, &mut displays);
        }
        let style = style.map_or(Style::default(), |style| style.0);
        show_card(&atlas, level, &mut session, style, drawn, &mut commands);
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
fn left_click(
    event: On<PacketEvent<SpectateEntity>>,
    mut sessions: Query<&mut MapSession>,
    ids: Query<&MinecraftEntityId>,
    atlas: Res<Atlas>,
    mut displays: Query<&mut Display>,
    messages: Messages,
) {
    let player = event.entity;
    let Ok(mut session) = sessions.get_mut(player) else {
        return;
    };
    if !counts_as_click(&mut session, event.packet.entity_id, &ids) {
        return;
    }
    let Some(Hover::Waypoint(index)) = session.hovered else {
        tracing::info!(?player, "map left click on no marker");
        return;
    };
    let Some(level) = atlas.level(session.mode) else {
        return;
    };
    let marker = &level.waypoints[index].marker;
    let region = atlas.regions.at(f64::from(marker.x), f64::from(marker.z));
    tracing::info!(?player, marker = %marker.name, state = ?region.state, "map left click");
    if region.state == RegionState::Discovered {
        select(&mut session, Some(index), &mut displays);
        messages
            .message(player, format!("{} sélectionné.", marker.name))
            .send();
    } else {
        let message = format!("{} : {}.", region.name, region.state.label());
        messages.message(player, message).send();
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
        .append(key("key.sneak"))
        .append(Text::new(" : fermer"))
}

fn show_hint(mut sessions: Query<(Entity, &mut MapSession)>, players: Players) {
    for (player, mut session) in &mut sessions {
        session.ticks += 1;
        if session.ticks.is_multiple_of(HINT_PERIOD) {
            players.send(
                player,
                SystemChat {
                    content: hint().to_nbt(),
                    overlay: true,
                },
            );
        }
    }
}

fn read_keys(
    event: On<PlayerInputEvent>,
    mut sessions: Query<&mut MapSession>,
    mut commands: Commands,
) {
    let Ok(mut session) = sessions.get_mut(event.entity) else {
        return;
    };
    let keys = [event.jump, event.left, event.right];
    let [jump, out, closer] = std::array::from_fn(|key| keys[key] && !session.keys[key]);
    session.keys = keys;
    if event.sneak {
        commands.trigger(CloseMap(event.entity));
    } else if jump && session.rig.is_some() {
        commands.trigger(OpenMap(event.entity, session.mode.other()));
    } else if session.mode == MapMode::Pan {
        let zoom = session.zoom + usize::from(out);
        session.zoom = zoom
            .saturating_sub(usize::from(closer))
            .min(ZOOMS.len() - 1);
    }
}

/// The look of a player's cards, kept between sessions.
#[derive(Component, Debug, Clone, Copy)]
struct MapStyle(Style);

/// Sets a player's style of card, or moves on to the next one.
#[derive(Event, Debug, Clone, Copy)]
pub struct ChooseStyle(pub Entity, pub Option<Style>);

fn choose_style(
    event: On<ChooseStyle>,
    styles: Query<&MapStyle>,
    messages: Messages,
    mut commands: Commands,
) {
    let player = event.0;
    let current = styles.get(player).map_or(Style::default(), |style| style.0);
    let style = event.1.unwrap_or(current.next());
    commands.entity(player).insert(MapStyle(style));
    messages
        .message(player, format!("Panneau : {}", style.label()))
        .send();
}

/// The client has the pack, so it can draw cards.
#[derive(Component)]
struct PackLoaded;

fn note_pack(event: On<PackSettled>, mut commands: Commands) {
    if event.loaded {
        commands.entity(event.player).insert(PackLoaded);
    }
}

fn leave(
    event: On<CloseMap>,
    mut sessions: Query<(&mut MapSession, &MinecraftEntityId, Option<&mut TabEntry>)>,
    config: Res<ServerConfigResource>,
    players: Players,
    mut commands: Commands,
) {
    let player = event.0;
    let Ok((mut session, id, tab)) = sessions.get_mut(player) else {
        return;
    };
    tracing::info!(?player, "leaving the map");
    if let Some(mut entry) = tab {
        entry.game_mode = session.tab_game_mode.or(Some(config.game_mode));
    }
    players.send(
        player,
        Respawn {
            dimension_type: OVERWORLD_TYPE,
            dimension_name: OVERWORLD.into(),
            hashed_seed: 0,
            game_mode: config.game_mode,
            previous_game_mode: SPECTATOR as i8,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: OVERWORLD_SEA_LEVEL,
            data_to_keep: Respawn::KEEP_ALL,
        },
    );
    players.send(player, SetCamera { camera_id: id.0 });
    players.send(
        player,
        GameEvent {
            event: GameEventType::StartWaitingForLevelChunks,
            value: 0.0,
        },
    );
    despawn_rig(&mut session, &mut commands);
    despawn_pins(&mut session, &mut commands);
    commands.entity(session.bar).try_despawn();
    let Position { x, y, z } = session.position;
    let (yaw, pitch) = (session.rotation.yaw, session.rotation.pitch);
    let abilities = session.abilities;
    let mut back = commands.entity(player);
    back.remove::<(MapSession, ChunkStreamingPaused, ServerControlledPosition)>();
    match abilities {
        Some(abilities) => back.insert(abilities),
        None => back.remove::<PlayerAbilities>(),
    };
    back.insert((
        LoadedChunks(HashSet::new()),
        // Not a real distance: makes the next streaming pass start over.
        EffectiveViewDistance(0),
        ChunkStreamBacklog::default(),
        Teleport::to(x, y, z).facing(yaw, pitch),
    ));
}

fn player_left(
    event: On<PlayerQuitEvent>,
    mut sessions: Query<&mut MapSession>,
    mut commands: Commands,
) {
    if let Ok(mut session) = sessions.get_mut(event.entity) {
        despawn_rig(&mut session, &mut commands);
        despawn_pins(&mut session, &mut commands);
        commands.entity(session.bar).try_despawn();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapAction {
    Look,
    Pan,
    Stop,
    Style,
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
            ]),
        )
        .arg_optional(
            "style",
            EnumArg::new([("1", Style::Card), ("2", Style::Band), ("3", Style::Tag)]),
        )
        .handler(run_map)
        .build()
}

/// The engine's arguments form one chain, so a style can follow any action.
fn run_map(ctx: &mut CommandContext) {
    let player = ctx.entity;
    let action = ctx.get::<MapAction>("action").copied();
    let style = ctx.get::<Style>("style").copied();
    ctx.with_world_mut(|world| {
        match action.unwrap_or(MapAction::Look) {
            MapAction::Look => world.trigger(OpenMap(player, MapMode::Look)),
            MapAction::Pan => world.trigger(OpenMap(player, MapMode::Pan)),
            MapAction::Stop => world.trigger(CloseMap(player)),
            MapAction::Style => {}
        }
        if style.is_some() || action == Some(MapAction::Style) {
            world.trigger(ChooseStyle(player, style));
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
        for action in ["key.jump", "key.left", "key.right", "key.sneak"] {
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
        assert_eq!(actions.matches, ["look", "pan", "stop", "style"]);
        let styles = registry.complete("map style ", &world).expect("styles");
        assert_eq!(styles.matches, ["1", "2", "3"]);
    }

    /// A look that stopped on the client's position tick used to go unseen, so that
    /// the marker under the crosshair stayed unhovered until the mouse moved again.
    #[test]
    fn a_look_reported_with_the_position_counts() {
        let mut app = engine();
        let world = app.world_mut();
        let entity = world.spawn_empty().id();
        let session = MapSession {
            mode: MapMode::Look,
            anchor: [0, 0],
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
            bar: entity,
            rig: Some(Rig {
                eye_box: entity,
                camera: None,
            }),
            mouse: Mouse::default(),
            pan: Pan::default(),
            zoom: DEFAULT_ZOOM,
            sized_for: DEFAULT_ZOOM,
            head: item("minecraft:player_head"),
            keys: [false; 3],
            look: Rotation {
                yaw: REST_YAW,
                pitch: REST_PITCH,
            },
            hovered: None,
            selected: None,
            centre: None,
            streamed: (0, 0),
            ticks: 0,
            last_click: 0,
        };
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
    fn a_chunk_stays_until_it_is_past_the_keep_radius() {
        let chunk = (DETAIL_SEND_RADIUS + 1, 0);
        assert!(wanted(MapMode::Pan, chunk, (0, 0), DETAIL_KEEP_RADIUS));
        assert!(!wanted(
            MapMode::Pan,
            (DETAIL_KEEP_RADIUS + 1, 0),
            (0, 0),
            DETAIL_KEEP_RADIUS
        ));
    }

    #[test]
    fn a_card_says_what_a_waypoint_is_and_what_can_be_done() {
        let atlas = Atlas {
            info: MapLevel {
                dimension_type: 0,
                biome: voidmc::BiomeId::plains(),
                biome_count: 1,
            },
            regions: Regions::demo(),
            overview: None,
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
