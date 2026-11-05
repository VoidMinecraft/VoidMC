//! Takes players into the map and back.
//!
//! In the map a player is a spectator riding an invisible entity, seeing through
//! another one. The camera never follows the mouse. A rider keeps reporting its
//! rotation, so the mouse still reaches the server, which draws a cursor with it.

use std::collections::HashSet;

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::prelude::*;
use voidmc::{
    Billboard, BossBar, BossBarColor, Command, CommandBuilder, CommandContext, CommandRegistry,
    Display, DisplayTransform, EntityBuilder, EntityKind, EntityMetadata, EntityShownEvent,
    Glowing, ItemDisplay, ItemDisplayContext, ItemStack, Messages, MetadataSource,
    MetadataSourceAppExt, Passengers, PlayerAbilities, Players, RegistryDataStore,
    ServerConfigResource, TabEntry, Teleport, TextDisplay, WorldGen,
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
        Chunk, EntityMetadataValue, GameEvent, GameEventType, PlayerRotation, Respawn, SetCamera,
        SetCenterChunk,
    },
    serverbound::{Interact, SetPlayerRotation, SpectateEntity},
};

use crate::dimension::{self, DIMENSION, MapLevel};
use crate::markers::{Marker, MarkerField, MarkerKind};
use crate::pointer::{Mouse, Pointer};
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

const CAMERA_Y: f64 = 190.0;
/// South of the view's centre, so that the tilted camera looks at it.
const CAMERA_BACK: f64 = 40.0;
/// North, tilted down. Must agree with the axes of [`Pointer::apply`].
const CAMERA_YAW: f32 = 180.0;
const CAMERA_PITCH: f32 = 72.0;
const CAMERA_GLIDE_TICKS: u8 = 3;
const CURSOR_GLIDE_TICKS: u8 = 2;

/// How close to a marker, in model blocks, the cursor must come.
const HOVER_RADIUS: f64 = 7.0;
const ICON_LIFT: f64 = 4.0;
const LABEL_LIFT: f64 = 9.0;
const CURSOR_LIFT: f64 = 3.0;
const ICON_SCALE: f32 = 10.0;
const ICON_HOVER_SCALE: f32 = 15.0;
const LABEL_SCALE: f32 = 5.0;
const CURSOR_SCALE: f32 = 8.0;
/// Larger than the camera's glide, which it has to contain at all times.
const HITBOX_SIZE: f32 = 8.0;
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
    count: 14,
    radius: 300,
    min_spacing: 64,
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

/// An invisible box around the camera. A spectator only sends a click when the
/// camera's look hits an entity, and it never looks at anything else.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct Hitbox;

impl MetadataSource for Hitbox {
    fn write(&self, meta: &mut EntityMetadata) {
        // Entity data of `Interaction`: width, then height.
        meta.set(8, EntityMetadataValue::Float(HITBOX_SIZE));
        meta.set(9, EntityMetadataValue::Float(HITBOX_SIZE));
    }
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
    label: Entity,
}

/// The entities of a session once the player is in place.
#[derive(Debug, Clone, Copy)]
struct Rig {
    vehicle: Entity,
    camera: Entity,
    hitbox: Entity,
}

/// A player in the map, and how to put them back.
#[derive(Component)]
pub struct MapSession {
    origin: [f64; 2],
    position: Position,
    rotation: Rotation,
    abilities: Option<PlayerAbilities>,
    tab_game_mode: Option<u8>,
    pins: Vec<Pin>,
    you: Option<Entity>,
    cursor: Entity,
    bar: Entity,
    rig: Option<Rig>,
    mouse: Mouse,
    pointer: Pointer,
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

    fn camera_position(&self) -> Position {
        let [x, z] = self.pointer.view;
        self.on_model(x, CAMERA_Y, z + CAMERA_BACK)
    }

    fn hitbox_position(&self) -> Position {
        let camera = self.camera_position();
        Position {
            y: camera.y - f64::from(HITBOX_SIZE) / 2.0,
            ..camera
        }
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
        .add_metadata_source::<Hitbox>()
        .add_systems(Startup, build_atlas)
        .add_systems(Update, (mount, follow_pointer, show_hint))
        .add_observer(enter)
        .add_observer(look_through_camera)
        .add_observer(read_mouse)
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
    let scene = Scene::new(&terrain, SEA_LEVEL);
    atlas.chunks = scene.chunks(atlas.level.biome, atlas.level.biome_count);
    atlas.waypoints = MARKERS
        .place(generator.0.as_ref())
        .into_iter()
        .filter_map(|marker| {
            let (x, z) = Scene::cell_of(&window, marker.x, marker.z)?;
            let top = scene.top(x, z)?;
            Some(Waypoint {
                marker,
                at: [f64::from(x) + 0.5, f64::from(top + 1), f64::from(z) + 0.5],
            })
        })
        .collect();
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

/// Asks for a player to enter the map.
#[derive(Event, Debug, Clone, Copy)]
pub struct OpenMap(pub Entity);

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

fn enter(
    event: On<OpenMap>,
    mut states: Query<
        (
            &ClientId,
            &Position,
            &Rotation,
            Option<&PlayerAbilities>,
            Option<&mut TabEntry>,
        ),
        Without<MapSession>,
    >,
    atlas: Res<Atlas>,
    config: Res<ServerConfigResource>,
    players: Players,
    mut commands: Commands,
) {
    let player = event.0;
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
    players.send(
        player,
        SetCenterChunk {
            chunk_x: anchor[0],
            chunk_z: anchor[1],
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
            label: spawn_label(
                &mut commands,
                lifted(waypoint.at, LABEL_LIFT),
                &waypoint.marker.name,
            ),
        })
        .collect();
    let you = model_spot(&atlas, position)
        .map(|at| spawn_label(&mut commands, lifted(at, LABEL_LIFT), "VOUS"));
    let cursor = EntityBuilder::new(EntityKind::ItemDisplay)
        .position(lifted([0.5, f64::from(scene::SEA_Y), 0.5], CURSOR_LIFT))
        .with_bundle((
            display(CURSOR_SCALE).teleport_ticks(CURSOR_GLIDE_TICKS),
            ItemDisplay::new(item("minecraft:nether_star")).context(ItemDisplayContext::Gui),
        ))
        .spawn(&mut commands)
        .id();
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
                origin,
                position: *position,
                rotation: *rotation,
                abilities: abilities.cloned(),
                tab_game_mode,
                pins,
                you,
                cursor,
                bar,
                rig: None,
                mouse: Mouse::default(),
                pointer: Pointer::default(),
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
            Teleport::to(origin[0], CAMERA_Y, origin[1])
                .facing(CAMERA_YAW, 0.0)
                .unthrottled()
                .preload_radius(0),
        ));
}

fn model_spot(atlas: &Atlas, position: &Position) -> Option<[f64; 3]> {
    let scene = atlas.scene.as_ref()?;
    let (x, z) = Scene::cell_of(&scene::window(), position.x as i32, position.z as i32)?;
    let top = scene.top(x, z)?;
    Some([f64::from(x) + 0.5, f64::from(top + 1), f64::from(z) + 0.5])
}

/// Once the player stands in the level. A teleport would throw them off a vehicle.
fn mount(
    mut sessions: Query<(Entity, &mut MapSession), Without<Teleport>>,
    mut commands: Commands,
) {
    for (player, mut session) in &mut sessions {
        if session.rig.is_some() {
            continue;
        }
        let vehicle = EntityBuilder::new(EntityKind::ItemDisplay)
            .position(session.on_model(0.0, CAMERA_Y, 0.0))
            .with(Passengers::new([player]))
            .spawn(&mut commands)
            .id();
        let camera = EntityBuilder::new(EntityKind::ItemDisplay)
            .position(session.camera_position())
            .rotation(CAMERA_YAW, CAMERA_PITCH)
            .with(Display::default().teleport_ticks(CAMERA_GLIDE_TICKS))
            .spawn(&mut commands)
            .id();
        let hitbox = EntityBuilder::new(EntityKind::Interaction)
            .position(session.hitbox_position())
            .with(Hitbox)
            .spawn(&mut commands)
            .id();
        session.rig = Some(Rig {
            vehicle,
            camera,
            hitbox,
        });
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
    if session.rig.is_some_and(|rig| rig.camera == event.entity)
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
    let turn = session.mouse.report(event.packet.yaw, event.packet.pitch);
    if turn.delta != [0.0, 0.0] {
        session.pointer.apply(turn.delta);
    }
    if let Some(pitch) = turn.recentre {
        players.send(
            event.entity,
            PlayerRotation {
                yaw: 0.0,
                relative_yaw: true,
                pitch,
                relative_pitch: true,
            },
        );
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
        None => {
            let scale = f64::from(scene::BLOCKS_PER_CELL);
            let [x, z] = session.pointer.cursor.map(|v| (v * scale).round());
            format!("{TITLE} - X {x}  Z {z}")
        }
    }
}

fn follow_pointer(
    mut sessions: Query<&mut MapSession, Changed<MapSession>>,
    mut positions: Query<&mut Position, Without<MapSession>>,
    atlas: Res<Atlas>,
    mut displays: Query<&mut Display>,
    mut bars: Query<&mut BossBar>,
    mut commands: Commands,
) {
    let Some(scene) = atlas.scene.as_ref() else {
        return;
    };
    for mut session in &mut sessions {
        let Some(rig) = session.rig else {
            continue;
        };
        let [x, z] = session.pointer.cursor;
        let ground = scene
            .top(x.floor() as i32, z.floor() as i32)
            .unwrap_or(scene::SEA_Y);
        let cursor = session.on_model(x, f64::from(ground + 1) + CURSOR_LIFT, z);
        for (entity, at) in [
            (session.cursor, cursor),
            (rig.camera, session.camera_position()),
            (rig.hitbox, session.hitbox_position()),
        ] {
            if let Ok(mut position) = positions.get_mut(entity) {
                position.set_if_neq(at);
            }
        }

        let hovered = session.pointer.nearest(
            atlas.waypoints.iter().map(|w| [w.at[0], w.at[2]]),
            HOVER_RADIUS,
        );
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
            if let Some(index) = hovered {
                commands.entity(session.pins[index].icon).insert(Glowing);
                tracing::info!(marker = %atlas.waypoints[index].marker.name, "map hover");
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

fn select(
    session: &mut MapSession,
    choice: Option<usize>,
    atlas: &Atlas,
    displays: &mut Query<&mut Display>,
    labels: &mut Query<&mut TextDisplay>,
) {
    let previous = std::mem::replace(&mut session.selected, choice);
    if let Some(old) = previous.filter(|old| Some(*old) != choice) {
        let scale = session.pin_scale(old);
        set_scale(displays, session.pins[old].icon, scale);
        if let Ok(mut label) = labels.get_mut(session.pins[old].label) {
            label.text = atlas.waypoints[old].marker.name.clone();
        }
    }
    if let Some(index) = choice
        && let Ok(mut label) = labels.get_mut(session.pins[index].label)
    {
        label.text = format!("> {} <", atlas.waypoints[index].marker.name);
    }
}

/// Whether this packet is a new click on the session's hitbox.
fn counts_as_click(session: &mut MapSession, id: i32, ids: &Query<&MinecraftEntityId>) -> bool {
    let is_hitbox = session
        .rig
        .and_then(|rig| ids.get(rig.hitbox).ok())
        .is_some_and(|hitbox| hitbox.0 == id);
    if !is_hitbox || session.ticks.saturating_sub(session.last_click) < CLICK_COOLDOWN {
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
    mut labels: Query<&mut TextDisplay>,
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
    select(
        &mut session,
        Some(index),
        &atlas,
        &mut displays,
        &mut labels,
    );
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
    atlas: Res<Atlas>,
    mut displays: Query<&mut Display>,
    mut labels: Query<&mut TextDisplay>,
) {
    let player = event.entity;
    let Ok(mut session) = sessions.get_mut(player) else {
        return;
    };
    if counts_as_click(&mut session, event.packet.entity_id, &ids) {
        tracing::info!(?player, "map right click");
        select(&mut session, None, &atlas, &mut displays, &mut labels);
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
    let pins = session.pins.iter().flat_map(|pin| [pin.icon, pin.label]);
    let rig = session
        .rig
        .iter()
        .flat_map(|rig| [rig.vehicle, rig.camera, rig.hitbox]);
    for entity in pins
        .chain(rig)
        .chain(session.you)
        .chain([session.cursor, session.bar])
    {
        commands.entity(entity).try_despawn();
    }
}

fn crouch(
    event: On<PlayerInputEvent>,
    mut sessions: Query<(&MapSession, &MinecraftEntityId, Option<&mut TabEntry>)>,
    config: Res<ServerConfigResource>,
    players: Players,
    mut commands: Commands,
) {
    let player = event.entity;
    if !event.sneak {
        return;
    }
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
        Some(abilities) => back.insert(abilities.clone()),
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
        .description("Ouvre la carte du monde")
        .handler(open_map)
        .build()
}

fn open_map(ctx: &mut CommandContext) {
    let player = ctx.entity;
    ctx.with_world_mut(|world| world.trigger(OpenMap(player)));
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
