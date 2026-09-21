//! Boss bars as entities: spawn a [`BossBar`], mutate its fields, despawn it.
//! Its [`Audience`] picks the viewers (every ready player by default). A
//! `PostUpdate` system diffs each bar against what each viewer last received
//! and sends only the matching Boss Event actions.

use std::collections::HashSet;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use ussr_nbt::owned::{Nbt, Tag};
use uuid::Uuid;
use voidmc_protocol::clientbound::{BossEvent, BossEventAction};

pub use voidmc_protocol::clientbound::{BossBarColor, BossBarDivision, BossBarFlags};

use crate::players::{Audience, Players};
use crate::schedule::VoidSystems;

#[derive(Component, Clone, Debug)]
#[require(BossBarState)]
pub struct BossBar {
    pub title: String,
    pub progress: f32,
    pub color: BossBarColor,
    pub division: BossBarDivision,
    pub flags: BossBarFlags,
    pub audience: Audience,
}

impl BossBar {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            progress: 1.0,
            color: BossBarColor::default(),
            division: BossBarDivision::default(),
            flags: BossBarFlags::empty(),
            audience: Audience::All,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn progress(mut self, progress: f32) -> Self {
        self.set_progress(progress);
        self
    }

    pub fn color(mut self, color: BossBarColor) -> Self {
        self.color = color;
        self
    }

    pub fn division(mut self, division: BossBarDivision) -> Self {
        self.division = division;
        self
    }

    pub fn flags(mut self, flags: BossBarFlags) -> Self {
        self.flags = flags;
        self
    }

    pub fn darken_screen(self) -> Self {
        let flags = self.flags | BossBarFlags::DARKEN_SCREEN;
        self.flags(flags)
    }

    pub fn boss_music(self) -> Self {
        let flags = self.flags | BossBarFlags::BOSS_MUSIC;
        self.flags(flags)
    }

    pub fn world_fog(self) -> Self {
        let flags = self.flags | BossBarFlags::WORLD_FOG;
        self.flags(flags)
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    /// Clamps to `0.0..=1.0`; NaN becomes `0.0`. Assigning the field directly
    /// is fine too: the sync system applies the same clamp on the wire.
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = clamp_progress(progress);
    }

    fn title_nbt(&self) -> Nbt {
        Nbt {
            name: "".into(),
            compound: vec![("text".into(), Tag::String(self.title.as_str().into()))].into(),
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            title: self.title.clone(),
            progress: clamp_progress(self.progress),
            color: self.color,
            division: self.division,
            flags: self.flags,
        }
    }

    fn add(&self) -> BossEventAction {
        BossEventAction::Add {
            title: self.title_nbt(),
            progress: clamp_progress(self.progress),
            color: self.color,
            division: self.division,
            flags: self.flags,
        }
    }

    fn updates_since(&self, sent: &Snapshot) -> Vec<BossEventAction> {
        let mut actions = Vec::new();
        let progress = clamp_progress(self.progress);
        if progress != sent.progress {
            actions.push(BossEventAction::UpdateProgress(progress));
        }
        if self.title != sent.title {
            actions.push(BossEventAction::UpdateTitle(self.title_nbt()));
        }
        if self.color != sent.color || self.division != sent.division {
            actions.push(BossEventAction::UpdateStyle {
                color: self.color,
                division: self.division,
            });
        }
        if self.flags != sent.flags {
            actions.push(BossEventAction::UpdateFlags(self.flags));
        }
        actions
    }
}

fn clamp_progress(progress: f32) -> f32 {
    if progress.is_nan() {
        0.0
    } else {
        progress.clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone)]
struct Snapshot {
    title: String,
    progress: f32,
    color: BossBarColor,
    division: BossBarDivision,
    flags: BossBarFlags,
}

#[derive(Component, Debug)]
pub struct BossBarState {
    id: Uuid,
    sent: Option<Snapshot>,
    viewers: HashSet<Entity>,
}

impl Default for BossBarState {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            sent: None,
            viewers: HashSet::new(),
        }
    }
}

impl BossBarState {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn viewers(&self) -> impl Iterator<Item = Entity> + '_ {
        self.viewers.iter().copied()
    }

    fn event(&self, action: BossEventAction) -> BossEvent {
        BossEvent {
            id: self.id,
            action,
        }
    }
}

pub struct BossBarPlugin;

impl Plugin for BossBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(remove_from_viewers)
            .add_systems(PostUpdate, sync_boss_bars.in_set(VoidSystems::BossBarSync));
    }
}

fn sync_boss_bars(players: Players, mut bars: Query<(&BossBar, &mut BossBarState)>) {
    let ready = players.ready();
    for (bar, mut state) in bars.iter_mut() {
        let members = || {
            ready
                .iter()
                .filter(|r| bar.audience.includes(r))
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
                players.send(*gone, state.event(BossEventAction::Remove));
            }
        }

        if let Some(sent) = &state.sent {
            let actions = bar.updates_since(sent);
            if !actions.is_empty() {
                let kept = state
                    .viewers
                    .iter()
                    .copied()
                    .filter(|viewer| desired.as_ref().is_none_or(|d| d.contains(viewer)));
                let kept: Vec<Entity> = kept.collect();
                for action in actions {
                    players.send_to(kept.iter().copied(), state.event(action));
                }
                state.sent = Some(bar.snapshot());
            }
        } else {
            state.sent = Some(bar.snapshot());
        }

        if let Some(desired) = desired {
            for joined in desired.difference(&state.viewers) {
                players.send(*joined, state.event(bar.add()));
            }
            state.viewers = desired;
        }
    }
}

fn remove_from_viewers(
    event: On<Remove, BossBar>,
    players: Players,
    mut bars: Query<&mut BossBarState>,
) {
    let Ok(mut state) = bars.get_mut(event.entity) else {
        return;
    };
    for viewer in state.viewers.iter() {
        players.send(*viewer, state.event(BossEventAction::Remove));
    }
    state.viewers.clear();
    state.sent = None;
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, PlayerDimension, PlayerReady};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::world::DimensionId;

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx))
        .add_plugins(BossBarPlugin);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut().spawn((ClientId(id), PlayerReady)).id()
    }

    #[derive(Debug, PartialEq, PartialOrd)]
    enum Sent {
        Add(u32, f32),
        Remove(u32),
        Progress(u32, f32),
        Title(u32),
        Style(u32),
        Flags(u32),
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent: Vec<Sent> = rx
            .try_iter()
            .map(|out| {
                let ClientboundPacket::Play(PlayPacket::BossEvent(event)) = out.packet else {
                    panic!("unexpected packet {:?}", out.packet);
                };
                match event.action {
                    BossEventAction::Add { progress, .. } => Sent::Add(out.client_id, progress),
                    BossEventAction::Remove => Sent::Remove(out.client_id),
                    BossEventAction::UpdateProgress(p) => Sent::Progress(out.client_id, p),
                    BossEventAction::UpdateTitle(_) => Sent::Title(out.client_id),
                    BossEventAction::UpdateStyle { .. } => Sent::Style(out.client_id),
                    BossEventAction::UpdateFlags(_) => Sent::Flags(out.client_id),
                }
            })
            .collect();
        sent.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sent
    }

    #[test]
    fn builder_clamps_progress_and_accumulates_flags() {
        let bar = BossBar::new("Boss")
            .progress(1.5)
            .color(BossBarColor::Red)
            .division(BossBarDivision::Notches10)
            .darken_screen()
            .world_fog();
        assert_eq!(bar.progress, 1.0);
        assert_eq!(bar.color, BossBarColor::Red);
        assert_eq!(bar.division, BossBarDivision::Notches10);
        assert_eq!(
            bar.flags,
            BossBarFlags::DARKEN_SCREEN | BossBarFlags::WORLD_FOG
        );
        assert_eq!(BossBar::new("x").progress(-3.0).progress, 0.0);
        assert_eq!(BossBar::new("x").progress(f32::NAN).progress, 0.0);
        assert!(matches!(BossBar::new("x").audience, Audience::All));
        assert!(matches!(
            BossBar::new("x").viewers([Entity::PLACEHOLDER]).audience,
            Audience::Explicit(_)
        ));
    }

    #[test]
    fn adds_on_first_sight_then_stays_quiet() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut().spawn(BossBar::new("Boss"));

        app.update();
        assert_eq!(drain(&rx), vec![Sent::Add(1, 1.0), Sent::Add(2, 1.0)]);

        app.update();
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn sends_only_the_changed_aspects() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let bar = app.world_mut().spawn(BossBar::new("Boss")).id();
        app.update();
        drain(&rx);

        app.world_mut().get_mut::<BossBar>(bar).unwrap().progress = 0.5;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Progress(1, 0.5)]);

        {
            let mut bar = app.world_mut().get_mut::<BossBar>(bar).unwrap();
            bar.title = "Phase 2".into();
            bar.color = BossBarColor::Red;
            bar.flags = BossBarFlags::BOSS_MUSIC;
        }
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::Title(1), Sent::Style(1), Sent::Flags(1)]
        );

        app.world_mut().get_mut::<BossBar>(bar).unwrap().progress = 0.5;
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn out_of_range_progress_assigned_directly_is_sent_clamped_once() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let bar = app
            .world_mut()
            .spawn(BossBar::new("Boss").progress(0.5))
            .id();
        app.update();
        drain(&rx);

        app.world_mut().get_mut::<BossBar>(bar).unwrap().progress = f32::NAN;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Progress(1, 0.0)]);
        app.update();
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().get_mut::<BossBar>(bar).unwrap().progress = 7.0;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Progress(1, 1.0)]);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn late_joiners_get_add_and_leavers_get_remove() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1);
        let bar = app
            .world_mut()
            .spawn(BossBar::new("Boss").progress(0.2))
            .id();
        app.update();
        drain(&rx);

        let second = player(&mut app, 2);
        app.world_mut().get_mut::<BossBar>(bar).unwrap().progress = 0.4;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Add(2, 0.4), Sent::Progress(1, 0.4)]);

        app.world_mut().entity_mut(first).remove::<PlayerReady>();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1)]);

        app.world_mut().despawn(second);
        app.update();
        assert!(drain(&rx).is_empty());
        let state = app.world().get::<BossBarState>(bar).unwrap();
        assert_eq!(state.viewers().count(), 0);
    }

    #[test]
    fn despawn_and_component_removal_send_remove() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        player(&mut app, 2);
        let despawned = app.world_mut().spawn(BossBar::new("A")).id();
        let stripped = app.world_mut().spawn(BossBar::new("B")).id();
        app.update();
        drain(&rx);

        app.world_mut().despawn(despawned);
        assert_eq!(drain(&rx), vec![Sent::Remove(1), Sent::Remove(2)]);

        app.world_mut().entity_mut(stripped).remove::<BossBar>();
        assert_eq!(drain(&rx), vec![Sent::Remove(1), Sent::Remove(2)]);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn audience_changes_add_and_remove_viewers() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1);
        let second = player(&mut app, 2);
        app.world_mut().spawn((
            ClientId(3),
            PlayerReady,
            PlayerDimension(DimensionId::Nether),
        ));
        let bar = app
            .world_mut()
            .spawn(BossBar::new("Boss").viewers([first]))
            .id();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Add(1, 1.0)]);

        app.world_mut().get_mut::<BossBar>(bar).unwrap().audience =
            Audience::explicit([first, second]);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Add(2, 1.0)]);

        {
            let mut bar = app.world_mut().get_mut::<BossBar>(bar).unwrap();
            bar.audience = Audience::explicit([second]);
            bar.progress = 0.1;
        }
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1), Sent::Progress(2, 0.1)]);

        app.world_mut().get_mut::<BossBar>(bar).unwrap().audience =
            Audience::InDimension(DimensionId::Nether);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Add(3, 0.1), Sent::Remove(2)]);
    }
}
