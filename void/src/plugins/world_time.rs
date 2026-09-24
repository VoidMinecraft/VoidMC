//! World time as an entity: spawn a [`WorldTime`], mutate it, despawn it. The
//! server clock advances `time_of_day` every tick unless frozen; the sync
//! system pushes a Set Time packet on explicit changes and, while the clock
//! runs, every [`TIME_SYNC_INTERVAL`] ticks (the client extrapolates between).

use std::collections::HashSet;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc_data::Version;
use voidmc_protocol::clientbound::{ClockUpdate, SetTime};

use super::viewers::{claimed, desired_viewers};
use crate::players::{Audience, Players};
use crate::schedule::VoidSystems;

pub const TICKS_PER_DAY: i64 = 24_000;
pub const TIME_SYNC_INTERVAL: u32 = 20;

const WORLD_CLOCK_REGISTRY: &str = "minecraft:world_clock";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WorldClock {
    #[default]
    Overworld,
    End,
}

impl WorldClock {
    pub fn name(self) -> &'static str {
        match self {
            WorldClock::Overworld => "minecraft:overworld",
            WorldClock::End => "minecraft:the_end",
        }
    }

    pub fn registry_id(self) -> i32 {
        voidmc_data::registry_index(Version::V26_1_2, WORLD_CLOCK_REGISTRY, self.name())
            .unwrap_or_else(|| panic!("world clock {} is not in the synced registry", self.name()))
    }
}

/// Ticks since the server started, sent as the game time of every Set Time
/// packet so all clocks a client sees share one monotonic timeline.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GameTime(pub i64);

#[derive(Component, Clone, Debug)]
#[require(WorldTimeState)]
pub struct WorldTime {
    pub time_of_day: i64,
    pub frozen: bool,
    pub clock: WorldClock,
    pub audience: Audience,
}

impl Default for WorldTime {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldTime {
    pub fn new() -> Self {
        Self {
            time_of_day: 0,
            frozen: false,
            clock: WorldClock::Overworld,
            audience: Audience::All,
        }
    }

    pub fn time_of_day(mut self, time_of_day: i64) -> Self {
        self.time_of_day = time_of_day;
        self
    }

    pub fn frozen(mut self) -> Self {
        self.frozen = true;
        self
    }

    pub fn clock(mut self, clock: WorldClock) -> Self {
        self.clock = clock;
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    pub fn set(&mut self, time_of_day: i64) {
        self.time_of_day = time_of_day;
    }

    pub fn add(&mut self, ticks: i64) {
        self.time_of_day = self.time_of_day.wrapping_add(ticks);
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn resume(&mut self) {
        self.frozen = false;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn day_time(&self) -> i64 {
        self.time_of_day.rem_euclid(TICKS_PER_DAY)
    }

    fn packet(&self, game_time: GameTime, running: bool) -> SetTime {
        SetTime {
            game_time: game_time.0,
            clocks: vec![ClockUpdate {
                clock: self.clock.registry_id(),
                total_ticks: self.time_of_day,
                partial_tick: 0.0,
                rate: if running { 1.0 } else { 0.0 },
            }],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Sent {
    frozen: bool,
    clock: WorldClock,
}

#[derive(Component, Debug, Default)]
pub struct WorldTimeState {
    sent: Option<Sent>,
    expected: i64,
    since_send: u32,
    viewers: HashSet<Entity>,
}

impl WorldTimeState {
    pub fn viewers(&self) -> impl Iterator<Item = Entity> + '_ {
        self.viewers.iter().copied()
    }
}

pub struct WorldTimePlugin;

impl Plugin for WorldTimePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameTime>()
            .add_observer(release_on_remove)
            .add_systems(
                PostUpdate,
                (sync_world_time, advance_game_time)
                    .chain()
                    .in_set(VoidSystems::WorldTimeSync),
            );
    }
}

fn sync_world_time(
    players: Players,
    game_time: Res<GameTime>,
    mut clocks: Query<(&mut WorldTime, &mut WorldTimeState)>,
) {
    let ready = players.ready();
    let game_time = *game_time;
    let claimed = claimed(&ready, clocks.iter().map(|(time, _)| &time.audience));
    for explicit in [true, false] {
        let mut released = Vec::new();
        let pass = clocks
            .iter_mut()
            .filter(|(time, _)| matches!(time.audience, Audience::Explicit(_)) == explicit);
        for (mut time, mut state) in pass {
            let changed = match &state.sent {
                Some(sent) => {
                    time.time_of_day != state.expected
                        || time.frozen != sent.frozen
                        || time.clock != sent.clock
                }
                None => true,
            };

            let desired = desired_viewers(&ready, &time.audience, &state.viewers, &claimed);
            if let Some(desired) = &desired {
                let was_frozen = state.sent.is_some_and(|sent| sent.frozen);
                for gone in state.viewers.difference(desired) {
                    if !explicit && claimed.contains(gone) {
                        continue;
                    }
                    if was_frozen {
                        players.send(*gone, time.packet(game_time, true));
                    }
                    released.push(*gone);
                }
            }

            if !time.frozen {
                state.since_send += 1;
            }
            let periodic = !time.frozen && state.since_send >= TIME_SYNC_INTERVAL;
            if changed || periodic {
                let targets = desired.as_ref().unwrap_or(&state.viewers);
                players.send_to(
                    targets.iter().copied(),
                    time.packet(game_time, !time.frozen),
                );
                state.sent = Some(Sent {
                    frozen: time.frozen,
                    clock: time.clock,
                });
                state.since_send = 0;
            } else if let Some(desired) = &desired {
                for joined in desired.difference(&state.viewers) {
                    players.send(*joined, time.packet(game_time, !time.frozen));
                }
            }

            if let Some(desired) = desired {
                state.viewers = desired;
            }

            if !time.frozen {
                time.bypass_change_detection().time_of_day = time.time_of_day.wrapping_add(1);
            }
            state.expected = time.time_of_day;
        }
        let peers = clocks
            .iter_mut()
            .filter(|(time, _)| matches!(time.audience, Audience::Explicit(_)) == explicit)
            .map(|(_, state)| state);
        forget(&released, peers);
    }
}

fn advance_game_time(mut game_time: ResMut<GameTime>) {
    game_time.0 = game_time.0.wrapping_add(1);
}

fn forget<'a>(released: &[Entity], states: impl Iterator<Item = Mut<'a, WorldTimeState>>) {
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
    event: On<Remove, WorldTime>,
    players: Players,
    game_time: Res<GameTime>,
    times: Query<&WorldTime>,
    mut states: Query<(Entity, &mut WorldTimeState)>,
) {
    let released = {
        let Ok((_, mut state)) = states.get_mut(event.entity) else {
            return;
        };
        if let (Some(sent), Ok(time)) = (state.sent, times.get(event.entity)) {
            if sent.frozen {
                for viewer in state.viewers.iter() {
                    players.send(*viewer, time.packet(*game_time, true));
                }
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
        .add_plugins(WorldTimePlugin);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut().spawn((ClientId(id), PlayerReady)).id()
    }

    #[derive(Debug, PartialEq, PartialOrd)]
    struct Sent {
        client: u32,
        age: i64,
        time: i64,
        rate: f32,
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent: Vec<Sent> = rx
            .try_iter()
            .map(|out| {
                let ClientboundPacket::Play(PlayPacket::SetTime(packet)) = out.packet else {
                    panic!("unexpected packet {:?}", out.packet);
                };
                assert_eq!(packet.clocks.len(), 1);
                let clock = packet.clocks[0];
                assert_eq!(clock.clock, WorldClock::Overworld.registry_id());
                assert_eq!(clock.partial_tick, 0.0);
                Sent {
                    client: out.client_id,
                    age: packet.game_time,
                    time: clock.total_ticks,
                    rate: clock.rate,
                }
            })
            .collect();
        sent.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sent
    }

    fn sent(client: u32, age: i64, time: i64, rate: f32) -> Sent {
        Sent {
            client,
            age,
            time,
            rate,
        }
    }

    #[test]
    fn builder_and_clock_ids() {
        let time = WorldTime::new().time_of_day(30_000).frozen();
        assert_eq!(time.time_of_day, 30_000);
        assert_eq!(time.day_time(), 6_000);
        assert!(time.is_frozen());
        assert!(matches!(time.audience, Audience::All));
        assert!(matches!(
            WorldTime::new().viewers([Entity::PLACEHOLDER]).audience,
            Audience::Explicit(_)
        ));
        assert_eq!(
            WorldTime::new().time_of_day(-1).day_time(),
            TICKS_PER_DAY - 1
        );
        assert_eq!(
            WorldClock::Overworld.registry_id(),
            voidmc_data::registry_index(
                Version::V26_1_2,
                WORLD_CLOCK_REGISTRY,
                "minecraft:overworld"
            )
            .unwrap()
        );
        assert_ne!(
            WorldClock::End.registry_id(),
            WorldClock::Overworld.registry_id()
        );
    }

    #[test]
    fn running_clock_advances_and_resends_every_twenty_ticks() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let clock = app
            .world_mut()
            .spawn(WorldTime::new().time_of_day(6000))
            .id();

        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 0, 6000, 1.0)]);
        for _ in 0..19 {
            app.update();
            assert!(drain(&rx).is_empty());
        }
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 20, 6020, 1.0)]);
        let time = app.world().get::<WorldTime>(clock).unwrap();
        assert_eq!(time.time_of_day, 6021);
        assert_eq!(app.world().resource::<GameTime>().0, 21);
    }

    #[derive(Resource, Default)]
    struct Changes(u32);

    #[test]
    fn natural_advance_does_not_trip_change_detection() {
        let (mut app, _rx) = test_app();
        app.init_resource::<Changes>().add_systems(
            bevy_app::Update,
            |changed: Query<(), Changed<WorldTime>>, mut changes: ResMut<Changes>| {
                changes.0 += changed.iter().count() as u32;
            },
        );
        let clock = app.world_mut().spawn(WorldTime::new()).id();
        app.update();
        assert_eq!(app.world().resource::<Changes>().0, 1);
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Changes>().0, 1);
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .freeze();
        app.update();
        assert_eq!(app.world().resource::<Changes>().0, 2);
    }

    #[test]
    fn explicit_set_sends_once_with_the_new_value() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let clock = app.world_mut().spawn(WorldTime::new()).id();
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .set(18000);
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 1, 18000, 1.0)]);
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .add(100);
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 3, 18102, 1.0)]);
    }

    #[test]
    fn freeze_sends_once_then_stays_quiet_until_resume() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let clock = app
            .world_mut()
            .spawn(WorldTime::new().time_of_day(100))
            .id();
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .freeze();
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 1, 101, 0.0)]);
        for _ in 0..50 {
            app.update();
        }
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world().get::<WorldTime>(clock).unwrap().time_of_day,
            101
        );
        assert_eq!(app.world().resource::<GameTime>().0, 52);

        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .set(200);
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 52, 200, 0.0)]);

        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .resume();
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 53, 200, 1.0)]);
        for _ in 0..19 {
            app.update();
        }
        assert!(drain(&rx).is_empty());
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 73, 220, 1.0)]);
    }

    #[test]
    fn frozen_clock_spawned_frozen_sends_once() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        app.world_mut()
            .spawn(WorldTime::new().time_of_day(18000).frozen());
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 0, 18000, 0.0)]);
        for _ in 0..40 {
            app.update();
        }
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn late_joiner_gets_full_state_and_leaver_of_frozen_clock_is_unfrozen() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1);
        let clock = app
            .world_mut()
            .spawn(WorldTime::new().time_of_day(500).frozen())
            .id();
        app.update();
        drain(&rx);

        let second = player(&mut app, 2);
        app.update();
        assert_eq!(drain(&rx), vec![sent(2, 1, 500, 0.0)]);

        app.world_mut().entity_mut(first).remove::<PlayerReady>();
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 2, 500, 1.0)]);

        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .resume();
        app.update();
        assert_eq!(drain(&rx), vec![sent(2, 3, 500, 1.0)]);
        app.world_mut().entity_mut(second).remove::<PlayerReady>();
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world()
                .get::<WorldTimeState>(clock)
                .unwrap()
                .viewers()
                .count(),
            0
        );
    }

    #[test]
    fn late_joiner_on_a_periodic_tick_is_sent_once() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        app.world_mut().spawn(WorldTime::new());
        app.update();
        drain(&rx);
        for _ in 0..19 {
            app.update();
        }
        player(&mut app, 2);
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 20, 20, 1.0), sent(2, 20, 20, 1.0)]);
    }

    #[test]
    fn dimension_audience_follows_players() {
        let (mut app, rx) = test_app();
        let nether = app
            .world_mut()
            .spawn((
                ClientId(1),
                PlayerReady,
                PlayerDimension(DimensionId::Nether),
            ))
            .id();
        app.world_mut()
            .spawn((ClientId(2), PlayerReady, PlayerDimension(DimensionId::End)));
        app.world_mut().spawn(
            WorldTime::new()
                .time_of_day(12000)
                .frozen()
                .clock(WorldClock::End)
                .audience(Audience::InDimension(DimensionId::End)),
        );
        app.update();
        let out: Vec<_> = rx.try_iter().collect();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].client_id, 2);
        let ClientboundPacket::Play(PlayPacket::SetTime(packet)) = &out[0].packet else {
            panic!("unexpected packet");
        };
        assert_eq!(packet.clocks[0].clock, WorldClock::End.registry_id());

        app.world_mut()
            .get_mut::<PlayerDimension>(nether)
            .unwrap()
            .0 = DimensionId::End;
        app.update();
        let out: Vec<_> = rx.try_iter().collect();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].client_id, 1);
    }

    #[test]
    fn removal_unfreezes_viewers_and_hands_them_back_to_other_clocks() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut()
            .spawn(WorldTime::new().time_of_day(1000).frozen());
        app.update();
        drain(&rx);

        let personal = app
            .world_mut()
            .spawn(
                WorldTime::new()
                    .time_of_day(18000)
                    .frozen()
                    .viewers([first]),
            )
            .id();
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 1, 18000, 0.0)]);
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().despawn(personal);
        assert_eq!(drain(&rx), vec![sent(1, 3, 18000, 1.0)]);
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 3, 1000, 0.0)]);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn personal_frozen_clock_is_not_clobbered_by_the_running_world_clock() {
        let (mut app, rx) = test_app();
        let vip = player(&mut app, 1);
        player(&mut app, 2);
        let world = app
            .world_mut()
            .spawn(WorldTime::new().time_of_day(6000))
            .id();
        app.update();
        drain(&rx);

        let personal = app
            .world_mut()
            .spawn(WorldTime::new().time_of_day(18000).frozen().viewers([vip]))
            .id();
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 1, 18000, 0.0)]);
        assert!(
            !app.world()
                .get::<WorldTimeState>(world)
                .unwrap()
                .viewers
                .contains(&vip)
        );

        let mut to_vip = Vec::new();
        for _ in 0..40 {
            app.update();
            to_vip.extend(drain(&rx).into_iter().filter(|s| s.client == 1));
        }
        assert!(to_vip.is_empty(), "{to_vip:?}");

        app.world_mut()
            .get_mut::<WorldTime>(world)
            .unwrap()
            .set(12000);
        app.update();
        assert_eq!(drain(&rx), vec![sent(2, 42, 12000, 1.0)]);

        app.world_mut().despawn(personal);
        assert_eq!(drain(&rx), vec![sent(1, 43, 18000, 1.0)]);
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 43, 12001, 1.0)]);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn override_spawned_before_the_world_clock_still_wins() {
        let (mut app, rx) = test_app();
        let vip = player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut()
            .spawn(WorldTime::new().time_of_day(18000).frozen().viewers([vip]));
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 0, 18000, 0.0)]);

        app.world_mut().spawn(WorldTime::new().time_of_day(6000));
        app.update();
        assert_eq!(drain(&rx), vec![sent(2, 1, 6000, 1.0)]);
        for _ in 0..40 {
            app.update();
            assert!(drain(&rx).iter().all(|s| s.client == 2));
        }
    }

    #[test]
    fn override_dropping_a_viewer_hands_them_back_after_its_reset() {
        let (mut app, rx) = test_app();
        let vip = player(&mut app, 1);
        app.world_mut()
            .spawn(WorldTime::new().time_of_day(6000).frozen());
        let personal = app
            .world_mut()
            .spawn(WorldTime::new().time_of_day(18000).frozen().viewers([vip]))
            .id();
        app.update();
        assert_eq!(drain(&rx), vec![sent(1, 0, 18000, 0.0)]);

        app.world_mut()
            .get_mut::<WorldTime>(personal)
            .unwrap()
            .audience = Audience::explicit([]);
        app.update();
        let out: Vec<_> = rx.try_iter().map(|out| out.packet).collect();
        assert_eq!(out.len(), 2);
        let ClientboundPacket::Play(PlayPacket::SetTime(first)) = &out[0] else {
            panic!("unexpected packet");
        };
        let ClientboundPacket::Play(PlayPacket::SetTime(second)) = &out[1] else {
            panic!("unexpected packet");
        };
        assert_eq!(
            (first.clocks[0].total_ticks, first.clocks[0].rate),
            (18000, 1.0)
        );
        assert_eq!(
            (second.clocks[0].total_ticks, second.clocks[0].rate),
            (6000, 0.0)
        );
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn frozen_clock_does_not_count_towards_the_periodic_resend() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let clock = app.world_mut().spawn(WorldTime::new()).id();
        app.update();
        drain(&rx);
        for _ in 0..10 {
            app.update();
        }
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .freeze();
        app.update();
        drain(&rx);
        assert_eq!(
            app.world().get::<WorldTimeState>(clock).unwrap().since_send,
            0
        );
        for _ in 0..100 {
            app.update();
        }
        assert_eq!(
            app.world().get::<WorldTimeState>(clock).unwrap().since_send,
            0
        );
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn removing_a_running_clock_sends_nothing() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let clock = app.world_mut().spawn(WorldTime::new()).id();
        app.update();
        drain(&rx);
        app.world_mut().entity_mut(clock).remove::<WorldTime>();
        app.update();
        assert!(drain(&rx).is_empty());
    }
}
