//! Boss bars as entities: spawn a [`BossBar`], mutate its fields, despawn it.
//! Every ready player sees it unless a [`BossBarViewers`] component narrows the
//! audience. A `PostUpdate` system diffs each bar against what each viewer last
//! received and sends only the matching Boss Event actions.

use std::collections::HashSet;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use ussr_nbt::owned::{Nbt, Tag};
use uuid::Uuid;
use voidmc_protocol::clientbound::{BossEvent, BossEventAction};

pub use voidmc_protocol::clientbound::{BossBarColor, BossBarDivision, BossBarFlags};

use crate::players::Players;
use crate::schedule::VoidSystems;

#[derive(Component, Clone, Debug, PartialEq)]
#[require(BossBarSync)]
pub struct BossBar {
    pub title: String,
    pub progress: f32,
    pub color: BossBarColor,
    pub division: BossBarDivision,
    pub flags: BossBarFlags,
}

impl BossBar {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            progress: 1.0,
            color: BossBarColor::default(),
            division: BossBarDivision::default(),
            flags: BossBarFlags::empty(),
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

    /// Clamps to `0.0..=1.0`; NaN becomes `0.0`.
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = if progress.is_nan() {
            0.0
        } else {
            progress.clamp(0.0, 1.0)
        };
    }

    fn title_nbt(&self) -> Nbt {
        Nbt {
            name: "".into(),
            compound: vec![("text".into(), Tag::String(self.title.as_str().into()))].into(),
        }
    }

    fn add(&self) -> BossEventAction {
        BossEventAction::Add {
            title: self.title_nbt(),
            progress: self.progress,
            color: self.color,
            division: self.division,
            flags: self.flags,
        }
    }

    fn updates_since(&self, previous: &BossBar) -> Vec<BossEventAction> {
        let mut actions = Vec::new();
        if self.progress != previous.progress {
            actions.push(BossEventAction::UpdateProgress(self.progress));
        }
        if self.title != previous.title {
            actions.push(BossEventAction::UpdateTitle(self.title_nbt()));
        }
        if self.color != previous.color || self.division != previous.division {
            actions.push(BossEventAction::UpdateStyle {
                color: self.color,
                division: self.division,
            });
        }
        if self.flags != previous.flags {
            actions.push(BossEventAction::UpdateFlags(self.flags));
        }
        actions
    }
}

/// Restricts a [`BossBar`] to these player entities; without it every ready
/// player is a viewer.
#[derive(Component, Clone, Debug, Default)]
pub struct BossBarViewers(pub HashSet<Entity>);

impl BossBarViewers {
    pub fn new(players: impl IntoIterator<Item = Entity>) -> Self {
        Self(players.into_iter().collect())
    }

    pub fn add(&mut self, player: Entity) -> bool {
        self.0.insert(player)
    }

    pub fn remove(&mut self, player: Entity) -> bool {
        self.0.remove(&player)
    }
}

#[derive(Component, Debug)]
pub struct BossBarSync {
    id: Uuid,
    sent: Option<BossBar>,
    viewers: HashSet<Entity>,
}

impl Default for BossBarSync {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            sent: None,
            viewers: HashSet::new(),
        }
    }
}

impl BossBarSync {
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

fn sync_boss_bars(
    players: Players,
    mut bars: Query<(&BossBar, Option<&BossBarViewers>, &mut BossBarSync)>,
) {
    let ready: HashSet<Entity> = players.ready().entities().collect();
    for (bar, viewers, mut sync) in bars.iter_mut() {
        let desired: HashSet<Entity> = match viewers {
            Some(viewers) => viewers.0.intersection(&ready).copied().collect(),
            None => ready.clone(),
        };

        for gone in sync.viewers.difference(&desired) {
            players.send(*gone, sync.event(BossEventAction::Remove));
        }
        for joined in desired.difference(&sync.viewers) {
            players.send(*joined, sync.event(bar.add()));
        }

        if sync.sent.as_ref() != Some(bar) {
            if let Some(previous) = &sync.sent {
                let kept = desired.intersection(&sync.viewers).copied();
                let kept: Vec<Entity> = kept.collect();
                for action in bar.updates_since(previous) {
                    players.send_to(kept.iter().copied(), sync.event(action));
                }
            }
            sync.sent = Some(bar.clone());
        }
        sync.viewers = desired;
    }
}

fn remove_from_viewers(
    event: On<Remove, BossBar>,
    players: Players,
    mut bars: Query<&mut BossBarSync>,
) {
    let Ok(mut sync) = bars.get_mut(event.entity) else {
        return;
    };
    for viewer in sync.viewers.iter() {
        players.send(*viewer, sync.event(BossEventAction::Remove));
    }
    sync.viewers.clear();
    sync.sent = None;
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, PlayerReady};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

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

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Sent {
        Add(u32),
        Remove(u32),
        Progress(u32),
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
                    BossEventAction::Add { .. } => Sent::Add(out.client_id),
                    BossEventAction::Remove => Sent::Remove(out.client_id),
                    BossEventAction::UpdateProgress(_) => Sent::Progress(out.client_id),
                    BossEventAction::UpdateTitle(_) => Sent::Title(out.client_id),
                    BossEventAction::UpdateStyle { .. } => Sent::Style(out.client_id),
                    BossEventAction::UpdateFlags(_) => Sent::Flags(out.client_id),
                }
            })
            .collect();
        sent.sort();
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
    }

    #[test]
    fn adds_on_first_sight_then_stays_quiet() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut().spawn(BossBar::new("Boss"));

        app.update();
        assert_eq!(drain(&rx), vec![Sent::Add(1), Sent::Add(2)]);

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
        assert_eq!(drain(&rx), vec![Sent::Progress(1)]);

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
        assert_eq!(drain(&rx), vec![Sent::Add(2), Sent::Progress(1)]);

        app.world_mut().entity_mut(first).remove::<PlayerReady>();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1)]);

        app.world_mut().despawn(second);
        app.update();
        assert!(drain(&rx).is_empty());
        let sync = app.world().get::<BossBarSync>(bar).unwrap();
        assert_eq!(sync.viewers().count(), 0);
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
    fn explicit_viewers_can_be_added_and_removed() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1);
        let second = player(&mut app, 2);
        player(&mut app, 3);
        let bar = app
            .world_mut()
            .spawn((BossBar::new("Boss"), BossBarViewers::new([first])))
            .id();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Add(1)]);

        app.world_mut()
            .get_mut::<BossBarViewers>(bar)
            .unwrap()
            .add(second);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Add(2)]);

        app.world_mut()
            .get_mut::<BossBarViewers>(bar)
            .unwrap()
            .remove(first);
        app.world_mut().get_mut::<BossBar>(bar).unwrap().progress = 0.1;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1), Sent::Progress(2)]);
    }
}
