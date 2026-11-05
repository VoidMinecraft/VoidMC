//! Takes players into the map and back.
//!
//! In the map a player is a spectator who cannot move. In [`MapMode::Look`] they
//! look around freely, with no delay since the client turns the view. In
//! [`MapMode::Pan`] they ride an invisible entity and see through another one,
//! which the mouse moves. Either way the middle of the screen is the pointer.

use std::collections::HashSet;
use std::sync::Arc;

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::prelude::*;
use voidmc::{
    Billboard, BossBar, BossBarColor, Command, CommandBuilder, CommandContext, CommandRegistry,
    Display, DisplayTransform, EntityBuilder, EntityKind, EntityShownEvent, Glowing,
    GreedyStringArg, ItemDisplay, ItemDisplayContext, ItemStack, Messages, Passengers,
    PlayerAbilities, Players, RegistryDataStore, ServerConfigResource, TabEntry, Teleport,
    TextDisplay, WorldGen,
    components::{
        ChunkStreamBacklog, ChunkStreamingPaused, ClientId, EffectiveViewDistance, LoadedChunks,
        MinecraftEntityId, Position, Rotation, ServerControlledPosition,
    },
    events::{PlayerInputEvent, PlayerQuitEvent},
    network::PacketEvent,
    world::ChunkPos,
};
use voidmc_protocol::{
    clientbound::{
        Chunk, GameEvent, GameEventType, PlayerRotation, Respawn, SetCamera, SetCenterChunk,
    },
    serverbound::{Interact, SetPlayerPos, SetPlayerRotation, SpectateEntity},
};

use crate::dimension::{self, DIMENSION, MapLevel};
use crate::markers::{Marker, MarkerField, MarkerKind};
use crate::pointer::{
    CAMERA_Y, CAMERA_Z, Mouse, Pan, REST_PITCH, REST_YAW, look_direction, nearest_to_ray, stop,
};
use crate::scene::{self, Scene};
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

const CAMERA_GLIDE_TICKS: u8 = 3;

/// How far from the middle of the screen, in degrees, a marker still counts.
const AIM_ANGLE: f64 = 4.0;
const ICON_LIFT: f64 = 4.0;
const LABEL_LIFT: f64 = 9.0;
const ICON_SCALE: f32 = 7.0;
const ICON_HOVER_SCALE: f32 = 11.0;
const LABEL_SCALE: f32 = 6.0;
/// A spectator only clicks when the look hits an entity, and only sees the
/// crosshair when that entity has a menu. A chest minecart around the eye is always
/// hit. The eye sits under its floor, looking down, so the cart never shows.
const EYE_BOX_KIND: EntityKind = EntityKind::ChestMinecart;
const EYE_BOX_DROP: f64 = 0.02;
const SPECTATOR_EYE_HEIGHT: f64 = 1.62;
/// Displays vanish past 64 blocks times this, and the camera is further than that.
const VIEW_RANGE: f32 = 8.0;
const GLOW: i32 = 0xFFE9A0;
const LABEL_BACKGROUND: i32 = 0x8000_0000_u32 as i32;
const HINT_PERIOD: u32 = 30;
/// A right click sends one packet per hand.
const CLICK_COOLDOWN: u32 = 3;
const HINT: &str = "S'accroupir pour fermer la carte";
const TITLE: &str = "Carte du monde";

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
    let name = match kind {
        MarkerKind::Village => "minecraft:emerald",
        MarkerKind::Ruins => "minecraft:bone",
        MarkerKind::Mine => "minecraft:iron_pickaxe",
        MarkerKind::Lighthouse => "minecraft:lantern",
    };
    item(name)
}

/// What every player's map shows, built once from the server's generator.
#[derive(Resource)]
pub struct Atlas {
    level: MapLevel,
    scene: Option<Scene>,
    chunks: Vec<Chunk>,
    waypoints: Vec<Waypoint>,
}

#[derive(Debug, Clone, PartialEq)]
struct Waypoint {
    marker: Marker,
    /// On the model, from its centre.
    at: [f64; 3],
}

struct Pin {
    icon: Entity,
}

/// How the mouse drives the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapMode {
    /// The player looks around. The view reacts with no delay.
    #[default]
    Look,
    /// The mouse slides the view, whose look never changes.
    Pan,
}

/// The entities a session adds once the player is in place.
#[derive(Debug, Clone, Copy)]
struct Rig {
    eye_box: Entity,
    /// Pan mode only: what the player rides, and what they see through.
    pan: Option<[Entity; 2]>,
}

/// A player in the map, and how to put them back.
#[derive(Component)]
pub struct MapSession {
    mode: MapMode,
    origin: [f64; 2],
    position: Position,
    rotation: Rotation,
    abilities: Option<PlayerAbilities>,
    tab_game_mode: Option<u8>,
    pins: Vec<Pin>,
    you: Option<Entity>,
    /// The name above the hovered marker.
    label: Option<Entity>,
    bar: Entity,
    rig: Option<Rig>,
    mouse: Mouse,
    pan: Pan,
    /// Where the look is, in the middle of the screen.
    look: Rotation,
    hovered: Option<usize>,
    selected: Option<usize>,
    ticks: u32,
    last_click: u32,
}

impl MapSession {
    fn on_model(&self, x: f64, y: f64, z: f64) -> Position {
        Position {
            x: self.origin[0] + x,
            y,
            z: self.origin[1] + z,
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

    /// Where the player, or the entity they see through, stands on the model.
    fn standing(&self) -> [f64; 3] {
        let [x, z] = self.pan.offset();
        [x, CAMERA_Y, z + CAMERA_Z]
    }

    fn camera_position(&self) -> Position {
        let [x, y, z] = self.standing();
        self.on_model(x, y, z)
    }

    fn eye_box_position(&self) -> Position {
        let [x, y, z] = self.eye();
        self.on_model(x, y - EYE_BOX_DROP, z)
    }

    fn pin_scale(&self, index: usize) -> f32 {
        if self.hovered == Some(index) || self.selected == Some(index) {
            ICON_HOVER_SCALE
        } else {
            ICON_SCALE
        }
    }
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        let level = dimension::register(&mut app.world_mut().resource_mut::<RegistryDataStore>());
        app.insert_resource(Atlas {
            level,
            scene: None,
            chunks: Vec::new(),
            waypoints: Vec::new(),
        })
        .add_systems(Startup, build_atlas)
        .add_systems(Update, (rig_up, follow_view, show_hint))
        .add_observer(enter)
        .add_observer(switch_mode)
        .add_observer(leave)
        .add_observer(look_through_camera)
        .add_observer(read_mouse)
        .add_observer(hold_still)
        .add_observer(left_click)
        .add_observer(right_click)
        .add_observer(crouch)
        .add_observer(player_left);
        app.world_mut()
            .resource_mut::<CommandRegistry>()
            .register(map_command());
    }
}

fn build_atlas(mut atlas: ResMut<Atlas>, generator: Res<WorldGen>) {
    let window = scene::window();
    let terrain = Terrain::sample(generator.0.as_ref(), &window);
    let mut scene = Scene::new(&terrain, SEA_LEVEL);
    let mut cells = Vec::new();
    atlas.waypoints = MARKERS
        .place(generator.0.as_ref())
        .into_iter()
        .filter_map(|marker| {
            let (x, z) = Scene::cell_of(&window, marker.x, marker.z)?;
            let top = scene.top(x, z)?;
            cells.push((x, z));
            Some(Waypoint {
                marker,
                at: [f64::from(x) + 0.5, f64::from(top + 1), f64::from(z) + 0.5],
            })
        })
        .collect();
    for (index, cell) in cells.iter().enumerate() {
        let others = cells
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index);
        let distance = |c: &(i32, i32)| (c.0 - cell.0).pow(2) + (c.1 - cell.1).pow(2);
        if let Some((_, nearest)) = others.min_by_key(|(_, c)| distance(c)) {
            scene.pave(*cell, *nearest);
        }
    }
    atlas.chunks = scene.chunks(atlas.level.biome, atlas.level.biome_count);
    for waypoint in &atlas.waypoints {
        tracing::debug!(name = %waypoint.marker.name, at = ?waypoint.at, "waypoint");
    }
    tracing::info!(
        chunks = atlas.chunks.len(),
        waypoints = atlas.waypoints.len(),
        "map model built"
    );
    atlas.scene = Some(scene);
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

fn spawn_icon(commands: &mut Commands, at: Position, item: ItemStack) -> Entity {
    EntityBuilder::new(EntityKind::ItemDisplay)
        .position(at)
        .with_bundle((
            display(ICON_SCALE),
            ItemDisplay::new(item).context(ItemDisplayContext::Gui),
        ))
        .spawn(commands)
        .id()
}

fn spawn_label(commands: &mut Commands, at: Position, text: &str) -> Entity {
    EntityBuilder::new(EntityKind::TextDisplay)
        .position(at)
        .with_bundle((
            display(LABEL_SCALE),
            TextDisplay::new(text).background_color(LABEL_BACKGROUND),
        ))
        .spawn(commands)
        .id()
}

/// Model chunks at their place for this player, and the set the engine tracks.
fn send_chunks(players: &Players, player: Entity, atlas: &Atlas, anchor: [i32; 2]) -> LoadedChunks {
    let mut loaded = HashSet::new();
    for chunk in &atlas.chunks {
        let (x, z) = (chunk.x + anchor[0], chunk.z + anchor[1]);
        let mut placed = chunk.clone();
        (placed.x, placed.z) = (x, z);
        players.send(player, placed.to_packet());
        loaded.insert(ChunkPos::new(x, z));
    }
    LoadedChunks(loaded)
}

type EntrantState<'a> = (
    &'a ClientId,
    &'a Position,
    &'a Rotation,
    Option<&'a PlayerAbilities>,
    Option<&'a mut TabEntry>,
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
    if atlas.chunks.is_empty() {
        return;
    }
    let Ok((client, position, rotation, abilities, tab)) = states.get_mut(player) else {
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
            dimension_type: atlas.level.dimension_type,
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
    // The client keeps and draws chunks around this, and around its camera.
    players.send(
        player,
        SetCenterChunk {
            chunk_x: anchor[0],
            chunk_z: anchor[1] + CAMERA_Z as i32 / 16,
        },
    );
    let loaded = send_chunks(&players, player, &atlas, anchor);

    let origin = [f64::from(anchor[0] * 16), f64::from(anchor[1] * 16)];
    let lifted = |at: [f64; 3], lift: f64| Position {
        x: origin[0] + at[0],
        y: at[1] + lift,
        z: origin[1] + at[2],
    };
    let pins = atlas
        .waypoints
        .iter()
        .map(|waypoint| Pin {
            icon: spawn_icon(
                &mut commands,
                lifted(waypoint.at, ICON_LIFT),
                icon(waypoint.marker.kind),
            ),
        })
        .collect();
    let you = model_spot(&atlas, position)
        .map(|at| spawn_label(&mut commands, lifted(at, LABEL_LIFT), "VOUS"));
    let bar = commands
        .spawn(
            BossBar::new(TITLE)
                .color(BossBarColor::Green)
                .viewers([player]),
        )
        .id();

    commands
        .entity(player)
        .remove::<ChunkStreamBacklog>()
        .insert((
            MapSession {
                mode,
                origin,
                position: *position,
                rotation: *rotation,
                abilities: abilities.copied(),
                tab_game_mode,
                pins,
                you,
                label: None,
                bar,
                rig: None,
                mouse: Mouse::default(),
                pan: Pan::default(),
                look: Rotation {
                    yaw: REST_YAW,
                    pitch: REST_PITCH,
                },
                hovered: None,
                selected: None,
                ticks: 0,
                last_click: 0,
            },
            ChunkStreamingPaused,
            ServerControlledPosition,
            loaded,
            PlayerAbilities::new()
                .invulnerable(true)
                .allow_flight(true)
                .flying(true)
                .flying_speed(0.0)
                .walking_speed(0.0),
            Teleport::to(origin[0], CAMERA_Y, origin[1] + CAMERA_Z)
                .facing(REST_YAW, REST_PITCH)
                .unthrottled()
                .preload_radius(0),
        ));
}

/// Takes the rig down: `rig_up` builds the new one once the player is back in place.
fn switch_mode(
    event: On<OpenMap>,
    mut sessions: Query<(&mut MapSession, &MinecraftEntityId)>,
    players: Players,
    mut commands: Commands,
) {
    let (player, mode) = (event.0, event.1);
    let Ok((mut session, id)) = sessions.get_mut(player) else {
        return;
    };
    if session.mode == mode {
        return;
    }
    if let Some(rig) = session.rig.take() {
        for entity in rig.pan.into_iter().flatten().chain([rig.eye_box]) {
            commands.entity(entity).try_despawn();
        }
    }
    players.send(player, SetCamera { camera_id: id.0 });
    session.mode = mode;
    session.pan = Pan::default();
    session.mouse = Mouse::default();
    let home = session.camera_position();
    commands.entity(player).insert(
        Teleport::to(home.x, home.y, home.z)
            .facing(REST_YAW, REST_PITCH)
            .preload_radius(0),
    );
}

fn model_spot(atlas: &Atlas, position: &Position) -> Option<[f64; 3]> {
    let scene = atlas.scene.as_ref()?;
    let (x, z) = Scene::cell_of(&scene::window(), position.x as i32, position.z as i32)?;
    let top = scene.top(x, z)?;
    Some([f64::from(x) + 0.5, f64::from(top + 1), f64::from(z) + 0.5])
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
        let pan = (session.mode == MapMode::Pan).then(|| {
            let vehicle = EntityBuilder::new(EntityKind::ItemDisplay)
                .position(session.camera_position())
                .with(Passengers::new([player]))
                .spawn(&mut commands)
                .id();
            // Riding the camera, the box glides with it exactly.
            let camera = EntityBuilder::new(EntityKind::ItemDisplay)
                .position(session.camera_position())
                .rotation(REST_YAW, REST_PITCH)
                .with_bundle((
                    Display::default().teleport_ticks(CAMERA_GLIDE_TICKS),
                    Passengers::new([eye_box]),
                ))
                .spawn(&mut commands)
                .id();
            [vehicle, camera]
        });
        session.rig = Some(Rig { eye_box, pan });
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
    let camera = session
        .rig
        .and_then(|rig| rig.pan)
        .map(|[_, camera]| camera);
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
    let Ok(mut session) = sessions.get_mut(event.entity) else {
        return;
    };
    if session.rig.is_none() {
        return;
    }
    let (yaw, pitch) = (event.packet.yaw, event.packet.pitch);
    let correction = match session.mode {
        MapMode::Look => {
            session.look = Rotation { yaw, pitch };
            let [held_yaw, held_pitch] = stop(yaw, pitch);
            (held_yaw.is_some() || held_pitch.is_some()).then_some(PlayerRotation {
                yaw: held_yaw.unwrap_or(0.0),
                relative_yaw: held_yaw.is_none(),
                pitch: held_pitch.unwrap_or(0.0),
                relative_pitch: held_pitch.is_none(),
            })
        }
        MapMode::Pan => {
            let turn = session.mouse.report(yaw, pitch);
            if turn.delta != [0.0, 0.0] {
                session.pan.apply(turn.delta);
            }
            turn.recentre.map(|pitch| PlayerRotation {
                yaw: 0.0,
                relative_yaw: true,
                pitch,
                relative_pitch: true,
            })
        }
    };
    if let Some(correction) = correction {
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
    let eye = session.camera_position();
    let moved = (event.packet.x - eye.x).hypot(event.packet.z - eye.z);
    if session.mode == MapMode::Look && session.rig.is_some() && moved > 0.01 {
        let mut player = commands.entity(event.entity);
        if let Ok(held) = abilities.get(event.entity) {
            player.insert(*held);
        }
        player.insert(Teleport::to(eye.x, eye.y, eye.z).preload_radius(0));
    }
}

fn set_scale(displays: &mut Query<&mut Display>, entity: Entity, scale: f32) {
    if let Ok(mut display) = displays.get_mut(entity) {
        display.transform = DisplayTransform::default().uniform_scale(scale);
    }
}

fn bar_title(atlas: &Atlas, session: &MapSession) -> String {
    match session.hovered {
        Some(index) => {
            let marker = &atlas.waypoints[index].marker;
            format!("{} - {}", marker.name, marker.kind.label())
        }
        None => TITLE.to_string(),
    }
}

fn follow_view(
    mut sessions: Query<&mut MapSession, Changed<MapSession>>,
    mut positions: Query<&mut Position, Without<MapSession>>,
    atlas: Res<Atlas>,
    mut displays: Query<&mut Display>,
    mut bars: Query<&mut BossBar>,
    mut commands: Commands,
) {
    for mut session in &mut sessions {
        let Some(rig) = session.rig else {
            continue;
        };
        if let Some([_, camera]) = rig.pan
            && let Ok(mut position) = positions.get_mut(camera)
        {
            position.set_if_neq(session.camera_position());
        }

        let direction = look_direction(session.look.yaw, session.look.pitch);
        let icons = atlas
            .waypoints
            .iter()
            .map(|waypoint| [waypoint.at[0], waypoint.at[1] + ICON_LIFT, waypoint.at[2]]);
        let hovered = nearest_to_ray(session.eye(), direction, icons, AIM_ANGLE);
        if hovered != session.hovered {
            let previous = std::mem::replace(&mut session.hovered, hovered);
            for index in [previous, hovered].into_iter().flatten() {
                let scale = session.pin_scale(index);
                set_scale(&mut displays, session.pins[index].icon, scale);
            }
            if let Some(index) = previous {
                commands
                    .entity(session.pins[index].icon)
                    .remove::<Glowing>();
            }
            if let Some(label) = session.label.take() {
                commands.entity(label).try_despawn();
            }
            if let Some(index) = hovered {
                let waypoint = &atlas.waypoints[index];
                commands.entity(session.pins[index].icon).insert(Glowing);
                let [x, y, z] = waypoint.at;
                let at = session.on_model(x, y + LABEL_LIFT, z);
                session.label = Some(spawn_label(&mut commands, at, &waypoint.marker.name));
                tracing::info!(marker = %waypoint.marker.name, "map hover");
            }
        }
        let title = bar_title(&atlas, &session);
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
        let scale = session.pin_scale(old);
        set_scale(displays, session.pins[old].icon, scale);
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
    let Some(index) = session.hovered else {
        tracing::info!(?player, "map left click on no marker");
        return;
    };
    select(&mut session, Some(index), &mut displays);
    let marker = &atlas.waypoints[index].marker;
    tracing::info!(?player, marker = %marker.name, "map left click");
    messages
        .message(
            player,
            format!(
                "{} ({}) - X {}  Z {}, altitude {}",
                marker.name,
                marker.kind.label(),
                marker.x,
                marker.z,
                marker.height
            ),
        )
        .send();
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

fn show_hint(mut sessions: Query<(Entity, &mut MapSession)>, messages: Messages) {
    for (player, mut session) in &mut sessions {
        session.ticks += 1;
        if session.ticks.is_multiple_of(HINT_PERIOD) {
            messages.action_bar(player, HINT).send();
        }
    }
}

fn despawn_all(session: &MapSession, commands: &mut Commands) {
    let pins = session.pins.iter().map(|pin| pin.icon);
    let rig = session.rig.iter().flat_map(|rig| {
        let pan = rig.pan.into_iter().flatten();
        pan.chain([rig.eye_box])
    });
    for entity in pins
        .chain(rig)
        .chain(session.you)
        .chain(session.label)
        .chain([session.bar])
    {
        commands.entity(entity).try_despawn();
    }
}

fn crouch(event: On<PlayerInputEvent>, mut commands: Commands) {
    if event.sneak {
        commands.trigger(CloseMap(event.entity));
    }
}

fn leave(
    event: On<CloseMap>,
    mut sessions: Query<(&MapSession, &MinecraftEntityId, Option<&mut TabEntry>)>,
    config: Res<ServerConfigResource>,
    players: Players,
    mut commands: Commands,
) {
    let player = event.0;
    let Ok((session, id, tab)) = sessions.get_mut(player) else {
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
    despawn_all(session, &mut commands);
    let Position { x, y, z } = session.position;
    let mut back = commands.entity(player);
    back.remove::<(MapSession, ChunkStreamingPaused, ServerControlledPosition)>();
    match &session.abilities {
        Some(abilities) => back.insert(*abilities),
        None => back.remove::<PlayerAbilities>(),
    };
    back.insert((
        LoadedChunks(HashSet::new()),
        // Not a real distance: makes the next streaming pass start over.
        EffectiveViewDistance(0),
        ChunkStreamBacklog::default(),
        Teleport::to(x, y, z).facing(session.rotation.yaw, session.rotation.pitch),
    ));
}

fn player_left(event: On<PlayerQuitEvent>, sessions: Query<&MapSession>, mut commands: Commands) {
    if let Ok(session) = sessions.get(event.entity) {
        despawn_all(session, &mut commands);
    }
}

fn map_command() -> Command {
    CommandBuilder::new("map")
        .description("Ouvre la carte du monde : /map [look|pan|stop]")
        .arg_optional("mode", Arc::new(GreedyStringArg))
        .handler(open_map)
        .build()
}

fn open_map(ctx: &mut CommandContext) {
    let player = ctx.entity;
    match ctx.get::<String>("mode").map(String::as_str) {
        None | Some("look") => ctx.with_world_mut(|w| w.trigger(OpenMap(player, MapMode::Look))),
        Some("pan") => ctx.with_world_mut(|w| w.trigger(OpenMap(player, MapMode::Pan))),
        Some("stop") => ctx.with_world_mut(|w| w.trigger(CloseMap(player))),
        Some(_) => ctx.reply("Usage : /map [look|pan|stop]"),
    }
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

    /// Bevy only reports conflicting system params when the schedule first runs.
    #[test]
    fn the_plugin_ticks_with_the_full_engine() {
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

        app.update();
        app.update();
        let atlas = app.world().resource::<Atlas>();
        assert!(!atlas.chunks.is_empty());
        assert!(!atlas.waypoints.is_empty());
        assert!(atlas.waypoints.iter().all(|w| {
            w.at[0].abs() < f64::from(scene::RADIUS) && w.at[2].abs() < f64::from(scene::RADIUS)
        }));
    }
}
