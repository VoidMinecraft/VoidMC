//! World borders as entities: spawn a [`WorldBorder`], mutate it, despawn it.
//! Its [`Audience`] picks the viewers; a `PostUpdate` system diffs the border
//! against what viewers last received and sends only the matching packet.
//! The border is purely visual: the engine never pushes players back or
//! damages them, so gameplay rules go in your own systems.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::{
    ClientboundPacket, InitializeBorder, PlayPacket, SetBorderCenter, SetBorderLerpSize,
    SetBorderSize, SetBorderWarningDelay, SetBorderWarningDistance,
};

use crate::players::{Audience, Players};
use crate::schedule::VoidSystems;

pub const DEFAULT_DIAMETER: f64 = 59_999_968.0;
pub const DEFAULT_PORTAL_TELEPORT_BOUNDARY: u32 = 29_999_984;
pub const MAX_CENTER_COORDINATE: f64 = 29_999_984.0;
pub const DEFAULT_WARNING_BLOCKS: u32 = 5;
pub const DEFAULT_WARNING_TIME: Duration = Duration::from_secs(15);

const TICK: Duration = Duration::from_millis(50);

#[derive(Component, Clone, Debug)]
#[require(WorldBorderState)]
pub struct WorldBorder {
    pub center_x: f64,
    pub center_z: f64,
    pub warning_blocks: u32,
    pub warning_time: Duration,
    pub portal_teleport_boundary: u32,
    pub audience: Audience,
    size: Size,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Size {
    Fixed(f64),
    Lerp {
        from: f64,
        to: f64,
        started: Instant,
        duration: Duration,
    },
}

impl Size {
    fn at(&self, now: Instant) -> f64 {
        match *self {
            Size::Fixed(diameter) => diameter,
            Size::Lerp {
                from,
                to,
                started,
                duration,
            } => {
                let elapsed = now.saturating_duration_since(started);
                if elapsed >= duration {
                    to
                } else {
                    from + (to - from) * (elapsed.as_secs_f64() / duration.as_secs_f64())
                }
            }
        }
    }

    fn target(&self) -> f64 {
        match *self {
            Size::Fixed(diameter) | Size::Lerp { to: diameter, .. } => diameter,
        }
    }

    fn remaining(&self, now: Instant) -> Duration {
        match *self {
            Size::Fixed(_) => Duration::ZERO,
            Size::Lerp {
                started, duration, ..
            } => duration.saturating_sub(now.saturating_duration_since(started)),
        }
    }
}

impl Default for WorldBorder {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldBorder {
    pub fn new() -> Self {
        Self {
            center_x: 0.0,
            center_z: 0.0,
            warning_blocks: DEFAULT_WARNING_BLOCKS,
            warning_time: DEFAULT_WARNING_TIME,
            portal_teleport_boundary: DEFAULT_PORTAL_TELEPORT_BOUNDARY,
            audience: Audience::All,
            size: Size::Fixed(DEFAULT_DIAMETER),
        }
    }

    pub fn center(mut self, x: f64, z: f64) -> Self {
        self.set_center(x, z);
        self
    }

    pub fn diameter(mut self, diameter: f64) -> Self {
        self.set_diameter(diameter);
        self
    }

    pub fn warning_blocks(mut self, blocks: u32) -> Self {
        self.warning_blocks = blocks;
        self
    }

    pub fn warning_time(mut self, time: Duration) -> Self {
        self.warning_time = time;
        self
    }

    pub fn portal_teleport_boundary(mut self, boundary: u32) -> Self {
        self.portal_teleport_boundary = boundary;
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    pub fn set_center(&mut self, x: f64, z: f64) {
        self.center_x = sanitize_center(x);
        self.center_z = sanitize_center(z);
    }

    /// Jumps to `diameter` immediately, cancelling any running transition.
    pub fn set_diameter(&mut self, diameter: f64) {
        self.size = Size::Fixed(sanitize(diameter));
    }

    /// Animates from the current diameter to `diameter` over `duration`;
    /// growing works the same way. Viewers joining mid-way receive the
    /// remaining time.
    pub fn shrink_to(&mut self, diameter: f64, duration: Duration) {
        let to = sanitize(diameter);
        if duration.is_zero() {
            self.size = Size::Fixed(to);
            return;
        }
        let now = Instant::now();
        let from = self.size.at(now);
        if from == to {
            self.size = Size::Fixed(to);
            return;
        }
        self.size = Size::Lerp {
            from,
            to,
            started: now,
            duration,
        };
    }

    /// The diameter as viewers see it right now, interpolated during a transition.
    pub fn current_diameter(&self) -> f64 {
        self.size.at(Instant::now())
    }

    pub fn target_diameter(&self) -> f64 {
        self.size.target()
    }

    pub fn is_transitioning(&self) -> bool {
        !self.size.remaining(Instant::now()).is_zero()
    }

    fn snapshot(&self) -> Snapshot {
        let (center_x, center_z) = self.sanitized_center();
        Snapshot {
            center_x,
            center_z,
            size: self.size,
            warning_blocks: self.warning_blocks,
            warning_time: self.warning_time,
            portal_teleport_boundary: self.portal_teleport_boundary,
        }
    }

    fn sanitized_center(&self) -> (f64, f64) {
        (
            sanitize_center(self.center_x),
            sanitize_center(self.center_z),
        )
    }

    fn initialize(&self, now: Instant) -> ClientboundPacket {
        let (center_x, center_z) = self.sanitized_center();
        PlayPacket::InitializeBorder(InitializeBorder {
            center_x,
            center_z,
            old_diameter: self.size.at(now),
            new_diameter: self.size.target(),
            lerp_ticks: ticks_i64(self.size.remaining(now)),
            portal_teleport_boundary: varint_u32(self.portal_teleport_boundary),
            warning_blocks: varint_u32(self.warning_blocks),
            warning_ticks: varint_u32(ticks(self.warning_time)),
        })
        .into()
    }

    fn updates_since(&self, sent: &Snapshot, now: Instant) -> Vec<ClientboundPacket> {
        if self.portal_teleport_boundary != sent.portal_teleport_boundary {
            return vec![self.initialize(now)];
        }
        let mut packets = Vec::new();
        let (center_x, center_z) = self.sanitized_center();
        if center_x != sent.center_x || center_z != sent.center_z {
            packets
                .push(PlayPacket::SetBorderCenter(SetBorderCenter { center_x, center_z }).into());
        }
        if self.size != sent.size {
            packets.push(match self.size {
                Size::Fixed(diameter) => {
                    PlayPacket::SetBorderSize(SetBorderSize { diameter }).into()
                }
                Size::Lerp { from, to, .. } => PlayPacket::SetBorderLerpSize(SetBorderLerpSize {
                    old_diameter: from,
                    new_diameter: to,
                    lerp_ticks: ticks_i64(self.size.remaining(now)),
                })
                .into(),
            });
        }
        if self.warning_blocks != sent.warning_blocks {
            packets.push(
                PlayPacket::SetBorderWarningDistance(SetBorderWarningDistance {
                    warning_blocks: varint_u32(self.warning_blocks),
                })
                .into(),
            );
        }
        if self.warning_time != sent.warning_time {
            packets.push(
                PlayPacket::SetBorderWarningDelay(SetBorderWarningDelay {
                    warning_ticks: varint_u32(ticks(self.warning_time)),
                })
                .into(),
            );
        }
        packets
    }
}

fn sanitize(diameter: f64) -> f64 {
    if diameter.is_nan() {
        DEFAULT_DIAMETER
    } else {
        diameter.clamp(1.0, DEFAULT_DIAMETER)
    }
}

fn sanitize_center(coordinate: f64) -> f64 {
    if coordinate.is_nan() {
        0.0
    } else {
        coordinate.clamp(-MAX_CENTER_COORDINATE, MAX_CENTER_COORDINATE)
    }
}

fn ticks_u128(duration: Duration) -> u128 {
    duration.as_millis().div_ceil(TICK.as_millis())
}

fn ticks(duration: Duration) -> u32 {
    ticks_u128(duration).min(u32::MAX as u128) as u32
}

fn ticks_i64(duration: Duration) -> i64 {
    ticks_u128(duration).min(i64::MAX as u128) as i64
}

fn varint_u32(value: u32) -> i32 {
    value.min(i32::MAX as u32) as i32
}

fn reset_packet() -> ClientboundPacket {
    WorldBorder::new().initialize(Instant::now())
}

#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    center_x: f64,
    center_z: f64,
    size: Size,
    warning_blocks: u32,
    warning_time: Duration,
    portal_teleport_boundary: u32,
}

#[derive(Component, Debug, Default)]
pub struct WorldBorderState {
    sent: Option<Snapshot>,
    viewers: HashSet<Entity>,
}

impl WorldBorderState {
    pub fn viewers(&self) -> impl Iterator<Item = Entity> + '_ {
        self.viewers.iter().copied()
    }
}

pub struct WorldBorderPlugin;

impl Plugin for WorldBorderPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(reset_removed_border).add_systems(
            PostUpdate,
            sync_world_borders.in_set(VoidSystems::WorldBorderSync),
        );
    }
}

fn sync_world_borders(players: Players, mut borders: Query<(&WorldBorder, &mut WorldBorderState)>) {
    if borders.is_empty() {
        return;
    }
    let ready = players.ready();
    let now = Instant::now();
    for (border, mut state) in borders.iter_mut() {
        let members = || {
            ready
                .iter()
                .filter(|r| border.audience.includes(r))
                .map(|r| r.entity())
        };
        let mut count = 0;
        let same_members = members().all(|entity| {
            count += 1;
            state.viewers.contains(&entity)
        }) && count == state.viewers.len();
        let desired: Option<HashSet<Entity>> = (!same_members).then(|| members().collect());

        if let Some(desired) = &desired {
            for gone in state.viewers.difference(desired) {
                players.send(*gone, reset_packet());
            }
        }

        if let Some(sent) = &state.sent {
            let packets = border.updates_since(sent, now);
            if !packets.is_empty() {
                let kept: Vec<Entity> = state
                    .viewers
                    .iter()
                    .copied()
                    .filter(|viewer| desired.as_ref().is_none_or(|d| d.contains(viewer)))
                    .collect();
                for packet in packets {
                    players.send_to(kept.iter().copied(), packet);
                }
                state.sent = Some(border.snapshot());
            }
        } else {
            state.sent = Some(border.snapshot());
        }

        if let Some(desired) = desired {
            for joined in desired.difference(&state.viewers) {
                players.send(*joined, border.initialize(now));
            }
            state.viewers = desired;
        }
    }
}

fn reset_removed_border(
    event: On<Remove, WorldBorder>,
    players: Players,
    mut borders: Query<&mut WorldBorderState>,
) {
    let Ok(mut state) = borders.get_mut(event.entity) else {
        return;
    };
    for viewer in state.viewers.iter() {
        players.send(*viewer, reset_packet());
    }
    state.viewers.clear();
    state.sent = None;
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;

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
        .add_plugins(WorldBorderPlugin);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut().spawn((ClientId(id), PlayerReady)).id()
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<(u32, PlayPacket)> {
        let mut sent: Vec<(u32, PlayPacket)> = rx
            .try_iter()
            .map(|out| {
                let ClientboundPacket::Play(packet) = out.packet else {
                    panic!("unexpected packet {:?}", out.packet);
                };
                (out.client_id, packet)
            })
            .collect();
        sent.sort_by_key(|(client, _)| *client);
        sent
    }

    fn only(sent: Vec<(u32, PlayPacket)>, client: u32) -> PlayPacket {
        let [(to, packet)] = sent.try_into().expect("exactly one packet");
        assert_eq!(to, client);
        packet
    }

    fn initialize(packet: &PlayPacket) -> &InitializeBorder {
        match packet {
            PlayPacket::InitializeBorder(init) => init,
            other => panic!("expected InitializeBorder, got {other:?}"),
        }
    }

    fn default_initialize() -> InitializeBorder {
        InitializeBorder {
            center_x: 0.0,
            center_z: 0.0,
            old_diameter: DEFAULT_DIAMETER,
            new_diameter: DEFAULT_DIAMETER,
            lerp_ticks: 0,
            portal_teleport_boundary: 29_999_984,
            warning_blocks: 5,
            warning_ticks: 300,
        }
    }

    fn demo_border() -> WorldBorder {
        WorldBorder::new()
            .center(100.0, -50.0)
            .diameter(512.0)
            .warning_blocks(8)
            .warning_time(Duration::from_secs(10))
    }

    #[test]
    fn builder_produces_initialize_packet_in_ticks() {
        let border = demo_border();
        let init = border.initialize(Instant::now());
        let ClientboundPacket::Play(PlayPacket::InitializeBorder(init)) = init else {
            panic!("expected initialize");
        };
        assert_eq!(
            init,
            InitializeBorder {
                center_x: 100.0,
                center_z: -50.0,
                old_diameter: 512.0,
                new_diameter: 512.0,
                lerp_ticks: 0,
                portal_teleport_boundary: 29_999_984,
                warning_blocks: 8,
                warning_ticks: 200,
            }
        );
        assert!(matches!(WorldBorder::new().audience, Audience::All));
        assert!(matches!(
            WorldBorder::new().viewers([Entity::PLACEHOLDER]).audience,
            Audience::Explicit(_)
        ));
    }

    #[test]
    fn diameter_is_clamped_to_vanilla_range() {
        assert_eq!(WorldBorder::new().diameter(0.0).target_diameter(), 1.0);
        assert_eq!(WorldBorder::new().diameter(-5.0).target_diameter(), 1.0);
        assert_eq!(
            WorldBorder::new().diameter(f64::NAN).target_diameter(),
            DEFAULT_DIAMETER
        );
        assert_eq!(
            WorldBorder::new().diameter(1e9).target_diameter(),
            DEFAULT_DIAMETER
        );
    }

    #[test]
    fn center_is_sanitized_and_a_nan_center_stays_quiet() {
        let border = WorldBorder::new().center(f64::NAN, 1e9);
        assert_eq!(border.center_x, 0.0);
        assert_eq!(border.center_z, MAX_CENTER_COORDINATE);
        assert_eq!(
            WorldBorder::new().center(-1e9, f64::NEG_INFINITY).center_x,
            -MAX_CENTER_COORDINATE
        );

        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let border = app.world_mut().spawn(demo_border()).id();
        app.update();
        drain(&rx);

        {
            let mut border = app.world_mut().get_mut::<WorldBorder>(border).unwrap();
            border.center_x = f64::NAN;
            border.center_z = 40_000_000.0;
        }
        app.update();
        let PlayPacket::SetBorderCenter(center) = only(drain(&rx), 1) else {
            panic!("expected SetBorderCenter");
        };
        assert_eq!(
            center,
            SetBorderCenter {
                center_x: 0.0,
                center_z: MAX_CENTER_COORDINATE
            }
        );
        app.update();
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn durations_round_up_to_the_next_tick() {
        assert_eq!(ticks(Duration::ZERO), 0);
        assert_eq!(ticks(Duration::from_millis(49)), 1);
        assert_eq!(ticks(Duration::from_millis(50)), 1);
        assert_eq!(ticks(Duration::from_millis(51)), 2);
        assert_eq!(ticks_i64(Duration::from_millis(119_999)), 2400);
        assert_eq!(ticks_i64(Duration::from_secs(120)), 2400);
    }

    #[test]
    fn shrink_to_the_current_diameter_is_a_fixed_size() {
        let mut border = demo_border();
        border.shrink_to(512.0, Duration::from_secs(120));
        assert!(!border.is_transitioning());
        assert_eq!(border.size, Size::Fixed(512.0));
        assert_eq!(border.target_diameter(), 512.0);

        border.shrink_to(64.0, Duration::from_secs(120));
        assert!(border.is_transitioning());
    }

    #[test]
    fn lerp_interpolates_linearly_and_settles_on_target() {
        let started = Instant::now();
        let size = Size::Lerp {
            from: 512.0,
            to: 64.0,
            started,
            duration: Duration::from_secs(120),
        };
        assert_eq!(size.at(started), 512.0);
        assert_eq!(size.at(started + Duration::from_secs(60)), 288.0);
        assert_eq!(size.at(started + Duration::from_secs(120)), 64.0);
        assert_eq!(size.at(started + Duration::from_secs(500)), 64.0);
        assert_eq!(
            size.remaining(started + Duration::from_secs(90)),
            Duration::from_secs(30)
        );
        assert_eq!(
            size.remaining(started + Duration::from_secs(500)),
            Duration::ZERO
        );
    }

    #[test]
    fn sends_full_state_on_first_sight_then_stays_quiet() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut().spawn(demo_border());

        app.update();
        let sent = drain(&rx);
        assert_eq!(sent.len(), 2);
        assert_eq!(initialize(&sent[0].1).old_diameter, 512.0);
        assert_eq!(initialize(&sent[1].1).warning_blocks, 8);

        app.update();
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn sends_only_the_packet_for_the_changed_field() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let border = app.world_mut().spawn(demo_border()).id();
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<WorldBorder>(border)
            .unwrap()
            .set_center(1.5, 2.5);
        app.update();
        let PlayPacket::SetBorderCenter(center) = only(drain(&rx), 1) else {
            panic!("expected SetBorderCenter");
        };
        assert_eq!(
            center,
            SetBorderCenter {
                center_x: 1.5,
                center_z: 2.5
            }
        );

        app.world_mut()
            .get_mut::<WorldBorder>(border)
            .unwrap()
            .warning_blocks = 3;
        app.update();
        let PlayPacket::SetBorderWarningDistance(distance) = only(drain(&rx), 1) else {
            panic!("expected SetBorderWarningDistance");
        };
        assert_eq!(distance, SetBorderWarningDistance { warning_blocks: 3 });

        app.world_mut()
            .get_mut::<WorldBorder>(border)
            .unwrap()
            .warning_time = Duration::from_secs(2);
        app.update();
        let PlayPacket::SetBorderWarningDelay(delay) = only(drain(&rx), 1) else {
            panic!("expected SetBorderWarningDelay");
        };
        assert_eq!(delay, SetBorderWarningDelay { warning_ticks: 40 });

        app.world_mut()
            .get_mut::<WorldBorder>(border)
            .unwrap()
            .set_diameter(256.0);
        app.update();
        let PlayPacket::SetBorderSize(size) = only(drain(&rx), 1) else {
            panic!("expected SetBorderSize");
        };
        assert_eq!(size, SetBorderSize { diameter: 256.0 });

        app.world_mut()
            .get_mut::<WorldBorder>(border)
            .unwrap()
            .shrink_to(64.0, Duration::from_secs(120));
        app.update();
        let PlayPacket::SetBorderLerpSize(lerp) = only(drain(&rx), 1) else {
            panic!("expected SetBorderLerpSize");
        };
        assert_eq!(lerp.old_diameter, 256.0);
        assert_eq!(lerp.new_diameter, 64.0);
        assert_eq!(lerp.lerp_ticks, 2400);

        app.update();
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut()
            .get_mut::<WorldBorder>(border)
            .unwrap()
            .portal_teleport_boundary = 1000;
        app.update();
        let sent = drain(&rx);
        assert_eq!(sent.len(), 1);
        let init = initialize(&sent[0].1);
        assert_eq!(init.portal_teleport_boundary, 1000);
        assert_eq!(init.new_diameter, 64.0);
        assert!(init.lerp_ticks > 2300);
    }

    #[test]
    fn late_joiner_receives_the_remaining_lerp() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let border = app.world_mut().spawn(demo_border()).id();
        app.update();
        drain(&rx);

        {
            let mut border = app.world_mut().get_mut::<WorldBorder>(border).unwrap();
            border.shrink_to(64.0, Duration::from_secs(120));
            if let Size::Lerp { started, .. } = &mut border.size {
                *started -= Duration::from_secs(60);
            }
        }
        player(&mut app, 2);
        app.update();
        let sent = drain(&rx);
        assert_eq!(sent.len(), 2);
        assert!(matches!(sent[0].1, PlayPacket::SetBorderLerpSize(_)));
        let init = initialize(&sent[1].1);
        assert_eq!(init.new_diameter, 64.0);
        assert!(
            (287.0..=289.0).contains(&init.old_diameter),
            "{}",
            init.old_diameter
        );
        assert_eq!(init.lerp_ticks, 1200);
        assert_eq!(init.warning_blocks, 8);

        let border = app.world().get::<WorldBorder>(border).unwrap();
        assert!(border.is_transitioning());
        assert_eq!(border.target_diameter(), 64.0);
    }

    #[test]
    fn finished_lerp_is_sent_as_a_fixed_size_to_new_viewers() {
        let (mut app, rx) = test_app();
        let border = app.world_mut().spawn(demo_border()).id();
        {
            let mut border = app.world_mut().get_mut::<WorldBorder>(border).unwrap();
            border.shrink_to(64.0, Duration::from_secs(1));
            if let Size::Lerp { started, .. } = &mut border.size {
                *started -= Duration::from_secs(5);
            }
        }
        player(&mut app, 1);
        app.update();
        let sent = drain(&rx);
        let init = initialize(&sent[0].1);
        assert_eq!(init.old_diameter, 64.0);
        assert_eq!(init.new_diameter, 64.0);
        assert_eq!(init.lerp_ticks, 0);
        assert!(
            !app.world()
                .get::<WorldBorder>(border)
                .unwrap()
                .is_transitioning()
        );
    }

    #[test]
    fn removal_resets_viewers_to_the_vanilla_border() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        player(&mut app, 2);
        let despawned = app.world_mut().spawn(demo_border()).id();
        let stripped = app.world_mut().spawn(demo_border().viewers([])).id();
        app.update();
        drain(&rx);

        app.world_mut().despawn(despawned);
        let sent = drain(&rx);
        assert_eq!(sent.len(), 2);
        assert_eq!(initialize(&sent[0].1), &default_initialize());
        assert_eq!(initialize(&sent[1].1), &default_initialize());
        assert_eq!(sent[0].0, 1);
        assert_eq!(sent[1].0, 2);

        app.world_mut().entity_mut(stripped).remove::<WorldBorder>();
        assert!(drain(&rx).is_empty());
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn audience_changes_initialize_newcomers_and_reset_leavers() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1);
        let second = player(&mut app, 2);
        let nether = app
            .world_mut()
            .spawn((
                ClientId(3),
                PlayerReady,
                PlayerDimension(DimensionId::Nether),
            ))
            .id();
        let border = app.world_mut().spawn(demo_border().viewers([first])).id();
        app.update();
        let sent = drain(&rx);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, 1);
        assert_eq!(initialize(&sent[0].1).old_diameter, 512.0);

        {
            let mut border = app.world_mut().get_mut::<WorldBorder>(border).unwrap();
            border.audience = Audience::explicit([second]);
            border.warning_blocks = 1;
        }
        app.update();
        let sent = drain(&rx);
        assert_eq!(sent.len(), 2);
        assert_eq!(initialize(&sent[0].1), &default_initialize());
        assert_eq!(initialize(&sent[1].1).warning_blocks, 1);
        assert_eq!(sent[1].0, 2);

        app.world_mut()
            .get_mut::<WorldBorder>(border)
            .unwrap()
            .audience = Audience::InDimension(DimensionId::Nether);
        app.update();
        let sent = drain(&rx);
        assert_eq!(sent.len(), 2);
        assert_eq!(initialize(&sent[0].1), &default_initialize());
        assert_eq!(sent[0].0, 2);
        assert_eq!(initialize(&sent[1].1).warning_blocks, 1);
        assert_eq!(sent[1].0, 3);

        app.world_mut().entity_mut(second).remove::<PlayerReady>();
        app.update();
        assert!(drain(&rx).is_empty());
        let state = app.world().get::<WorldBorderState>(border).unwrap();
        assert_eq!(state.viewers().collect::<Vec<_>>(), vec![nether]);
    }
}
