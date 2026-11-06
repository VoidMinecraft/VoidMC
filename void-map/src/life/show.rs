//! Plays the scenes on the model of each session.
//!
//! An actor is an item display, which glides to wherever it is told over `GLIDE_TICKS`.
//! So a scene is told where it will be every `STEP_TICKS`, a little further ahead than
//! that: the client moves it at a steady pace, and never comes to a stop waiting for the
//! next place. What stands still sends nothing, and a scene out of the view has no entity
//! at all. As for figures, another model takes new entities: a client's glide outlives a
//! jump of more than 64 blocks, and pulls whatever jumped back towards where it was.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use voidmc::{
    Display, DisplayTransform, EntityBuilder, EntityKind, ItemDisplay, ItemDisplayContext,
    components::{Position, Rotation},
};

use super::{Land, LifeScene, Model, Placed, Playback};
use crate::plugin::{Atlas, MapSession, MapView};
use crate::scene::{SEA_Y, Scale, Scene};

/// How often a scene in the view is told where to go.
const STEP_TICKS: u64 = 10;
/// How far ahead it is told, and how long the client takes to get there. Longer than a
/// step, so that the next place comes before the last one is reached.
const GLIDE_TICKS: u8 = 12;
/// In the range of a client's display entities, past any height of the view.
const VIEW_RANGE: f32 = 8.0;
/// A view sees about this far around the land under the middle of the screen, in heights
/// over the sea, on a wide window.
const SEEN_PER_HEIGHT: f64 = 1.6;
/// How far north of the eye that land is, in heights over the sea.
const LOOKS_AHEAD: f64 = 0.2;
/// The least and the most a view reaches, in blocks of the model: at the most, the fog
/// of a render distance of 8 has covered it all.
const REACH: [f64; 2] = [40.0, 180.0];
/// How much further a scene that is shown has to go to be dropped: it must not come and go
/// on the edge of the view.
const REACH_MARGIN: f64 = 24.0;
const SUMMARY_PERIOD: u64 = 200;

/// The scenes of this map, ready to play, and the clock they all go by.
#[derive(Resource, Debug, Default)]
pub struct Life {
    plays: Vec<Playback>,
    tick: u64,
}

impl Life {
    pub fn new(scenes: &[LifeScene]) -> Self {
        Self {
            plays: scenes.iter().map(Playback::new).collect(),
            tick: 0,
        }
    }
}

/// Whether a player sees the scenes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LifeLook {
    #[default]
    On,
    Off,
}

impl LifeLook {
    pub const ALL: [Self; 2] = [Self::On, Self::Off];

    pub fn label(self) -> &'static str {
        match self {
            Self::On => "Actives",
            Self::Off => "Coupées",
        }
    }
}

impl Land for Scene {
    fn on_model(&self, at: [f64; 2]) -> [f64; 2] {
        at.map(|value| self.length(value))
    }

    fn length(&self, world: f64) -> f64 {
        world / f64::from(self.scale().blocks_per_cell)
    }

    fn ground(&self, at: [f64; 2]) -> Option<f64> {
        let (x, z) = cell(self, at)?;
        Some(f64::from(self.top(x, z)? + 1))
    }

    fn water(&self, at: [f64; 2]) -> bool {
        cell(self, at)
            .and_then(|(x, z)| self.top(x, z))
            .is_some_and(|top| top <= SEA_Y)
    }

    fn free(&self, at: [f64; 2]) -> bool {
        cell(self, at).is_some_and(|(x, z)| self.walkable(x, z))
    }
}

fn cell(scene: &Scene, at: [f64; 2]) -> Option<(i32, i32)> {
    scene.cell_of(at[0].floor() as i32, at[1].floor() as i32)
}

/// An actor on a session's model.
#[derive(Debug, Clone, Copy)]
struct Body {
    entity: Entity,
    model: Model,
}

/// A scene on a session's model.
#[derive(Debug, Clone)]
struct Shown {
    bodies: Vec<Option<Body>>,
    /// When it is next told where to go.
    next: u64,
}

/// What a session shows of the scenes.
#[derive(Component, Debug, Default)]
struct Stage {
    model: Option<Scale>,
    shown: BTreeMap<usize, Shown>,
}

impl Stage {
    fn clear(&mut self, commands: &mut Commands) {
        for shown in std::mem::take(&mut self.shown).into_values() {
            drop_scene(&shown, commands);
        }
    }
}

fn drop_scene(shown: &Shown, commands: &mut Commands) {
    for body in shown.bodies.iter().flatten() {
        commands.entity(body.entity).try_despawn();
    }
}

/// Whether a place of the model is in sight of an eye. `leaving` is for what is there
/// already. The view looks north and down, so that it goes by the land north of the eye.
fn in_reach([x, y, z]: [f64; 3], at: [f64; 3], leaving: bool) -> bool {
    let height = (y - f64::from(SEA_Y)).max(1.0);
    let margin = if leaving { REACH_MARGIN } else { 0.0 };
    let reach = (height * SEEN_PER_HEIGHT).clamp(REACH[0], REACH[1]) + margin;
    (at[0] - x).hypot(at[2] - (z - height * LOOKS_AHEAD)) <= reach
}

fn world_spot(view: &MapView, [x, y, z]: [f64; 3]) -> Position {
    Position {
        x: view.origin[0] + x,
        y,
        z: view.origin[1] + z,
    }
}

/// How large an actor is on a model.
fn size_on(play: &Playback, actor: usize, land: &Scene) -> f64 {
    let cast = play.actor(actor);
    land.length(cast.size).max(cast.least)
}

fn spawn(
    commands: &mut Commands,
    view: &MapView,
    placed: &Placed,
    size: f64,
    has_pack: bool,
) -> Entity {
    let transform = DisplayTransform::default()
        .translation(0.0, (size / 2.0) as f32, 0.0)
        .uniform_scale(size as f32);
    EntityBuilder::new(EntityKind::ItemDisplay)
        .position(world_spot(view, placed.at))
        .rotation(placed.yaw, 0.0)
        .gravity(false)
        .block_collision(false)
        .settle_ticks(0)
        .with_bundle((
            Display::default()
                .transform(transform)
                .brightness(15, 15)
                .view_range(VIEW_RANGE)
                .culling_box(size as f32 * 1.5, size as f32 * 1.5)
                .teleport_ticks(GLIDE_TICKS),
            ItemDisplay::new(placed.model.item(has_pack)).context(ItemDisplayContext::None),
        ))
        .spawn(commands)
        .id()
}

/// What the scenes cost, between two summaries.
#[derive(Resource, Debug, Default)]
struct Cost {
    moves: u64,
    turns: u64,
    spawns: u64,
    swaps: u64,
    spent: Duration,
    longest: Duration,
}

type Bodies<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Position,
        &'static mut Rotation,
        &'static mut ItemDisplay,
    ),
>;

/// Keeps every session's model up with the scenes.
fn play(
    mut life: ResMut<Life>,
    atlas: Res<Atlas>,
    mut sessions: Query<(&MapSession, &mut Stage)>,
    mut bodies: Bodies,
    mut cost: ResMut<Cost>,
    mut commands: Commands,
) {
    let started = Instant::now();
    life.tick += 1;
    let tick = life.tick;
    let (mut shown_scenes, mut shown_actors) = (0, 0);
    for (session, mut stage) in &mut sessions {
        let Some(view) = session.view(&atlas) else {
            continue;
        };
        let stage = &mut *stage;
        if session.looks().life == LifeLook::Off {
            stage.clear(&mut commands);
            stage.model = None;
            continue;
        }
        let model = view.scene.scale();
        let fresh = stage.model.replace(model) != Some(model);
        if fresh {
            stage.clear(&mut commands);
        }
        for (index, play) in life.plays.iter().enumerate() {
            let due = stage.shown.get(&index).map_or(
                fresh || tick % STEP_TICKS == index as u64 % STEP_TICKS,
                |shown| tick >= shown.next,
            );
            if !due {
                continue;
            }
            let now: Vec<_> = (0..play.actors())
                .map(|actor| play.place(actor, tick, view.scene))
                .collect();
            let shown = stage.shown.contains_key(&index);
            let near = now
                .iter()
                .flatten()
                .any(|placed| in_reach(view.eye, placed.at, shown));
            match stage.shown.get_mut(&index) {
                Some(scene) if near => {
                    scene.next = tick + STEP_TICKS;
                    for (actor, slot) in scene.bodies.iter_mut().enumerate() {
                        let Some(placed) =
                            play.place(actor, tick + u64::from(GLIDE_TICKS), view.scene)
                        else {
                            continue;
                        };
                        let Some(body) = slot else {
                            continue;
                        };
                        let Ok((mut position, mut rotation, mut item)) =
                            bodies.get_mut(body.entity)
                        else {
                            continue;
                        };
                        if position.set_if_neq(world_spot(&view, placed.at)) {
                            cost.moves += 1;
                        }
                        let turned = rotation.set_if_neq(Rotation {
                            yaw: placed.yaw,
                            pitch: 0.0,
                        });
                        if turned {
                            cost.turns += 1;
                        }
                        if body.model != placed.model {
                            body.model = placed.model;
                            item.item = placed.model.item(session.has_pack());
                            cost.swaps += 1;
                        }
                    }
                }
                Some(_) => {
                    if let Some(scene) = stage.shown.remove(&index) {
                        drop_scene(&scene, &mut commands);
                    }
                }
                None if near => {
                    let spawned = now.iter().enumerate().map(|(actor, placed)| {
                        let placed = placed.as_ref()?;
                        cost.spawns += 1;
                        let size = size_on(play, actor, view.scene);
                        let pack = session.has_pack();
                        Some(Body {
                            entity: spawn(&mut commands, &view, placed, size, pack),
                            model: placed.model,
                        })
                    });
                    let scene = Shown {
                        bodies: spawned.collect(),
                        next: tick + 1,
                    };
                    stage.shown.insert(index, scene);
                }
                None => {}
            }
        }
        shown_scenes += stage.shown.len();
        shown_actors += stage
            .shown
            .values()
            .map(|shown| shown.bodies.len())
            .sum::<usize>();
    }
    let took = started.elapsed();
    cost.spent += took;
    cost.longest = cost.longest.max(took);
    if tick.is_multiple_of(SUMMARY_PERIOD) {
        let cost = std::mem::take(&mut *cost);
        tracing::info!(
            scenes = life.plays.len(),
            shown_scenes,
            shown_actors,
            mean_us = cost.spent.as_micros() as u64 / SUMMARY_PERIOD,
            longest_us = cost.longest.as_micros() as u64,
            // A move is a packet, and a turn takes one more for the head.
            packets_per_second = (cost.moves + cost.turns + cost.swaps) * 20 / SUMMARY_PERIOD,
            spawns = cost.spawns,
            swaps = cost.swaps,
            "map life"
        );
    }
}

fn give_stage(sessions: Query<Entity, (With<MapSession>, Without<Stage>)>, mut commands: Commands) {
    for player in &sessions {
        commands.entity(player).insert(Stage::default());
    }
}

/// On leaving the map, and on leaving the server, which takes the player away too.
fn take_stage(
    event: On<bevy_ecs::lifecycle::Remove, MapSession>,
    mut stages: Query<&mut Stage>,
    mut commands: Commands,
) {
    if let Ok(mut stage) = stages.get_mut(event.entity) {
        stage.clear(&mut commands);
        commands.entity(event.entity).try_remove::<Stage>();
    }
}

pub struct LifePlugin;

impl Plugin for LifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Life>()
            .init_resource::<Cost>()
            .add_systems(Update, (give_stage, play).chain())
            .add_observer(take_stage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EYE: [f64; 3] = [0.0, 164.0, 0.0];

    #[test]
    fn what_is_past_the_reach_of_the_eye_is_out_of_sight() {
        assert!(in_reach(EYE, [0.0, 65.0, -100.0], false));
        assert!(!in_reach(EYE, [300.0, 65.0, 0.0], false));
    }

    /// Or a scene on the edge would come and go, each time with all its entities.
    #[test]
    fn a_scene_stays_a_little_past_where_it_came_in() {
        let reach = REACH[1].min(100.0 * SEEN_PER_HEIGHT);
        let just_out = [reach + 10.0, 65.0, -100.0 * LOOKS_AHEAD];
        assert!(!in_reach(EYE, just_out, false));
        assert!(in_reach(EYE, just_out, true));
    }

    #[test]
    fn a_low_view_still_keeps_what_is_around_it() {
        let low = [0.0, 70.0, 0.0];
        assert!(in_reach(low, [REACH[0] - 10.0, 65.0, 0.0], false));
    }
}
