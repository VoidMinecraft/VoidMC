//! Playing an intro or an outro over, to see how it looks, without leaving the map.
//!
//! The world stays on the client all the time a player is in the map, so an eye can fly
//! over it as it would on a real way in or out. The player stays where they are, on the
//! rig: only the view goes. A veil covers each cut, and at the end the view is back where
//! it was.

use bevy_ecs::prelude::*;
use voidmc::{Players, components::MinecraftEntityId, components::Position, world::ChunkPos};

use super::{
    Display, Ending, EyeCamera, LANDING_PATIENCE_TICKS, MapMode, MapSession, NEAR_WORLD_CHUNKS,
    Rotation, SetCamera, Travel, despawn_eye_camera, eye_of, hand_view_back, send_veil, set_off,
    settled, spawn_eye_camera, turn_eye, under_eye,
};
use crate::transition::{
    CLOSING_TICKS, FLIGHT_GLIDE_TICKS, Flight, Flow, OPENING_TICKS, TURN_TICKS, VEIL_FADE, veil_out,
};

/// How long the view stays on the ground at the end of an outro, to see it arrive.
const HOLD_TICKS: u32 = 20;

/// Which way to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowPart {
    Intro,
    Outro,
}

/// Plays a flow over for a player in the map, whichever they chose. Nothing changes for
/// them once it is over, and it does nothing while the view is busy. To make it their
/// choice as well, there is `ChooseLook` with `LookChoice::Intro` or `LookChoice::Outro`.
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewFlow(pub Entity, pub FlowPart, pub Flow);

/// A flow being played over.
#[derive(Debug, Clone, Copy)]
pub(super) struct Preview {
    flow: Flow,
    stage: Stage,
    /// What sees from the world, once there is one.
    camera: Option<EyeCamera>,
    /// The ticks gone by in this stage.
    ticks: u32,
}

#[derive(Debug, Clone, Copy)]
enum Stage {
    /// Under the veil, from the view to the eye of the player.
    Leaving(Flight),
    /// Up through the world.
    Rising(Flight),
    /// The camera of the view goes to where the opening starts, unseen.
    Crossing,
    /// Down to the marker, as any flight of the view.
    Closing,
    /// Under the veil, over the world, until it is there.
    Waiting(Flight),
    /// Down through the world.
    Descending,
    /// On the ground for a moment.
    Holding,
    /// Under the veil again, back to the view.
    Returning,
}

impl Preview {
    fn new(flow: Flow, stage: Stage, camera: Option<EyeCamera>) -> Self {
        Self {
            flow,
            stage,
            camera,
            ticks: 0,
        }
    }

    fn to(&mut self, stage: Stage) {
        self.stage = stage;
        self.ticks = 0;
    }

    /// The flow an outro takes, if this is one.
    pub(super) fn flow(&self) -> Flow {
        self.flow
    }

    /// A flight of the view goes on as usual. Past that, this is in charge of the view.
    pub(super) fn flies_the_view(&self) -> bool {
        matches!(self.stage, Stage::Closing)
    }

    /// The title of the player belongs to the veil, or soon will.
    pub(super) fn holds_the_title(&self) -> bool {
        !self.flies_the_view()
    }

    pub(super) fn cameras(&self) -> Option<EyeCamera> {
        self.camera
    }
}

/// Sets a preview off, if the view is free.
pub(super) fn start(
    event: On<PreviewFlow>,
    mut sessions: Query<&mut MapSession>,
    mut displays: Query<&mut Display>,
    players: Players,
    mut commands: Commands,
) {
    let PreviewFlow(player, part, flow) = *event;
    let Ok(mut session) = sessions.get_mut(player) else {
        return;
    };
    let free = session.rig.is_some()
        && session.mode == MapMode::Pan
        && session.travel.is_none()
        && session.veiled.is_none()
        && session.wheel.is_none()
        && session.preview.is_none();
    if !free {
        tracing::info!(?part, ?flow, "map preview refused: the view is busy");
        return;
    }
    tracing::info!(?part, ?flow, "map preview");
    let home = ChunkPos::from_block(session.position.x, session.position.z);
    session.heading = Some(home);
    match part {
        FlowPart::Intro => {
            let flight = flow.rise(eye_of(&session.position), session.rotation);
            let [x, y, z] = flight.start();
            let at = under_eye(Position { x, y, z });
            let camera = spawn_eye_camera(&mut commands, at, session.rotation);
            veil_on(&session, &players, player);
            let stage = Stage::Leaving(flight);
            session.preview = Some(Preview::new(flow, stage, Some(camera)));
        }
        FlowPart::Outro => {
            let flight = session.way_to_marker(flow, CLOSING_TICKS);
            let travel = Travel {
                flight,
                ending: Ending::World(None),
            };
            session.preview = Some(Preview::new(flow, Stage::Closing, None));
            set_off(&mut session, travel, &mut displays, &mut commands);
        }
    }
}

/// A client with no pack has no veil, and sees each cut as it is.
fn veil_on(session: &MapSession, players: &Players, player: Entity) {
    if session.drawn {
        send_veil(players, player);
    }
}

fn veil_off(session: &MapSession, players: &Players, player: Entity) {
    if session.drawn {
        players.send(player, veil_out());
    }
}

/// What sees through the rig, in a pan.
fn view_camera(session: &MapSession, ids: &Query<&MinecraftEntityId>) -> Option<i32> {
    let camera = session.rig.and_then(|rig| rig.camera)?;
    ids.get(camera).ok().map(|id| id.0)
}

pub(super) fn tick(
    mut sessions: Query<(Entity, &mut MapSession, &MinecraftEntityId)>,
    ids: Query<&MinecraftEntityId>,
    flights: Query<&Flight>,
    mut displays: Query<&mut Display>,
    players: Players,
    mut commands: Commands,
) {
    for (player, mut session, id) in &mut sessions {
        let Some(mut preview) = session.preview else {
            continue;
        };
        preview.ticks += 1;
        let ticks = preview.ticks;
        let eye = preview.camera.and_then(|camera| ids.get(camera.eye).ok());
        let flying = preview
            .camera
            .and_then(|camera| flights.get(camera.vehicle).ok())
            .copied();
        if let (Some(eye), Some(flight)) = (eye, flying)
            && flight.eye_turning()
        {
            let [x, y, z] = flight.at();
            turn_eye(
                &players,
                player,
                eye,
                Position { x, y, z },
                flight.looking(),
            );
        }
        let vehicle = preview.camera.map(|camera| camera.vehicle);
        match preview.stage {
            Stage::Leaving(flight) => {
                if ticks == TURN_TICKS
                    && let Some(eye) = eye
                {
                    let [x, y, z] = flight.start();
                    let at = Position { x, y, z };
                    players.send(player, settled(eye, at, flight.first_look()));
                }
                if ticks > VEIL_FADE + 1
                    && let (Some(eye), Some(vehicle)) = (eye, vehicle)
                {
                    players.send(player, SetCamera { camera_id: eye.0 });
                    veil_off(&session, &players, player);
                    commands.entity(vehicle).try_insert(flight);
                    preview.to(Stage::Rising(flight));
                }
            }
            Stage::Rising(flight) => {
                if ticks + VEIL_FADE == flight.length() {
                    veil_on(&session, &players, player);
                }
                if ticks >= flight.length() {
                    let opening = session.way_to_marker(preview.flow, OPENING_TICKS);
                    let travel = Travel {
                        flight: opening.reversed(),
                        ending: Ending::View,
                    };
                    set_off(&mut session, travel, &mut displays, &mut commands);
                    preview.to(Stage::Crossing);
                }
            }
            Stage::Crossing => {
                if ticks > u32::from(FLIGHT_GLIDE_TICKS) + 1 {
                    if let Some(camera_id) = view_camera(&session, &ids) {
                        players.send(player, SetCamera { camera_id });
                    }
                    if let Some(camera) = preview.camera {
                        despawn_eye_camera(camera, &mut commands);
                    }
                    // From here it is the end of any intro, under the veil.
                    session.veiled = Some(0);
                    session.preview = None;
                    continue;
                }
            }
            Stage::Closing => {
                let done = session
                    .travel
                    .is_none_or(|travel| travel.flight.flying_for() == 0);
                if done {
                    let landing = session.landing.take();
                    let (displays, commands) = (&mut displays, &mut commands);
                    hand_view_back(&mut session, player, id, displays, &players, commands);
                    let flight = preview
                        .flow
                        .landing(eye_of(&session.position), session.rotation);
                    if let Some(eye) = landing.and_then(|camera| ids.get(camera.eye).ok()) {
                        players.send(player, SetCamera { camera_id: eye.0 });
                    }
                    preview.camera = landing;
                    preview.to(Stage::Waiting(flight));
                }
            }
            Stage::Waiting(flight) => {
                let home = ChunkPos::from_block(session.position.x, session.position.z);
                let near = home.chunks_in_radius(NEAR_WORLD_CHUNKS);
                let whole = near.iter().all(|chunk| session.world.contains(chunk));
                if (whole && ticks > VEIL_FADE) || ticks >= LANDING_PATIENCE_TICKS {
                    veil_off(&session, &players, player);
                    if let Some(vehicle) = vehicle {
                        commands.entity(vehicle).try_insert(flight);
                    }
                    preview.to(Stage::Descending);
                }
            }
            Stage::Descending => {
                let Some(flight) = flying else {
                    continue;
                };
                if flight.settling()
                    && let Some(camera) = vehicle.and_then(|vehicle| ids.get(vehicle).ok())
                {
                    let [x, y, z] = flight.end();
                    let at = under_eye(Position { x, y, z });
                    players.send(player, settled(camera, at, flight.looking()));
                }
                if flight.left() == 0 {
                    preview.to(Stage::Holding);
                }
            }
            Stage::Holding => {
                if ticks >= HOLD_TICKS {
                    veil_on(&session, &players, player);
                    preview.to(Stage::Returning);
                }
            }
            Stage::Returning => {
                if ticks > VEIL_FADE + 1 {
                    if let Some(camera_id) = view_camera(&session, &ids) {
                        players.send(player, SetCamera { camera_id });
                    }
                    if let Some(camera) = preview.camera {
                        despawn_eye_camera(camera, &mut commands);
                    }
                    veil_off(&session, &players, player);
                    session.preview = None;
                    continue;
                }
            }
        }
        session.preview = Some(preview);
    }
}

/// How an eye looks when it starts, for what a flow gets from `Flight`.
pub(super) fn first_rotation(flight: &Flight) -> Rotation {
    let [yaw, pitch] = flight.first_look();
    Rotation { yaw, pitch }
}
