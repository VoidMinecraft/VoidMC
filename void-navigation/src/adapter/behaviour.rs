use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc::components::{PlayerDimension, PlayerReady, Position};
use voidmc::world::DimensionId;

use super::navigator::{Goal, Navigator};
use super::systems::to_vec;
use crate::pathing::Vec3;

/// What a behaviour wants this evaluation.
#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    /// Not applicable now; lower-priority behaviours get a turn.
    Pass,
    /// Applicable, and the navigator's current goal is fine as it is.
    Keep,
    /// Applicable: pursue this goal (a no-op if it is already the goal).
    Pursue(Goal),
}

/// What a behaviour can look at when it is evaluated.
pub struct BehaviourContext<'a> {
    pub entity: Entity,
    pub position: Vec3,
    pub dimension: DimensionId,
    pub navigator: &'a Navigator,
    pub active: bool,
    pub tick: u64,
    players: &'a [(Entity, Vec3, DimensionId)],
}

impl BehaviourContext<'_> {
    /// The closest ready player in the same dimension within `range` blocks.
    pub fn nearest_player(&self, range: f64) -> Option<(Entity, Vec3, f64)> {
        let range_sq = range * range;
        self.players
            .iter()
            .filter(|(_, _, dimension)| *dimension == self.dimension)
            .map(|&(player, position, _)| {
                (player, position, position.distance_squared(self.position))
            })
            .filter(|&(_, _, distance_sq)| distance_sq <= range_sq)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(player, position, distance_sq)| (player, position, distance_sq.sqrt()))
    }

    pub fn players_within(&self, range: f64) -> impl Iterator<Item = (Entity, Vec3)> + '_ {
        let range_sq = range * range;
        self.players
            .iter()
            .filter(move |(_, position, dimension)| {
                *dimension == self.dimension && position.distance_squared(self.position) <= range_sq
            })
            .map(|&(player, position, _)| (player, position))
    }

    /// A deterministic pseudo-random number in `[0, 1)`, different per entity
    /// and per tick.
    pub fn random(&self, salt: u64) -> f64 {
        let mut x = self.entity.to_bits() ^ self.tick.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt;
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        x ^= x >> 33;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

type Select = dyn Fn(&BehaviourContext) -> Choice + Send + Sync;

/// One entry of a [`Behaviours`] list.
#[derive(Clone)]
pub struct Behaviour {
    name: &'static str,
    select: Arc<Select>,
}

impl std::fmt::Debug for Behaviour {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Behaviour").field(&self.name).finish()
    }
}

impl Behaviour {
    pub fn new(
        name: &'static str,
        select: impl Fn(&BehaviourContext) -> Choice + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            select: Arc::new(select),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Follows the closest player within `range`, keeping `distance`.
    pub fn follow_nearest_player(range: f64, distance: f64) -> Self {
        Self::new("follow_player", move |ctx| {
            match ctx.nearest_player(range) {
                Some((player, _, _)) => Choice::Pursue(Goal::follow(player, distance)),
                None => Choice::Pass,
            }
        })
    }

    /// Runs from any player closer than `range` until `distance` away.
    pub fn flee_nearest_player(range: f64, distance: f64) -> Self {
        Self::new("flee_player", move |ctx| match ctx.nearest_player(range) {
            Some((player, _, _)) => Choice::Pursue(Goal::flee(player, distance)),
            None if ctx.active && !ctx.navigator.is_idle() => Choice::Keep,
            None => Choice::Pass,
        })
    }

    pub fn patrol(points: impl IntoIterator<Item = impl Into<Vec3>>) -> Self {
        let goal = Goal::patrol(points);
        Self::new("patrol", move |ctx| {
            if ctx.active && !ctx.navigator.is_idle() {
                Choice::Keep
            } else {
                Choice::Pursue(goal.clone())
            }
        })
    }

    /// Strolls to random spots within `radius` of `home`, pausing between
    /// walks.
    pub fn wander(home: impl Into<Vec3>, radius: f64) -> Self {
        let home = home.into();
        Self::new("wander", move |ctx| {
            if ctx.active && !ctx.navigator.is_idle() {
                return Choice::Keep;
            }
            if ctx.random(1) > 0.25 {
                return Choice::Keep;
            }
            let angle = ctx.random(2) * std::f64::consts::TAU;
            let reach = radius * ctx.random(3).sqrt();
            let target = Vec3::new(
                home.x + angle.cos() * reach,
                ctx.position.y,
                home.z + angle.sin() * reach,
            );
            Choice::Pursue(Goal::MoveTo {
                target,
                radius: 1.5,
            })
        })
    }
}

/// A priority list of behaviours: every `interval` ticks the first one that
/// does not [`Choice::Pass`] drives the entity's [`Navigator`].
#[derive(Component, Clone, Debug)]
#[require(Navigator)]
pub struct Behaviours {
    list: Vec<Behaviour>,
    interval: u16,
    countdown: u16,
    active: Option<usize>,
}

impl Default for Behaviours {
    fn default() -> Self {
        Self::new()
    }
}

impl Behaviours {
    pub fn new() -> Self {
        Self {
            list: Vec::new(),
            interval: 10,
            countdown: 0,
            active: None,
        }
    }

    /// Appends a behaviour below the ones already added.
    pub fn with(mut self, behaviour: Behaviour) -> Self {
        self.list.push(behaviour);
        self
    }

    pub fn interval(mut self, ticks: u16) -> Self {
        self.interval = ticks.max(1);
        self
    }

    pub fn active(&self) -> Option<&'static str> {
        self.active.map(|index| self.list[index].name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Behaviour> {
        self.list.iter()
    }
}

#[derive(Resource, Default)]
pub(crate) struct PlayerSnapshot {
    players: Vec<(Entity, Vec3, DimensionId)>,
    tick: u64,
}

pub(crate) fn select_behaviours(
    mut snapshot: ResMut<PlayerSnapshot>,
    players: Query<(Entity, &Position, &PlayerDimension), With<PlayerReady>>,
    mut agents: Query<(
        Entity,
        &mut Behaviours,
        &mut Navigator,
        &Position,
        Option<&voidmc::components::EntityDimension>,
    )>,
) {
    let snapshot = snapshot.as_mut();
    snapshot.tick += 1;
    let mut refreshed = false;
    for (entity, mut behaviours, mut navigator, position, dimension) in &mut agents {
        if behaviours.countdown > 0 {
            behaviours.countdown -= 1;
            continue;
        }
        if !refreshed {
            snapshot.players.clear();
            snapshot.players.extend(
                players
                    .iter()
                    .map(|(player, position, dimension)| (player, to_vec(position), dimension.0)),
            );
            refreshed = true;
        }
        let behaviours = behaviours.as_mut();
        behaviours.countdown = behaviours.interval - 1;
        let mut chosen = None;
        for (index, behaviour) in behaviours.list.iter().enumerate() {
            let ctx = BehaviourContext {
                entity,
                position: to_vec(position),
                dimension: dimension.map(|d| d.0).unwrap_or(DimensionId::Overworld),
                navigator: &navigator,
                active: behaviours.active == Some(index),
                tick: snapshot.tick,
                players: &snapshot.players,
            };
            match (behaviour.select)(&ctx) {
                Choice::Pass => continue,
                choice => {
                    chosen = Some((index, choice));
                    break;
                }
            }
        }
        match chosen {
            Some((index, Choice::Pursue(goal))) => {
                behaviours.active = Some(index);
                navigator.set_goal(goal);
            }
            Some((index, _)) => {
                if behaviours.active != Some(index) {
                    navigator.stop();
                }
                behaviours.active = Some(index);
            }
            None => {
                if behaviours.active.take().is_some() {
                    navigator.stop();
                }
            }
        }
    }
}

pub(crate) fn stagger_behaviours(
    event: On<bevy_ecs::lifecycle::Add, Behaviours>,
    mut agents: Query<&mut Behaviours>,
) {
    if let Ok(mut behaviours) = agents.get_mut(event.entity) {
        let interval = behaviours.interval as u64;
        behaviours.countdown = (event.entity.to_bits() % interval) as u16;
    }
}
