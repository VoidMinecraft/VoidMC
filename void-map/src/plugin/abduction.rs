//! Plays a [`Scene`] for whoever has one on the flight of their camera: the real thing or a
//! preview, it is the same film. It starts when such a flight does, and takes its clock from
//! it, so it ends with the camera, which also takes care of a player leaving midway.

use bevy_app::App;
use bevy_ecs::prelude::*;
use voidmc::{
    Billboard, Display, DisplayTransform, EntityBuilder, EntityKind, EntityMetadata, ItemDisplay,
    ItemDisplayContext, ItemStack, Mannequin, MetadataSource, MetadataSourceAppExt, NoGravity,
    Players, Sound, Sounds,
    components::{PlayerUuid, Position, Rotation},
    world::DimensionId,
};
use voidmc_codec::Encode;
use voidmc_protocol::clientbound::{
    EntityMetadataValue as Value, LevelParticles, Particle, ParticleColor, ResolvableProfile,
    entity_metadata,
};
use voidmc_protocol::slot::{DataComponent, component_ids};

use super::{MapSession, PackLoaded, Passage};
use crate::abduction::{BEAM_MODEL, Body, Cue, HOVER, SAUCER_MODEL, Scene, Stance};
use crate::transition::{Flight, SEGMENT_TICKS};

/// How much longer than a segment a display of the scene takes over it, so as never to stop.
const GLIDE_TICKS: u8 = SEGMENT_TICKS as u8 + 2;
/// The saucer, at the scale of a model of one block across.
const SAUCER_SCALE: f32 = 3.4;
const BEAM_WIDTH: f32 = 6.5;
const BEAM_TICKS: u16 = 4;
/// The view range of what is seen from high up, as a share of what a client calls far.
const FAR: f32 = 3.0;
/// Sparks are short lived, for none to hang in front of the camera on its way back.
const SPARK_COLOUR: ParticleColor = ParticleColor(0xC8FFE0);
/// What a client counts as frozen through, and shakes a body for.
const FROZEN_TICKS: i32 = 140;
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

/// A scene under way, with who sees it and what it is made of.
#[derive(Component, Debug)]
pub(super) struct Cast {
    scene: Scene,
    /// Its flight is the clock.
    camera: Entity,
    viewer: Entity,
    profile: ResolvableProfile,
    body: Option<Entity>,
    saucer: Option<Entity>,
    beam: Option<Entity>,
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

fn spawn_saucer(commands: &mut Commands, at: [f64; 3], drawn: bool) -> Entity {
    let item = modelled(
        "minecraft:heavy_weighted_pressure_plate",
        SAUCER_MODEL,
        drawn,
    );
    EntityBuilder::new(EntityKind::ItemDisplay)
        .position(place(at))
        .with_bundle((
            Display::default()
                .transform(DisplayTransform::default().uniform_scale(SAUCER_SCALE))
                .brightness(15, 15)
                .view_range(FAR)
                .culling_box(12.0, 6.0)
                .teleport_ticks(GLIDE_TICKS),
            ItemDisplay::new(item).context(ItemDisplayContext::None),
        ))
        .spawn(commands)
        .id()
}

/// A sheet hanging from the saucer, which the client turns to the camera around its height.
fn beam_transform(reach: f32) -> DisplayTransform {
    let length = HOVER as f32 - 0.4;
    DisplayTransform::default()
        .translation(0.0, -0.5 - length * reach / 2.0, 0.0)
        .scale(BEAM_WIDTH, length * reach, 1.0)
}

fn spawn_beam(commands: &mut Commands, at: [f64; 3], drawn: bool) -> Entity {
    let item = modelled("minecraft:white_stained_glass_pane", BEAM_MODEL, drawn);
    EntityBuilder::new(EntityKind::ItemDisplay)
        .position(place(at))
        .with_bundle((
            Display::default()
                .transform(beam_transform(0.0))
                .interpolation_ticks(BEAM_TICKS)
                .billboard(Billboard::Vertical)
                .brightness(15, 15)
                .view_range(FAR)
                .culling_box(10.0, 14.0)
                .teleport_ticks(GLIDE_TICKS),
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

fn play_cue(cue: Cue, scene: &Scene, viewer: Entity, sounds: &Sounds, players: &Players) {
    let feet = scene.feet();
    let sound = |name: &str, volume: f32, pitch: f32| {
        let sound = Sound::new(name).volume(volume).pitch(pitch);
        sounds.play_to(viewer, sound.at(DimensionId::Overworld, &place(feet)));
    };
    match cue {
        Cue::SaucerComes => sound("entity.phantom.swoop", 2.0, 0.6),
        Cue::BeamOn => sound("block.beacon.activate", 2.0, 1.4),
        Cue::Grabbed => sound("entity.villager.hurt", 2.0, 1.3),
        Cue::Taken => sound("entity.enderman.teleport", 2.0, 1.0),
        Cue::SaucerGoes => sound("entity.firework_rocket.launch", 2.0, 0.6),
        Cue::Dropped => sound("entity.villager.no", 2.0, 1.2),
        Cue::Landed => {
            sound("entity.player.big_fall", 2.0, 1.0);
            let [x, y, z] = feet;
            players.send(
                viewer,
                LevelParticles {
                    long_distance: true,
                    always_visible: true,
                    x,
                    y: y + 0.1,
                    z,
                    offset_x: 0.5,
                    offset_y: 0.1,
                    offset_z: 0.5,
                    max_speed: 0.02,
                    count: 6,
                    particle: Particle::Cloud,
                },
            );
        }
    }
}

/// Sparks going up or down the beam, around the body.
fn sparkle(scene: &Scene, viewer: Entity, tick: u32, players: &Players) {
    let [x, y, z] = scene.feet();
    let height = f64::from(tick * 7 % 100) / 100.0 * HOVER;
    players.send(
        viewer,
        LevelParticles {
            long_distance: true,
            always_visible: true,
            x,
            y: y + height,
            z,
            offset_x: 0.7,
            offset_y: 0.7,
            offset_z: 0.7,
            max_speed: 0.0,
            count: 3,
            particle: Particle::Dust {
                color: SPARK_COLOUR,
                scale: 1.2,
            },
        },
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn tick(
    started: Query<(Entity, &Flight), Added<Flight>>,
    flights: Query<&Flight>,
    passages: Query<(Entity, &Passage, Option<&PlayerUuid>, Has<PackLoaded>)>,
    sessions: Query<(Entity, &MapSession, Option<&PlayerUuid>)>,
    mut casts: Query<(Entity, &mut Cast)>,
    mut displays: Query<&mut Display>,
    mut stances: Query<&mut Stance>,
    players: Players,
    sounds: Sounds,
    mut commands: Commands,
) {
    for (camera, flight) in &started {
        let Some(scene) = flight.scene() else {
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
        tracing::info!(part = ?scene.part(), "map scene");
        commands.spawn(Cast {
            scene,
            camera,
            viewer,
            profile: profile_of(uuid),
            body: None,
            saucer: None,
            beam: None,
            drawn,
            played: None,
        });
    }

    for (entity, mut cast) in &mut casts {
        let Ok(flight) = flights.get(cast.camera) else {
            for part in [cast.body, cast.saucer, cast.beam].into_iter().flatten() {
                commands.entity(part).try_despawn();
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
        if let Some(cue) = scene.cue(tick) {
            play_cue(cue, &scene, viewer, &sounds, &players);
        }
        play_saucer(&mut cast, tick, &mut displays, &mut commands);
        match (scene.body(tick), cast.body) {
            (Some(body), None) => {
                let profile = cast.profile.clone();
                cast.body = Some(spawn_body(&mut commands, body, profile));
            }
            (Some(body), Some(entity)) => {
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
            (None, Some(entity)) => {
                commands.entity(entity).try_despawn();
                cast.body = None;
            }
            (None, None) => {}
        }
        if scene.beam(tick) {
            sparkle(&scene, viewer, tick, &players);
        }
    }
}

/// A new place for the saucer and the beam once a segment, for the client to glide to.
fn play_saucer(
    cast: &mut Cast,
    tick: u32,
    displays: &mut Query<&mut Display>,
    commands: &mut Commands,
) {
    let scene = cast.scene;
    if scene.saucer_gone(tick) {
        for part in [cast.saucer.take(), cast.beam.take()].into_iter().flatten() {
            commands.entity(part).try_despawn();
        }
        return;
    }
    if !scene.saucer_shown(tick) {
        return;
    }
    if cast.saucer.is_none() {
        let (at, _) = scene.saucer(tick);
        cast.saucer = Some(spawn_saucer(commands, at, cast.drawn));
        cast.beam = Some(spawn_beam(commands, at, cast.drawn));
    }
    let on = scene.beam(tick);
    if let Some(beam) = cast.beam
        && let Ok(mut display) = displays.get_mut(beam)
    {
        let wanted = beam_transform(if on { 1.0 } else { 0.0 });
        if display.transform != wanted {
            display.transform = wanted;
        }
    }
    if !tick.is_multiple_of(SEGMENT_TICKS) {
        return;
    }
    let (at, [yaw, pitch]) = scene.saucer(tick + SEGMENT_TICKS);
    if let Some(saucer) = cast.saucer {
        commands
            .entity(saucer)
            .try_insert((place(at), Rotation { yaw, pitch }));
    }
    if let Some(beam) = cast.beam {
        commands.entity(beam).try_insert(place(at));
    }
}
