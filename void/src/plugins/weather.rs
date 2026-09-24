//! Weather as an entity: spawn a [`Weather`], mutate its `kind`, despawn it.
//! The sync system diffs the target against what viewers last received and
//! sends only the matching Game Event packets; a `transition` ramps the rain
//! and thunder levels server-side, one level packet per tick, like vanilla.

use std::collections::HashSet;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::{GameEvent, GameEventType};

use super::viewers::{claimed, desired_viewers};
use crate::players::{Audience, Players};
use crate::schedule::VoidSystems;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WeatherKind {
    #[default]
    Clear,
    Rain {
        level: f32,
    },
    Thunder {
        rain: f32,
        thunder: f32,
    },
}

impl WeatherKind {
    pub fn raining(&self) -> bool {
        !matches!(self, WeatherKind::Clear)
    }

    /// `(rain, thunder)` levels, each clamped to `0.0..=1.0` (NaN counts as `0.0`).
    pub fn levels(&self) -> (f32, f32) {
        match *self {
            WeatherKind::Clear => (0.0, 0.0),
            WeatherKind::Rain { level } => (clamp_level(level), 0.0),
            WeatherKind::Thunder { rain, thunder } => (clamp_level(rain), clamp_level(thunder)),
        }
    }
}

fn clamp_level(level: f32) -> f32 {
    if level.is_nan() {
        0.0
    } else {
        level.clamp(0.0, 1.0)
    }
}

#[derive(Component, Clone, Debug)]
#[require(WeatherState)]
pub struct Weather {
    pub kind: WeatherKind,
    pub transition: u32,
    pub audience: Audience,
}

impl Default for Weather {
    fn default() -> Self {
        Self::clear()
    }
}

impl Weather {
    pub fn new(kind: WeatherKind) -> Self {
        Self {
            kind,
            transition: 0,
            audience: Audience::All,
        }
    }

    pub fn clear() -> Self {
        Self::new(WeatherKind::Clear)
    }

    pub fn rain() -> Self {
        Self::new(WeatherKind::Rain { level: 1.0 })
    }

    pub fn thunder() -> Self {
        Self::new(WeatherKind::Thunder {
            rain: 1.0,
            thunder: 1.0,
        })
    }

    pub fn kind(mut self, kind: WeatherKind) -> Self {
        self.kind = kind;
        self
    }

    /// Ticks over which rain and thunder levels move to their target; `0`
    /// (the default) jumps immediately. Vanilla fades over 100 ticks.
    pub fn transition(mut self, ticks: u32) -> Self {
        self.transition = ticks;
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    pub fn set(&mut self, kind: WeatherKind) {
        self.kind = kind;
    }

    pub fn set_clear(&mut self) {
        self.set(WeatherKind::Clear);
    }

    pub fn set_rain(&mut self) {
        self.set(WeatherKind::Rain { level: 1.0 });
    }

    pub fn set_thunder(&mut self) {
        self.set(WeatherKind::Thunder {
            rain: 1.0,
            thunder: 1.0,
        });
    }

    pub fn is_raining(&self) -> bool {
        self.kind.raining()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Levels {
    raining: bool,
    rain: f32,
    thunder: f32,
}

impl Levels {
    fn of(kind: WeatherKind) -> Self {
        let (rain, thunder) = kind.levels();
        Self {
            raining: kind.raining(),
            rain,
            thunder,
        }
    }

    fn towards(self, target: Self, elapsed: u32, transition: u32) -> Self {
        if elapsed >= transition {
            return target;
        }
        let t = elapsed as f32 / transition as f32;
        Self {
            raining: target.raining,
            rain: self.rain + (target.rain - self.rain) * t,
            thunder: self.thunder + (target.thunder - self.thunder) * t,
        }
    }

    /// Everything a client needs regardless of what it currently shows, so a
    /// newly covered viewer ends up on this state whatever synced before.
    fn full(self) -> Vec<GameEvent> {
        self.events(true)
    }

    fn diff_to(self, next: Self) -> Vec<GameEvent> {
        next.events(next.raining != self.raining)
            .into_iter()
            .filter(|event| match event.event {
                GameEventType::RainLevelChange => {
                    next.rain != self.rain || next.raining != self.raining
                }
                GameEventType::ThunderLevelChange => {
                    next.thunder != self.thunder || next.raining != self.raining
                }
                _ => true,
            })
            .collect()
    }

    fn events(self, flag: bool) -> Vec<GameEvent> {
        let mut events = Vec::with_capacity(3);
        if flag {
            let event = if self.raining {
                GameEventType::BeginRaining
            } else {
                GameEventType::EndRaining
            };
            events.push(GameEvent { event, value: 0.0 });
        }
        events.push(GameEvent {
            event: GameEventType::RainLevelChange,
            value: self.rain,
        });
        events.push(GameEvent {
            event: GameEventType::ThunderLevelChange,
            value: self.thunder,
        });
        events
    }

    fn reset(self) -> Vec<GameEvent> {
        self.diff_to(Self::default())
    }
}

#[derive(Component, Debug, Default)]
pub struct WeatherState {
    sent: Option<Levels>,
    target: Levels,
    from: Levels,
    elapsed: u32,
    viewers: HashSet<Entity>,
}

impl WeatherState {
    fn next(&mut self, weather: &Weather) -> Levels {
        let current = self.sent.unwrap_or_default();
        let target = Levels::of(weather.kind);
        if self.sent.is_none() || target != self.target {
            self.target = target;
            self.from = current;
            self.elapsed = 0;
        }
        self.elapsed = self.elapsed.saturating_add(1).min(weather.transition);
        self.from.towards(target, self.elapsed, weather.transition)
    }

    pub fn viewers(&self) -> impl Iterator<Item = Entity> + '_ {
        self.viewers.iter().copied()
    }

    /// Current on-wire `(rain, thunder)` levels, mid-transition included.
    pub fn levels(&self) -> (f32, f32) {
        self.sent
            .map(|levels| (levels.rain, levels.thunder))
            .unwrap_or_default()
    }
}

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(release_on_remove)
            .add_systems(PostUpdate, sync_weather.in_set(VoidSystems::WeatherSync));
    }
}

fn sync_weather(players: Players, mut weathers: Query<(&Weather, &mut WeatherState)>) {
    let ready = players.ready();
    let claimed = claimed(
        &ready,
        weathers.iter().map(|(weather, _)| &weather.audience),
    );
    for explicit in [true, false] {
        let mut released = Vec::new();
        let pass = weathers
            .iter_mut()
            .filter(|(weather, _)| matches!(weather.audience, Audience::Explicit(_)) == explicit);
        for (weather, mut state) in pass {
            let current = state.sent.unwrap_or_default();
            let next = state.next(weather);
            let desired = desired_viewers(&ready, &weather.audience, &state.viewers, &claimed);

            if let Some(desired) = &desired {
                let reset = current.reset();
                for gone in state.viewers.difference(desired) {
                    if !explicit && claimed.contains(gone) {
                        continue;
                    }
                    for event in &reset {
                        players.send(*gone, event.clone());
                    }
                    released.push(*gone);
                }
            }

            let events = current.diff_to(next);
            if !events.is_empty() {
                let kept = state
                    .viewers
                    .iter()
                    .copied()
                    .filter(|viewer| desired.as_ref().is_none_or(|d| d.contains(viewer)));
                let kept: Vec<Entity> = kept.collect();
                for event in events {
                    players.send_to(kept.iter().copied(), event);
                }
            }
            state.sent = Some(next);

            if let Some(desired) = desired {
                let full = next.full();
                for joined in desired.difference(&state.viewers) {
                    for event in &full {
                        players.send(*joined, event.clone());
                    }
                }
                state.viewers = desired;
            }
        }
        let peers = weathers
            .iter_mut()
            .filter(|(weather, _)| matches!(weather.audience, Audience::Explicit(_)) == explicit)
            .map(|(_, state)| state);
        forget(&released, peers);
    }
}

fn forget<'a>(released: &[Entity], states: impl Iterator<Item = Mut<'a, WeatherState>>) {
    if released.is_empty() {
        return;
    }
    for mut state in states {
        if state.viewers.is_empty() {
            continue;
        }
        for viewer in released {
            state.viewers.remove(viewer);
        }
    }
}

fn release_on_remove(
    event: On<Remove, Weather>,
    players: Players,
    mut states: Query<(Entity, &mut WeatherState)>,
) {
    let released = {
        let Ok((_, mut state)) = states.get_mut(event.entity) else {
            return;
        };
        let reset = state.sent.unwrap_or_default().reset();
        for viewer in state.viewers.iter() {
            for event in &reset {
                players.send(*viewer, event.clone());
            }
        }
        state.sent = None;
        state.viewers.drain().collect::<Vec<Entity>>()
    };
    forget(
        &released,
        states
            .iter_mut()
            .filter(|(entity, _)| *entity != event.entity)
            .map(|(_, state)| state),
    );
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, PlayerDimension, PlayerReady};
    use crate::network::{NetworkChannels, OutgoingPacket};
    use crate::world::DimensionId;

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_non_send_resource(incoming_tx)
        .add_plugins(WeatherPlugin);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut().spawn((ClientId(id), PlayerReady)).id()
    }

    #[derive(Debug, PartialEq, Clone, Copy)]
    enum Sent {
        Begin(u32),
        End(u32),
        Rain(u32, f32),
        Thunder(u32, f32),
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        rx.try_iter()
            .map(|out| {
                let ClientboundPacket::Play(PlayPacket::GameEvent(event)) = out.packet else {
                    panic!("unexpected packet {:?}", out.packet);
                };
                match event.event {
                    GameEventType::BeginRaining => {
                        assert_eq!(event.value, 0.0);
                        Sent::Begin(out.client_id)
                    }
                    GameEventType::EndRaining => {
                        assert_eq!(event.value, 0.0);
                        Sent::End(out.client_id)
                    }
                    GameEventType::RainLevelChange => Sent::Rain(out.client_id, event.value),
                    GameEventType::ThunderLevelChange => Sent::Thunder(out.client_id, event.value),
                    other => panic!("unexpected game event {other:?}"),
                }
            })
            .collect()
    }

    fn full(client: u32, raining: bool, rain: f32, thunder: f32) -> Vec<Sent> {
        vec![
            if raining {
                Sent::Begin(client)
            } else {
                Sent::End(client)
            },
            Sent::Rain(client, rain),
            Sent::Thunder(client, thunder),
        ]
    }

    #[test]
    fn kinds_clamp_levels_and_builders_set_fields() {
        assert_eq!(WeatherKind::Clear.levels(), (0.0, 0.0));
        assert!(!WeatherKind::Clear.raining());
        assert_eq!(WeatherKind::Rain { level: 2.0 }.levels(), (1.0, 0.0));
        assert_eq!(WeatherKind::Rain { level: f32::NAN }.levels(), (0.0, 0.0));
        assert_eq!(
            WeatherKind::Thunder {
                rain: -1.0,
                thunder: 0.5
            }
            .levels(),
            (0.0, 0.5)
        );
        let weather = Weather::thunder().transition(100);
        assert!(weather.is_raining());
        assert_eq!(weather.kind.levels(), (1.0, 1.0));
        assert_eq!(weather.transition, 100);
        assert!(matches!(weather.audience, Audience::All));
        assert!(matches!(
            Weather::rain().viewers([Entity::PLACEHOLDER]).audience,
            Audience::Explicit(_)
        ));
        assert_eq!(Weather::default().kind, WeatherKind::Clear);
    }

    #[test]
    fn transition_of_n_ticks_sends_exactly_n_level_steps_ending_on_target() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let weather = app.world_mut().spawn(Weather::rain().transition(100)).id();
        let mut levels = Vec::new();
        for _ in 0..120 {
            app.update();
            levels.extend(drain(&rx).into_iter().filter_map(|sent| match sent {
                Sent::Rain(1, level) => Some(level),
                _ => None,
            }));
        }
        assert_eq!(levels.len(), 100);
        assert_eq!(*levels.last().unwrap(), 1.0);
        assert!(levels.windows(2).all(|w| w[0] < w[1]), "{levels:?}");
        assert_eq!(
            app.world().get::<WeatherState>(weather).unwrap().levels(),
            (1.0, 0.0)
        );

        app.world_mut()
            .get_mut::<Weather>(weather)
            .unwrap()
            .set_clear();
        let mut levels = Vec::new();
        for _ in 0..120 {
            app.update();
            levels.extend(drain(&rx).into_iter().filter_map(|sent| match sent {
                Sent::Rain(1, level) => Some(level),
                _ => None,
            }));
        }
        assert_eq!(levels.len(), 100);
        assert_eq!(*levels.last().unwrap(), 0.0);
        assert!(levels.windows(2).all(|w| w[0] > w[1]), "{levels:?}");
    }

    #[test]
    fn rain_sends_full_state_once_then_clear_sends_the_diff() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let weather = app.world_mut().spawn(Weather::rain()).id();

        app.update();
        assert_eq!(drain(&rx), full(1, true, 1.0, 0.0));
        app.update();
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut()
            .get_mut::<Weather>(weather)
            .unwrap()
            .set_clear();
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::End(1), Sent::Rain(1, 0.0), Sent::Thunder(1, 0.0)]
        );
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut()
            .get_mut::<Weather>(weather)
            .unwrap()
            .set_thunder();
        app.update();
        assert_eq!(drain(&rx), full(1, true, 1.0, 1.0));

        app.world_mut()
            .get_mut::<Weather>(weather)
            .unwrap()
            .set(WeatherKind::Rain { level: 0.5 });
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Rain(1, 0.5), Sent::Thunder(1, 0.0)]);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn clear_weather_still_sends_full_state_to_newcomers() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        app.world_mut().spawn(Weather::clear());
        app.update();
        assert_eq!(drain(&rx), full(1, false, 0.0, 0.0));
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn transition_ramps_levels_one_packet_per_tick() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let weather = app.world_mut().spawn(Weather::thunder().transition(4)).id();

        app.update();
        assert_eq!(drain(&rx), full(1, true, 0.25, 0.25));
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Rain(1, 0.5), Sent::Thunder(1, 0.5)]);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::Rain(1, 0.75), Sent::Thunder(1, 0.75)]
        );
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Rain(1, 1.0), Sent::Thunder(1, 1.0)]);
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world().get::<WeatherState>(weather).unwrap().levels(),
            (1.0, 1.0)
        );

        app.world_mut()
            .get_mut::<Weather>(weather)
            .unwrap()
            .set_clear();
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::End(1), Sent::Rain(1, 0.75), Sent::Thunder(1, 0.75)]
        );
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(drain(&rx).len(), 6);
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world().get::<WeatherState>(weather).unwrap().levels(),
            (0.0, 0.0)
        );
    }

    #[test]
    fn late_joiner_gets_full_state_mid_transition_and_leaver_gets_reset() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1);
        app.world_mut().spawn(Weather::rain().transition(2));
        app.update();
        drain(&rx);

        let second = player(&mut app, 2);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Rain(1, 1.0),
                Sent::Begin(2),
                Sent::Rain(2, 1.0),
                Sent::Thunder(2, 0.0)
            ]
        );

        app.world_mut().entity_mut(first).remove::<PlayerReady>();
        app.update();
        assert_eq!(drain(&rx), full(1, false, 0.0, 0.0));

        app.world_mut().entity_mut(second).remove::<PlayerReady>();
        app.update();
        assert_eq!(drain(&rx), full(2, false, 0.0, 0.0));
    }

    #[test]
    fn dimension_audience_follows_players() {
        let (mut app, rx) = test_app();
        let roaming = app
            .world_mut()
            .spawn((
                ClientId(1),
                PlayerReady,
                PlayerDimension(DimensionId::Nether),
            ))
            .id();
        app.world_mut().spawn((
            ClientId(2),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
        ));
        app.world_mut()
            .spawn(Weather::rain().audience(Audience::InDimension(DimensionId::Overworld)));
        app.update();
        assert_eq!(drain(&rx), full(2, true, 1.0, 0.0));

        app.world_mut()
            .get_mut::<PlayerDimension>(roaming)
            .unwrap()
            .0 = DimensionId::Overworld;
        app.update();
        assert_eq!(drain(&rx), full(1, true, 1.0, 0.0));

        app.world_mut()
            .get_mut::<PlayerDimension>(roaming)
            .unwrap()
            .0 = DimensionId::Nether;
        app.update();
        assert_eq!(drain(&rx), full(1, false, 0.0, 0.0));
    }

    #[test]
    fn removal_resets_viewers_and_hands_them_back_to_other_weather() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut().spawn(Weather::rain());
        app.update();
        drain(&rx);

        let personal = app
            .world_mut()
            .spawn(Weather::clear().viewers([first]))
            .id();
        app.update();
        assert_eq!(drain(&rx), full(1, false, 0.0, 0.0));
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().despawn(personal);
        assert!(drain(&rx).is_empty());
        app.update();
        assert_eq!(drain(&rx), full(1, true, 1.0, 0.0));
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn personal_clear_sky_is_not_clobbered_by_changing_world_weather() {
        let (mut app, rx) = test_app();
        let vip = player(&mut app, 1);
        player(&mut app, 2);
        let world = app.world_mut().spawn(Weather::clear()).id();
        app.update();
        drain(&rx);

        let personal = app.world_mut().spawn(Weather::clear().viewers([vip])).id();
        app.update();
        assert_eq!(drain(&rx), full(1, false, 0.0, 0.0));
        assert!(
            !app.world()
                .get::<WeatherState>(world)
                .unwrap()
                .viewers
                .contains(&vip)
        );

        app.world_mut()
            .get_mut::<Weather>(world)
            .unwrap()
            .set_thunder();
        app.world_mut()
            .get_mut::<Weather>(world)
            .unwrap()
            .transition = 10;
        let mut to_vip = Vec::new();
        let mut to_other = 0;
        for _ in 0..40 {
            app.update();
            for sent in drain(&rx) {
                match sent {
                    Sent::Begin(1) | Sent::End(1) | Sent::Rain(1, _) | Sent::Thunder(1, _) => {
                        to_vip.push(sent)
                    }
                    _ => to_other += 1,
                }
            }
        }
        assert!(to_vip.is_empty(), "{to_vip:?}");
        assert_eq!(to_other, 21);

        app.world_mut().despawn(personal);
        assert!(drain(&rx).is_empty());
        app.update();
        assert_eq!(drain(&rx), full(1, true, 1.0, 1.0));
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn removing_rain_sends_reset_immediately() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let weather = app.world_mut().spawn(Weather::thunder()).id();
        app.update();
        drain(&rx);
        app.world_mut().entity_mut(weather).remove::<Weather>();
        assert_eq!(
            drain(&rx),
            vec![Sent::End(1), Sent::Rain(1, 0.0), Sent::Thunder(1, 0.0)]
        );
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world()
                .get::<WeatherState>(weather)
                .unwrap()
                .viewers()
                .count(),
            0
        );
    }
}
