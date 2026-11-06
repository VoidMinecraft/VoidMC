//! Plays a [`Scene`] for whoever has one on the flight of their camera: the real thing or a
//! preview, it is the same film. It starts when such a flight does, and takes its clock from
//! it, so it ends with the camera, which also takes care of a player leaving midway.

use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{
    Display, EntityBuilder, EntityKind, EntityMetadata, ItemDisplay, ItemDisplayContext, ItemStack,
    Mannequin, MetadataSource, MetadataSourceAppExt, NoGravity, Passengers, Players, Sound, Sounds,
    components::{PlayerUuid, Position, Rotation},
    world::{
        ChunkData, ChunkIndex, ChunkPosition, DimensionId, block_state_at_world,
        is_solid_block_state,
    },
};
use voidmc_codec::Encode;
use voidmc_protocol::clientbound::{
    EntityMetadataValue as Value, LevelParticles, ResolvableProfile, entity_metadata,
};
use voidmc_protocol::slot::{DataComponent, component_ids};

use super::{MapSession, PackLoaded, Passage};
use crate::film::{Body, Burst, Pose, Prop, Scene, Stance, View};
use crate::transition::Flight;

/// The view range of what is seen from high up, as a share of what a client calls far.
const FAR: f32 = 3.0;
/// What a client counts as frozen through, and shakes a body for.
const FROZEN_TICKS: i32 = 140;
/// How far up the land is searched over a spot of the camera.
const SEARCHED_HEIGHT: i32 = 12;
/// What a camera keeps clear of the ground.
const CAMERA_MARGIN: f64 = 0.6;
const POSE_STANDING: i32 = 0;
const POSE_FALL_FLYING: i32 = 1;
const POSE_SPIN_ATTACK: i32 = 4;
const POSE_CROUCHING: i32 = 5;
/// `LivingEntity`: the spin of a riptide.
const LIVING_SPINNING: i8 = 0x04;

impl MetadataSource for Stance {
    fn write(&self, meta: &mut EntityMetadata) {
        let (index, flag) = (
            entity_metadata::entity_index::FLAGS,
            entity_metadata::entity_flag::FALL_FLYING,
        );
        let stance = *self;
        let tumbling = stance == Stance::Tumble;
        meta.set(index, Value::Byte(if tumbling { flag as i8 } else { 0 }));
        let pose = match stance {
            Stance::Cower => POSE_CROUCHING,
            Stance::Tumble => POSE_FALL_FLYING,
            Stance::Whirl => POSE_SPIN_ATTACK,
            _ => POSE_STANDING,
        };
        meta.set(entity_metadata::entity_index::POSE, Value::Pose(pose));
        let shaking = matches!(stance, Stance::Shiver | Stance::Cower | Stance::Flail);
        meta.set(
            entity_metadata::entity_index::TICKS_FROZEN,
            Value::Int(if shaking { FROZEN_TICKS } else { 0 }),
        );
        let spinning = stance == Stance::Whirl;
        meta.set(
            entity_metadata::living_entity_index::LIVING_FLAGS,
            Value::Byte(if spinning { LIVING_SPINNING } else { 0 }),
        );
    }

    fn clear(meta: &mut EntityMetadata) {
        Stance::Stand.write(meta);
    }
}

/// What the world is made of, for the camera to keep out of it.
#[derive(SystemParam)]
pub(super) struct Land<'w, 's> {
    index: Res<'w, ChunkIndex>,
    chunks: Query<'w, 's, (&'static ChunkPosition, &'static ChunkData)>,
}

impl Land<'_, '_> {
    fn solid(&self, x: f64, y: i32, z: f64) -> bool {
        let (x, z) = (x.floor() as i32, z.floor() as i32);
        let block =
            block_state_at_world(&self.index, &self.chunks, DimensionId::Overworld, x, y, z);
        block.is_some_and(is_solid_block_state)
    }

    /// How far a camera has to go up from here to be clear of the land.
    fn clearance(&self, [x, y, z]: [f64; 3]) -> f64 {
        let height = y.floor() as i32;
        (0..=SEARCHED_HEIGHT)
            .rev()
            .find(|up| self.solid(x, height + up, z))
            .map_or(0.0, |up| f64::from(up + 1) + CAMERA_MARGIN)
    }

    /// The first side with room for the whole way of the camera, or else the one that has
    /// to go up the least.
    fn view_for(&self, scene: &Scene) -> View {
        let mut best = View::default();
        let mut least = f64::INFINITY;
        for turn in Scene::turns() {
            let lift = scene
                .stops(turn)
                .into_iter()
                .map(|stop| self.clearance(stop))
                .fold(0.0, f64::max);
            if lift < least {
                least = lift;
                best = View { turn, lift };
            }
        }
        best
    }
}

/// A scene under way, with who sees it and what it is made of.
#[derive(Component, Debug)]
pub(super) struct Cast {
    scene: Scene,
    /// Its flight is the clock.
    camera: Entity,
    viewer: Entity,
    profile: ResolvableProfile,
    body: Option<Entity>,
    /// What the body sits on, for as long as it does.
    seat: Option<usize>,
    /// Whether a client has been told that it does.
    seated: bool,
    /// What shows of each prop of the scene.
    props: Vec<Option<Entity>>,
    drawn: bool,
    /// The last tick played.
    played: Option<u32>,
}

pub(super) fn register(app: &mut App) {
    app.add_metadata_source::<Stance>();
}

fn modelled(vanilla: &str, model: &str, drawn: bool) -> ItemStack {
    let item = ItemStack::of(vanilla, 1).expect("a vanilla item");
    if !drawn {
        return item;
    }
    let mut body = Vec::new();
    model.to_string().encode(&mut body);
    item.with_component(DataComponent::Raw {
        type_id: component_ids::ITEM_MODEL,
        body,
    })
}

fn place([x, y, z]: [f64; 3]) -> Position {
    Position { x, y, z }
}

fn spawn_prop(commands: &mut Commands, prop: &Prop, pose: Pose, drawn: bool) -> Entity {
    let item = modelled(prop.vanilla, prop.model, drawn);
    let [width, height] = prop.reach;
    let display = Display::default()
        .transform(pose.shape)
        .interpolation_ticks(prop.morph)
        .brightness(15, 15)
        .view_range(FAR)
        .culling_box(width, height)
        .teleport_ticks(prop.glide);
    let display = match prop.billboard {
        Some(billboard) => display.billboard(billboard),
        None => display,
    };
    let [yaw, pitch] = pose.look;
    EntityBuilder::new(EntityKind::ItemDisplay)
        .position(place(pose.at))
        .rotation(yaw, pitch)
        .with_bundle((
            display,
            ItemDisplay::new(item).context(ItemDisplayContext::None),
        ))
        .spawn(commands)
        .id()
}

fn spawn_body(commands: &mut Commands, body: Body, profile: ResolvableProfile) -> Entity {
    EntityBuilder::new(EntityKind::Mannequin)
        .position(place(body.at))
        .rotation(body.yaw, body.pitch)
        .gravity(false)
        .block_collision(false)
        .settle_ticks(0)
        .with_bundle((Mannequin::new(profile), NoGravity, body.stance))
        .spawn(commands)
        .id()
}

fn profile_of(uuid: Option<&PlayerUuid>) -> ResolvableProfile {
    ResolvableProfile {
        id: uuid.map(|uuid| uuid.0),
        ..ResolvableProfile::default()
    }
}

fn show(burst: Burst, viewer: Entity, players: &Players) {
    let [x, y, z] = burst.at;
    let [offset_x, offset_y, offset_z] = burst.spread;
    players.send(
        viewer,
        LevelParticles {
            long_distance: true,
            always_visible: true,
            x,
            y,
            z,
            offset_x,
            offset_y,
            offset_z,
            max_speed: burst.speed,
            count: burst.count,
            particle: burst.particle,
        },
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn tick(
    mut flights: Query<(Entity, &mut Flight)>,
    land: Land,
    passages: Query<(Entity, &Passage, Option<&PlayerUuid>, Has<PackLoaded>)>,
    sessions: Query<(Entity, &MapSession, Option<&PlayerUuid>)>,
    mut casts: Query<(Entity, &mut Cast)>,
    mut displays: Query<&mut Display>,
    mut stances: Query<&mut Stance>,
    players: Players,
    sounds: Sounds,
    mut commands: Commands,
) {
    for (camera, mut flight) in &mut flights {
        let (true, Some(scene)) = (flight.is_added(), flight.scene()) else {
            continue;
        };
        let by_passage = passages
            .iter()
            .find(|(_, passage, ..)| passage.camera.vehicle == camera)
            .map(|(player, _, uuid, drawn)| (player, uuid, drawn));
        let by_preview = || {
            sessions
                .iter()
                .find(|(_, session, _)| {
                    session
                        .preview
                        .and_then(|preview| preview.cameras())
                        .is_some_and(|cameras| cameras.vehicle == camera)
                })
                .map(|(player, session, uuid)| (player, uuid, session.drawn))
        };
        let Some((viewer, uuid, drawn)) = by_passage.or_else(by_preview) else {
            tracing::warn!(?camera, "a scene with nobody to see it");
            continue;
        };
        let view = land.view_for(&scene);
        let scene = scene.seen_from(view);
        flight.refilm(scene);
        tracing::info!(part = ?scene.part(), ?view, "map scene");
        commands.spawn(Cast {
            scene,
            camera,
            viewer,
            profile: profile_of(uuid),
            body: None,
            seat: None,
            seated: false,
            props: vec![None; scene.props().len()],
            drawn,
            played: None,
        });
    }

    for (entity, mut cast) in &mut casts {
        let Ok((_, flight)) = flights.get(cast.camera) else {
            for part in cast.props.iter().flatten().chain(&cast.body) {
                commands.entity(*part).try_despawn();
            }
            commands.entity(entity).despawn();
            tracing::info!(part = ?cast.scene.part(), "map scene over");
            continue;
        };
        let tick = flight.elapsed();
        if cast.played == Some(tick) {
            continue;
        }
        cast.played = Some(tick);
        let scene = cast.scene;
        let viewer = cast.viewer;
        if let Some(noise) = scene.noise(tick) {
            let sound = Sound::new(noise.name)
                .volume(noise.volume)
                .pitch(noise.pitch);
            let at = place(scene.feet());
            sounds.play_to(viewer, sound.at(DimensionId::Overworld, &at));
        }
        play_props(&mut cast, tick, &mut displays, &mut commands);
        play_body(&mut cast, tick, &mut stances, &mut commands);
        for burst in scene.bursts(tick) {
            show(burst, viewer, &players);
        }
    }
}

/// A new place for each prop each tick, as far ahead as it stays behind it.
fn play_props(
    cast: &mut Cast,
    tick: u32,
    displays: &mut Query<&mut Display>,
    commands: &mut Commands,
) {
    let scene = cast.scene;
    for (index, prop) in scene.props().iter().enumerate() {
        let Some(pose) = scene.pose(index, tick) else {
            if let Some(part) = cast.props[index].take() {
                commands.entity(part).try_despawn();
            }
            continue;
        };
        let Some(part) = cast.props[index] else {
            cast.props[index] = Some(spawn_prop(commands, prop, pose, cast.drawn));
            continue;
        };
        let ahead = scene
            .pose(index, tick + u32::from(prop.glide))
            .unwrap_or(pose);
        let [yaw, pitch] = ahead.look;
        commands
            .entity(part)
            .try_insert((place(ahead.at), Rotation { yaw, pitch }));
        let shape = scene
            .pose(index, tick + u32::from(prop.morph))
            .unwrap_or(pose)
            .shape;
        if let Ok(mut display) = displays.get_mut(part)
            && display.transform != shape
        {
            display.transform = shape;
        }
    }
}

/// A body that sits down or gets up is another entity: a client seats what rides from the
/// moment it does, and this way nothing has to throw it off.
fn play_body(
    cast: &mut Cast,
    tick: u32,
    stances: &mut Query<&mut Stance>,
    commands: &mut Commands,
) {
    let wanted = cast.scene.body(tick);
    let moved_seat = wanted.is_some_and(|body: Body| body.seat != cast.seat);
    if (wanted.is_none() || moved_seat)
        && let Some(entity) = cast.body.take()
    {
        commands.entity(entity).try_despawn();
    }
    let Some(body) = wanted else {
        return;
    };
    let Some(entity) = cast.body else {
        let entity = spawn_body(commands, body, cast.profile.clone());
        cast.body = Some(entity);
        cast.seat = body.seat;
        cast.seated = false;
        return;
    };
    // A tick after it came: a client drops a rider it has not been shown yet.
    if !cast.seated
        && let Some(seat) = body.seat.and_then(|seat| cast.props[seat])
    {
        commands.entity(seat).try_insert(Passengers::new([entity]));
        cast.seated = true;
    }
    commands.entity(entity).try_insert((
        place(body.at),
        Rotation {
            yaw: body.yaw,
            pitch: body.pitch,
        },
    ));
    if let Ok(mut stance) = stances.get_mut(entity) {
        stance.set_if_neq(body.stance);
    }
}
