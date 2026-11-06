//! How a scene plays out, tick by tick, for one player.
//!
//! What only that player should see exists on their client alone: the double that stands in
//! for them, as a client draws no local player from another view, and the entity they see
//! through. Being a spectator on their client alone hides their hand and crosshair.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use uuid::Uuid;
use voidmc::{
    CustomName, Display, EntityBuilder, EntityKind, EntityMetadata, Inventory, ItemDisplay,
    ItemDisplayContext, ItemStack, Mannequin, MetadataSource, NoGravity, Particles, Players,
    ServerConfigResource, Silent, Sound, Sounds, TabEntry,
    components::{MinecraftEntityId, PlayerName, PlayerUuid, Position, Rotation},
    events::PlayerReadyEvent,
    network::PacketEvent,
};
use voidmc_codec::Encode;
use voidmc_protocol::clientbound::entity_metadata::{entity_flag, entity_index};
use voidmc_protocol::clientbound::{
    EntityMetadataValue, EntityPositionSync, GameEvent, GameEventType, RemoveEntities,
    ResolvableProfile, SetCamera, SetEntityData, SetHeadRotation, SpawnEntity,
};
use voidmc_protocol::serverbound::Interact;
use voidmc_protocol::slot::{DataComponent, component_ids};
use voidmc_protocol::types::LpVec3;
use voidmc_ui::{Dialog, UiClick};

use crate::conversation::{Choice, Node, Then};
use crate::interface::{Cx, Memory, Reply, State};
use crate::npc::{Cast, Npc};
use crate::props::Order;
use crate::rig::{self, Figure, Gesture, Limbs, Motion};
use crate::stage::{GLIDE_TICKS, Pose, Shot, Stage, ease};
use crate::view::{self, Click, Speech, VEIL_FULL_TICKS};

const SPECTATOR: f32 = 3.0;
/// What a prop is under its model.
const PROP_ITEM: &str = "minecraft:paper";
const FULL_LIGHT: u8 = 15;
const FIGURE_SHADOW: f32 = 0.5;
/// A keyframe of a figure every this many ticks.
const FIGURE_STEP: u32 = 2;
const BURST_SPREAD: f32 = 0.15;
const BURST_SPEED: f32 = 0.3;
/// The packet has only meant a right click since attacks got one of their own, and this
/// field now says which hand: `Interact` still names it after what it was.
const MAIN_HAND: i32 = 0;
/// The action bar fades after three seconds, so it is sent again before that.
const KEEP_SHOWN_TICKS: u32 = 40;
const CAMERA_GLIDE: u8 = 3;
const TALK_REACH: f64 = 3.3;
const AIM_RADIUS: f64 = 0.75;
const LOOK_REACH: f64 = 7.0;
const TURN_STEP: f32 = 14.0;
const EYES: f64 = 1.62;
const CHEST: f64 = 1.0;
/// Ids the server never gives an entity of its own.
static NEXT_LOCAL_ID: AtomicI32 = AtomicI32::new(1 << 30);

/// A player did something an NPC's conversation or interface emits.
#[derive(Event, Debug, Clone, PartialEq, Eq)]
pub struct NpcEvent {
    pub player: Entity,
    pub npc: String,
    pub name: String,
}

/// The cast, from [`crate::Npcs::build`].
#[derive(Resource, Clone)]
struct Scenes(Arc<Cast>);

/// The body of an NPC, by its place in the cast.
#[derive(Component, Debug, Clone, Copy)]
struct Body(usize);

/// What goes on in a scene.
enum Phase {
    /// The veil is coming down over the world.
    Entering,
    Speaking {
        node: String,
        line: usize,
        lines: Vec<String>,
        typed: usize,
    },
    Choosing {
        node: String,
        lines: Vec<String>,
    },
    Interface {
        name: String,
        from: String,
        state: State,
    },
    /// The veil is coming down, for the world to come back under it.
    Leaving,
}

/// The view, going from one pose to the next.
#[derive(Debug, Clone, Copy)]
struct Glide {
    from: Pose,
    to: Pose,
    tick: u32,
}

impl Glide {
    fn still(pose: Pose) -> Self {
        Self {
            from: pose,
            to: pose,
            tick: GLIDE_TICKS,
        }
    }

    fn towards(&mut self, to: Pose) {
        let now = self.now();
        *self = Self {
            from: now,
            to,
            tick: 0,
        };
    }

    fn now(&self) -> Pose {
        self.from
            .blend(self.to, ease(f64::from(self.tick) / f64::from(GLIDE_TICKS)))
    }

    /// Where to be this tick, if the view is on its way.
    fn step(&mut self) -> Option<Pose> {
        (self.tick < GLIDE_TICKS).then(|| {
            self.tick += 1;
            self.now()
        })
    }
}

/// A player in a scene.
#[derive(Component)]
pub struct Talking {
    npc: usize,
    /// The network id of the NPC's body.
    body: i32,
    stage: Stage,
    phase: Phase,
    /// Ticks since the phase began.
    ticks: u32,
    camera: i32,
    double: i32,
    glide: Glide,
    /// What is in the world, by slot.
    props: BTreeMap<u32, i32>,
    figure: Option<Puppet>,
}

/// The figure of an NPC in a scene.
struct Puppet {
    figure: Figure,
    parts: [i32; 4],
    motion: Motion,
}

impl Talking {
    /// Who they talk to, by the id it was given.
    pub fn npc<'a>(&self, cast: &'a [String]) -> Option<&'a str> {
        cast.get(self.npc).map(String::as_str)
    }
}

/// Goes in the app, with `voidmc_ui::UiPlugin` serving the pack it was built into.
pub struct NpcPlugin {
    cast: Arc<Cast>,
}

impl NpcPlugin {
    pub(crate) fn new(cast: Arc<Cast>) -> Self {
        Self { cast }
    }
}

impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Scenes(self.cast.clone()))
            .add_observer(remember)
            .add_observer(right_click)
            .add_observer(click)
            .add_systems(Update, (spawn_bodies, watch, play_scenes));
    }
}

fn remember(event: On<PlayerReadyEvent>, mut commands: Commands) {
    commands
        .entity(event.entity)
        .insert((Memory::default(), Prompted(false)));
}

/// Whether the action bar is telling a player to right click.
#[derive(Component)]
struct Prompted(bool);

fn spawn_bodies(scenes: Res<Scenes>, mut done: Local<bool>, mut commands: Commands) {
    if std::mem::replace(&mut *done, true) {
        return;
    }
    for (index, npc) in scenes.0.npcs.iter().enumerate() {
        EntityBuilder::new(EntityKind::Mannequin)
            .position(npc.at)
            .rotation(npc.yaw, 0.0)
            .gravity(false)
            .block_collision(false)
            .settle_ticks(0)
            .with_bundle((
                Mannequin::new(npc.profile_shown()),
                CustomName::new(npc.name.clone()),
                NoGravity,
                Silent,
                Body(index),
            ))
            .spawn(&mut commands);
    }
}

/// How far `from`, looking along `yaw` and `pitch`, passes from `target`, if it looks towards it.
fn aim_miss(from: [f64; 3], yaw: f32, pitch: f32, target: [f64; 3]) -> Option<f64> {
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    let look = [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ];
    let to = [
        target[0] - from[0],
        target[1] - from[1],
        target[2] - from[2],
    ];
    let along: f64 = (0..3).map(|axis| to[axis] * look[axis]).sum();
    if along <= 0.0 {
        return None;
    }
    let miss = (0..3).map(|axis| (to[axis] - look[axis] * along).powi(2));
    Some(miss.sum::<f64>().sqrt())
}

fn apart(a: &Position, b: &Position) -> f64 {
    (a.x - b.x).hypot(a.z - b.z).hypot(a.y - b.y)
}

/// Turns `from` by at most `step` towards `to`.
fn turned(from: f32, to: f32, step: f32) -> f32 {
    let turn = (to - from + 180.0).rem_euclid(360.0) - 180.0;
    from + turn.clamp(-step, step)
}

fn yaw_byte(yaw: f32) -> u8 {
    (yaw.rem_euclid(360.0) / 360.0 * 256.0) as u8
}

type Walkers<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Position,
        &'static Rotation,
        &'static mut Prompted,
    ),
    (Without<Body>, Without<Talking>),
>;

/// Heads follow who comes close, and the action bar tells who looks at an NPC what to do.
fn watch(
    mut bodies: Query<(&Body, &MinecraftEntityId, &Position, &mut Rotation)>,
    mut walkers: Walkers,
    talkers: Query<(&Talking, &Position), Without<Body>>,
    players: Players,
) {
    for (body, id, at, mut rotation) in &mut bodies {
        let partner = talkers.iter().find(|(talking, _)| talking.npc == body.0);
        let nearest = walkers
            .iter()
            .map(|(_, position, _, _)| *position)
            .filter(|position| apart(position, at) < LOOK_REACH)
            .min_by(|a, b| apart(a, at).total_cmp(&apart(b, at)));
        let wanted = match partner {
            Some((talking, _)) => Some(talking.stage.facing().1),
            None => nearest.map(|position| {
                Pose::towards([at.x, at.y, at.z], [position.x, position.y, position.z]).yaw
            }),
        };
        let Some(wanted) = wanted else {
            continue;
        };
        let yaw = turned(rotation.yaw, wanted, TURN_STEP);
        if (yaw - rotation.yaw).abs() > 0.5 {
            rotation.yaw = yaw;
            let packet = SetHeadRotation {
                entity_id: id.0,
                head_yaw: yaw_byte(yaw),
            };
            players.ready().send(packet);
        }
    }
    for (player, position, rotation, mut prompted) in &mut walkers {
        let eye = [position.x, position.y + EYES, position.z];
        let aimed = bodies.iter().any(|(_, _, at, _)| {
            apart(at, position) < TALK_REACH
                && aim_miss(
                    eye,
                    rotation.yaw,
                    rotation.pitch,
                    [at.x, at.y + CHEST, at.z],
                )
                .is_some_and(|miss| miss < AIM_RADIUS)
        });
        if aimed != prompted.0 {
            prompted.0 = aimed;
            players.send(
                player,
                if aimed {
                    view::prompt()
                } else {
                    view::no_prompt()
                },
            );
        }
    }
}

/// An entity of one client only, which the world knows nothing about.
fn spawn_local(
    players: &Players,
    player: Entity,
    kind: EntityKind,
    at: [f64; 3],
    yaw: f32,
    pitch: f32,
    source: &impl MetadataSource,
) -> i32 {
    let id = NEXT_LOCAL_ID.fetch_add(1, Ordering::Relaxed);
    let [x, y, z] = at;
    players.send(
        player,
        SpawnEntity {
            entity_id: id,
            entity_uuid: Uuid::from_u128(0x766f_6964_6e70_6300_0000_0000_0000_0000 | id as u128),
            entity_type: kind.id(),
            x,
            y,
            z,
            velocity: LpVec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            pitch: yaw_byte(pitch),
            yaw: yaw_byte(yaw),
            head_yaw: yaw_byte(yaw),
            data: 0,
        },
    );
    let mut metadata = EntityMetadata::default();
    source.write(&mut metadata);
    players.send(
        player,
        SetEntityData {
            entity_id: id,
            entries: metadata.entries(),
        },
    );
    id
}

fn move_camera(players: &Players, player: Entity, camera: i32, pose: Pose) {
    players.send(
        player,
        EntityPositionSync {
            entity_id: camera,
            x: pose.x,
            y: pose.y,
            z: pose.z,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            yaw: pose.yaw,
            pitch: pose.pitch,
            on_ground: false,
        },
    );
}

fn right_click(
    event: On<PacketEvent<Interact>>,
    bodies: Query<(&Body, &MinecraftEntityId, &Position)>,
    players_at: Query<&Position, Without<Body>>,
    talking: Query<(), With<Talking>>,
    scenes: Res<Scenes>,
    players: Players,
    mut commands: Commands,
) {
    if event.packet.interact_type != MAIN_HAND || talking.contains(event.entity) {
        return;
    }
    let target = event.packet.entity_id;
    let Some((body, id, at)) = bodies.iter().find(|(_, id, _)| id.0 == target) else {
        return;
    };
    let Ok(player_at) = players_at.get(event.entity) else {
        return;
    };
    let stage = Stage::new(*at, *player_at);
    for packet in view::veil_in(&scenes.0.art) {
        players.send(event.entity, packet);
    }
    players.send(event.entity, view::no_prompt());
    commands.entity(event.entity).insert(Talking {
        npc: body.0,
        body: id.0,
        stage,
        phase: Phase::Entering,
        ticks: 0,
        camera: 0,
        double: 0,
        glide: Glide::still(stage.pose(Shot::Shoulder)),
        props: BTreeMap::new(),
        figure: None,
    });
}

/// What can be done to a scene, from a tick or a click.
struct Scene<'a, 'w, 's> {
    player: Entity,
    talking: &'a mut Talking,
    npc: &'a Npc,
    cast: &'a Cast,
    memory: &'a mut Memory,
    inventory: &'a mut Inventory,
    players: &'a Players<'w, 's>,
    sounds: &'a Sounds<'w, 's>,
    particles: &'a Particles<'w, 's>,
    commands: &'a mut Commands<'w, 's>,
    me: Me<'a>,
}

/// Who the player is, for what stands in for them and what comes back after a scene.
struct Me<'a> {
    id: i32,
    uuid: Uuid,
    name: &'a str,
    game_mode: f32,
}

impl Me<'_> {
    /// Only an id: the client finds the skin that goes with it.
    fn profile(&self) -> ResolvableProfile {
        ResolvableProfile {
            id: Some(self.uuid),
            name: Some(self.name.to_string()),
            ..ResolvableProfile::default()
        }
    }
}

impl Scene<'_, '_, '_> {
    fn choices<'n>(&self, node: &'n Node) -> Vec<(usize, &'n Choice)> {
        let shown = |choice: &&Choice| {
            choice.needs.iter().all(|flag| self.memory.is_set(flag))
                && !choice.unless.iter().any(|flag| self.memory.is_set(flag))
        };
        node.choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| shown(choice))
            .collect()
    }

    fn set_phase(&mut self, phase: Phase) {
        self.talking.phase = phase;
        self.talking.ticks = 0;
    }

    fn frame(&mut self, shot: Shot) {
        let pose = self.talking.stage.pose(shot);
        self.talking.glide.towards(pose);
    }

    fn start(&mut self) {
        let stage = self.talking.stage;
        let pose = stage.pose(self.npc.shot);
        self.talking.glide = Glide::still(pose);
        let camera = Display::default().teleport_ticks(CAMERA_GLIDE);
        self.talking.camera = spawn_local(
            self.players,
            self.player,
            EntityKind::ItemDisplay,
            [pose.x, pose.y, pose.z],
            pose.yaw,
            pose.pitch,
            &camera,
        );
        move_camera(self.players, self.player, self.talking.camera, pose);
        let (double_yaw, _) = stage.facing();
        let double = Mannequin::new(self.me.profile());
        self.talking.double = spawn_local(
            self.players,
            self.player,
            EntityKind::Mannequin,
            [stage.double.x, stage.double.y, stage.double.z],
            double_yaw,
            0.0,
            &double,
        );
        self.players.send(
            self.player,
            GameEvent {
                event: GameEventType::ChangeGameMode,
                value: SPECTATOR,
            },
        );
        self.players.send(
            self.player,
            SetCamera {
                camera_id: self.talking.camera,
            },
        );
        self.show_name(false);
        self.put_figure();
        self.players.send(self.player, view::veil_out());
        let start = self
            .npc
            .talk
            .start(|flag| self.memory.is_set(flag))
            .to_string();
        self.go_to(&start);
    }

    /// A name over the NPC would hang in the middle of the shot. Others still see it. A hidden
    /// name is not enough: the client shows it anyway to a view aimed at the NPC.
    fn show_name(&self, shown: bool) {
        let name = if shown {
            CustomName::new(self.npc.name.clone())
        } else {
            CustomName::new("").hidden()
        };
        let mut metadata = EntityMetadata::default();
        name.write(&mut metadata);
        self.players.send(
            self.player,
            SetEntityData {
                entity_id: self.talking.body,
                entries: metadata.entries(),
            },
        );
    }

    /// Under the veil, so nobody sees one body take the place of the other.
    fn put_figure(&mut self) {
        let Some(figure) = self.npc.figure() else {
            return;
        };
        let rest = rig::placings(Limbs::default(), figure.slim);
        let stage = self.talking.stage;
        let parts = std::array::from_fn(|index| {
            let display = Display::default().transform(stage.placed(rest[index]));
            let display = if index == 0 {
                display.shadow(FIGURE_SHADOW, 1.0)
            } else {
                display
            };
            self.spawn_prop(&figure.parts[index], display)
        });
        self.body_flags(entity_flag::INVISIBLE);
        self.talking.figure = Some(Puppet {
            figure,
            parts,
            motion: Motion::default(),
        });
    }

    fn take_figure(&mut self) {
        if let Some(puppet) = self.talking.figure.take() {
            self.players.send(
                self.player,
                RemoveEntities {
                    entity_ids: puppet.parts.to_vec(),
                },
            );
            self.body_flags(0);
        }
    }

    fn body_flags(&self, flags: u8) {
        let mut metadata = EntityMetadata::default();
        metadata.set(entity_index::FLAGS, EntityMetadataValue::Byte(flags as i8));
        self.players.send(
            self.player,
            SetEntityData {
                entity_id: self.talking.body,
                entries: metadata.entries(),
            },
        );
    }

    fn move_figure(&mut self) {
        let speaking = matches!(&self.talking.phase, Phase::Speaking { lines, typed, .. }
            if *typed < lines.iter().map(|line| line.chars().count()).sum());
        let stage = self.talking.stage;
        let Some(puppet) = &mut self.talking.figure else {
            return;
        };
        let limbs = puppet.motion.advance(FIGURE_STEP, speaking);
        let placings = rig::placings(limbs, puppet.figure.slim);
        for (id, placing) in puppet.parts.iter().zip(placings).skip(1) {
            self.players.send(
                self.player,
                SetEntityData {
                    entity_id: *id,
                    entries: stage
                        .placed(placing)
                        .entries(u16::try_from(FIGURE_STEP).unwrap_or(1)),
                },
            );
        }
    }

    fn gesture(&mut self, gesture: Gesture) {
        if let Some(puppet) = &mut self.talking.figure {
            puppet.motion.play(gesture);
        }
    }

    fn go_to(&mut self, id: &str) {
        let Some(node) = self.npc.talk.get(id) else {
            return self.leave();
        };
        for flag in &node.sets {
            self.memory.set(flag.clone());
        }
        self.say(id.to_string(), 0);
    }

    fn say(&mut self, node: String, line: usize) {
        let said = self
            .npc
            .talk
            .get(&node)
            .and_then(|node| node.lines.get(line));
        let Some(said) = said else {
            return self.leave();
        };
        if let Some(shot) = said.shot {
            self.frame(shot);
        }
        let gesture = said.gesture;
        let lines = said.wrapped();
        self.set_phase(Phase::Speaking {
            node,
            line,
            lines,
            typed: 0,
        });
        if let Some(gesture) = gesture {
            self.gesture(gesture);
        }
        self.players
            .send(self.player, view::listening(&self.cast.art).packet());
    }

    fn speech(&self, lines: &[String], typed: usize, hint: Option<u32>, aside: bool) {
        let speech = Speech {
            name: &self.npc.name,
            lines,
            typed,
            hint,
            aside,
        };
        self.players.send(
            self.player,
            view::speech(&self.cast.art, &self.npc.theme, &speech),
        );
    }

    fn choose(&mut self, node: String, lines: Vec<String>) {
        let Some(asked) = self.npc.talk.get(&node) else {
            return self.leave();
        };
        let choices = self.choices(asked);
        let dialog = view::choosing(&self.cast.art, &self.npc.theme, &choices);
        self.players.send(self.player, dialog.packet());
        self.speech(&lines, usize::MAX, None, true);
        self.frame(self.npc.shot);
        self.set_phase(Phase::Choosing { node, lines });
    }

    /// Goes on from the end of a line.
    fn after_line(&mut self, node: String, line: usize, lines: Vec<String>) {
        let Some(asked) = self.npc.talk.get(&node) else {
            return self.leave();
        };
        if line + 1 < asked.lines.len() {
            return self.say(node, line + 1);
        }
        if !self.choices(asked).is_empty() {
            return self.choose(node, lines);
        }
        match asked.next.clone() {
            Some(next) => self.go_to(&next),
            None => self.leave(),
        }
    }

    fn pick(&mut self, node: &str, index: usize) {
        let choice = self
            .npc
            .talk
            .get(node)
            .and_then(|node| node.choices.get(index));
        let Some(choice) = choice else {
            return;
        };
        for flag in &choice.sets {
            self.memory.set(flag.clone());
        }
        if let Some(name) = &choice.emits {
            self.emit(name.clone());
        }
        match &choice.then {
            Then::Goto(to) => self.go_to(to),
            Then::Open(name) => self.open(name, node.to_string()),
            Then::Leave => self.leave(),
        }
    }

    fn emit(&mut self, name: String) {
        self.commands.trigger(NpcEvent {
            player: self.player,
            npc: self.npc.id.clone(),
            name,
        });
    }

    fn open(&mut self, name: &str, from: String) {
        let Some(interface) = self.npc.interfaces.get(name) else {
            return;
        };
        self.frame(interface.shot());
        self.set_phase(Phase::Interface {
            name: name.to_string(),
            from,
            state: interface.state(),
        });
        self.players
            .send(self.player, view::silence(&self.cast.art));
        self.show();
    }

    /// Runs `act` on the open interface, then does what came of it.
    fn with_interface<R>(
        &mut self,
        act: impl FnOnce(&dyn crate::interface::AnyInterface, &mut State, &mut Cx) -> R,
    ) -> Option<R> {
        let Phase::Interface { name, state, .. } = &mut self.talking.phase else {
            return None;
        };
        let interface = self.npc.interfaces.get(name)?;
        let mut cx = Cx {
            inventory: self.inventory,
            memory: self.memory,
            theme: &self.npc.theme,
            name: &self.npc.name,
            sounds: Vec::new(),
            events: Vec::new(),
            props: Vec::new(),
        };
        let result = act(interface.as_ref(), state, &mut cx);
        let (sounds, events, props) = (cx.sounds, cx.events, cx.props);
        for order in props {
            self.apply(order);
        }
        for sound in sounds {
            self.sounds.play_to(self.player, sound);
        }
        for event in events {
            self.emit(event);
        }
        Some(result)
    }

    fn apply(&mut self, order: Order) {
        match order {
            Order::Show {
                slot,
                model,
                placing,
                ticks,
            } => {
                let transform = self.talking.stage.placed(placing);
                if let Some(id) = self.talking.props.get(&slot) {
                    self.players.send(
                        self.player,
                        SetEntityData {
                            entity_id: *id,
                            entries: transform.entries(ticks),
                        },
                    );
                } else {
                    let display = Display::default()
                        .transform(transform)
                        .brightness(FULL_LIGHT, FULL_LIGHT);
                    let id = self.spawn_prop(&model, display);
                    self.talking.props.insert(slot, id);
                }
            }
            Order::Hide(slot) => {
                if let Some(id) = self.talking.props.remove(&slot) {
                    self.players.send(
                        self.player,
                        RemoveEntities {
                            entity_ids: vec![id],
                        },
                    );
                }
            }
            Order::Gesture(gesture) => self.gesture(gesture),
            Order::Burst {
                particle,
                spot,
                count,
            } => {
                self.particles
                    .spawn(particle)
                    .at(self.talking.stage.spot(spot))
                    .count(count)
                    .offset(BURST_SPREAD, BURST_SPREAD, BURST_SPREAD)
                    .speed(BURST_SPEED)
                    .viewers([self.player])
                    .send();
            }
        }
    }

    fn spawn_prop(&self, model: &str, display: Display) -> i32 {
        let npc = self.talking.stage.npc;
        let id = spawn_local(
            self.players,
            self.player,
            EntityKind::ItemDisplay,
            [npc.x, npc.y, npc.z],
            0.0,
            0.0,
            &display,
        );
        let mut body = Vec::new();
        model.to_string().encode(&mut body);
        let item = ItemStack::of(PROP_ITEM, 1)
            .expect("a vanilla item")
            .with_component(DataComponent::Raw {
                type_id: component_ids::ITEM_MODEL,
                body,
            });
        let mut metadata = EntityMetadata::default();
        ItemDisplay::new(item)
            .context(ItemDisplayContext::None)
            .write(&mut metadata);
        self.players.send(
            self.player,
            SetEntityData {
                entity_id: id,
                entries: metadata.entries(),
            },
        );
        id
    }

    fn clear_props(&mut self) {
        let entity_ids: Vec<i32> = std::mem::take(&mut self.talking.props)
            .into_values()
            .collect();
        if !entity_ids.is_empty() {
            self.players
                .send(self.player, RemoveEntities { entity_ids });
        }
    }

    fn show(&mut self) {
        let art = &self.cast.art;
        let screen = self.with_interface(|interface, state, cx| interface.view(state, cx));
        if let Some(screen) = screen {
            self.players
                .send(self.player, view::interface(art, screen).packet());
        }
    }

    fn act(&mut self, action: &str) {
        let reply = self.with_interface(|interface, state, cx| interface.click(state, action, cx));
        match reply {
            Some(Reply::Stay) => self.show(),
            Some(Reply::Back) => {
                self.clear_props();
                if let Phase::Interface { from, .. } = &self.talking.phase {
                    let from = from.clone();
                    let lines = self
                        .npc
                        .talk
                        .get(&from)
                        .and_then(|node| node.lines.last())
                        .map(|line| line.wrapped())
                        .unwrap_or_default();
                    self.choose(from, lines);
                }
            }
            Some(Reply::Goto(node)) => {
                self.clear_props();
                self.frame(self.npc.shot);
                self.go_to(&node);
            }
            Some(Reply::Leave) => self.leave(),
            None => {}
        }
    }

    fn leave(&mut self) {
        if matches!(self.talking.phase, Phase::Leaving) {
            return;
        }
        self.clear_props();
        self.players.send(self.player, Dialog::close());
        for packet in view::veil_in(&self.cast.art) {
            self.players.send(self.player, packet);
        }
        self.set_phase(Phase::Leaving);
    }

    fn end(&mut self) {
        let player = self.player;
        self.players.send(
            player,
            SetCamera {
                camera_id: self.me.id,
            },
        );
        self.players.send(
            player,
            GameEvent {
                event: GameEventType::ChangeGameMode,
                value: self.me.game_mode,
            },
        );
        self.players.send(
            player,
            RemoveEntities {
                entity_ids: vec![self.talking.camera, self.talking.double],
            },
        );
        self.take_figure();
        self.show_name(true);
        self.players.send(player, view::no_prompt());
        self.players.send(player, view::veil_out());
        self.commands.entity(player).remove::<Talking>();
    }

    fn voice(&self, typed: usize) {
        let voice = &self.npc.voice;
        if typed.is_multiple_of(voice.every.max(1)) {
            let sound = Sound::new(voice.sound.clone())
                .pitch(voice.pitch)
                .volume(voice.volume);
            self.sounds.play_to(self.player, sound);
        }
    }

    fn tick(&mut self) {
        self.talking.ticks += 1;
        if let Some(pose) = self.talking.glide.step() {
            move_camera(self.players, self.player, self.talking.camera, pose);
        }
        let ticks = self.talking.ticks;
        if ticks.is_multiple_of(FIGURE_STEP) {
            self.move_figure();
        }
        match &mut self.talking.phase {
            Phase::Entering => {
                if ticks >= VEIL_FULL_TICKS {
                    self.start();
                }
            }
            Phase::Speaking {
                node,
                line,
                lines,
                typed,
            } => {
                let total: usize = lines.iter().map(|line| line.chars().count()).sum();
                if *typed < total {
                    *typed += 1;
                    let (lines, typed) = (lines.clone(), *typed);
                    self.speech(&lines, typed, None, false);
                    self.voice(typed);
                    return;
                }
                let (node, line, lines) = (node.clone(), *line, lines.clone());
                let asked = self.npc.talk.get(&node);
                let last = asked.is_some_and(|asked| line + 1 == asked.lines.len());
                let asks = asked.is_some_and(|asked| !self.choices(asked).is_empty());
                if last && asks {
                    self.choose(node, lines);
                } else if ticks.is_multiple_of(2) {
                    self.speech(&lines, usize::MAX, Some(ticks), false);
                }
            }
            Phase::Choosing { lines, .. } => {
                if ticks.is_multiple_of(KEEP_SHOWN_TICKS) {
                    let lines = lines.clone();
                    self.speech(&lines, usize::MAX, None, true);
                }
            }
            Phase::Interface { .. } => {
                if ticks.is_multiple_of(KEEP_SHOWN_TICKS) {
                    self.players
                        .send(self.player, view::silence(&self.cast.art));
                }
                let changed = self.with_interface(|interface, state, cx| interface.tick(state, cx));
                if changed == Some(true) {
                    self.show();
                }
            }
            Phase::Leaving => {
                if ticks >= VEIL_FULL_TICKS {
                    self.end();
                }
            }
        }
    }

    fn clicked(&mut self, click: Click) {
        match (click, &mut self.talking.phase) {
            (
                Click::Leave,
                Phase::Speaking { .. } | Phase::Choosing { .. } | Phase::Interface { .. },
            ) => {
                self.leave();
            }
            (
                Click::Next,
                Phase::Speaking {
                    node,
                    line,
                    lines,
                    typed,
                },
            ) => {
                let total: usize = lines.iter().map(|line| line.chars().count()).sum();
                if *typed < total {
                    *typed = total;
                    let lines = lines.clone();
                    self.speech(&lines, total, None, false);
                    self.players
                        .send(self.player, view::listening(&self.cast.art).packet());
                } else {
                    let (node, line, lines) = (node.clone(), *line, lines.clone());
                    self.after_line(node, line, lines);
                }
            }
            (Click::Choice(index), Phase::Choosing { node, .. }) => {
                let node = node.clone();
                self.pick(&node, index);
            }
            (Click::Interface(action), Phase::Interface { .. }) => self.act(action),
            _ => {}
        }
    }
}

type Who<'a> = (
    &'a mut Talking,
    &'a mut Memory,
    &'a mut Inventory,
    &'a MinecraftEntityId,
    &'a PlayerUuid,
    &'a PlayerName,
    Option<&'a TabEntry>,
);

type WhoItem<'a> = (
    Mut<'a, Talking>,
    Mut<'a, Memory>,
    Mut<'a, Inventory>,
    &'a MinecraftEntityId,
    &'a PlayerUuid,
    &'a PlayerName,
    Option<&'a TabEntry>,
);

/// All a scene acts through.
#[derive(SystemParam)]
struct Crew<'w, 's> {
    scenes: Res<'w, Scenes>,
    config: Res<'w, ServerConfigResource>,
    players: Players<'w, 's>,
    sounds: Sounds<'w, 's>,
    particles: Particles<'w, 's>,
    commands: Commands<'w, 's>,
}

impl Crew<'_, '_> {
    fn on(&mut self, player: Entity, who: WhoItem, act: impl FnOnce(&mut Scene)) {
        let (mut talking, mut memory, mut inventory, id, uuid, name, tab) = who;
        let cast = self.scenes.0.clone();
        let npc = &cast.npcs[talking.npc];
        let game_mode = tab
            .and_then(|entry| entry.game_mode)
            .unwrap_or(self.config.game_mode);
        let mut scene = Scene {
            player,
            npc,
            cast: &cast,
            talking: &mut talking,
            memory: &mut memory,
            inventory: &mut inventory,
            players: &self.players,
            sounds: &self.sounds,
            particles: &self.particles,
            commands: &mut self.commands,
            me: Me {
                id: id.0,
                uuid: uuid.0,
                name: &name.0,
                game_mode: f32::from(game_mode),
            },
        };
        act(&mut scene);
    }
}

fn play_scenes(mut talkers: Query<(Entity, Who)>, mut crew: Crew) {
    for (player, who) in &mut talkers {
        crew.on(player, who, |scene| scene.tick());
    }
}

fn click(event: On<UiClick>, mut talkers: Query<Who>, mut crew: Crew) {
    tracing::debug!(id = %event.id, "click");
    let Some(click) = Click::parse(&event.id) else {
        return;
    };
    if let Ok(who) = talkers.get_mut(event.player) {
        crew.on(event.player, who, |scene| scene.clicked(click));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_straight_at_a_target_misses_by_nothing() {
        let miss = aim_miss([0.0, 0.0, 0.0], 0.0, 0.0, [0.0, 0.0, 3.0]).unwrap();
        assert!(miss < 1e-9);
    }

    #[test]
    fn a_target_behind_is_not_aimed_at() {
        assert_eq!(aim_miss([0.0; 3], 0.0, 0.0, [0.0, 0.0, -3.0]), None);
    }

    #[test]
    fn a_look_to_the_side_misses_by_the_gap() {
        let miss = aim_miss([0.0; 3], 0.0, 0.0, [1.0, 0.0, 3.0]).unwrap();
        assert!((miss - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_head_turns_the_short_way_and_no_faster_than_its_step() {
        assert_eq!(turned(170.0, -170.0, 5.0), 175.0);
        assert_eq!(turned(0.0, 3.0, 5.0), 3.0);
    }

    #[test]
    fn a_glide_ends_where_it_was_told() {
        let from = Pose::towards([0.0; 3], [0.0, 0.0, 1.0]);
        let to = Pose { x: 4.0, ..from };
        let mut glide = Glide::still(from);
        assert_eq!(glide.step(), None);
        glide.towards(to);
        let mut last = from;
        while let Some(pose) = glide.step() {
            last = pose;
        }
        assert_eq!(last, to);
    }

    #[test]
    fn a_new_glide_starts_from_where_the_view_is() {
        let from = Pose::towards([0.0; 3], [0.0, 0.0, 1.0]);
        let mut glide = Glide::still(from);
        glide.towards(Pose { x: 4.0, ..from });
        let now = (0..5).filter_map(|_| glide.step()).last().unwrap();
        glide.towards(from);
        assert_eq!(glide.from, now);
    }
}
