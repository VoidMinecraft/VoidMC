//! Tab list customisation. A [`TabList`] entity carries the header and footer
//! its [`Audience`] sees; a [`TabEntry`] on a player entity shapes that
//! player's row for every ready player. A `PostUpdate` system diffs both
//! against what viewers last received and sends only the changed actions.

use std::collections::HashSet;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use uuid::Uuid;
use voidmc_protocol::clientbound::{
    PlayerInfoActions, PlayerInfoEntry, PlayerInfoUpdate, SetTabListHeaderFooter,
};

use crate::components::{KeepAliveState, PlayerReady, PlayerUuid};
use crate::config::ServerConfigResource;
use crate::messages::{TextColor, text_component};
use crate::players::{Audience, Players};
use crate::schedule::VoidSystems;

#[derive(Component, Clone, Debug)]
#[require(TabListState)]
pub struct TabList {
    pub header: String,
    pub footer: String,
    pub header_color: TextColor,
    pub footer_color: TextColor,
    pub audience: Audience,
}

impl Default for TabList {
    fn default() -> Self {
        Self::new()
    }
}

impl TabList {
    pub fn new() -> Self {
        Self {
            header: String::new(),
            footer: String::new(),
            header_color: TextColor::White,
            footer_color: TextColor::White,
            audience: Audience::All,
        }
    }

    pub fn header(mut self, header: impl Into<String>) -> Self {
        self.header = header.into();
        self
    }

    pub fn footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }

    pub fn header_color(mut self, color: TextColor) -> Self {
        self.header_color = color;
        self
    }

    pub fn footer_color(mut self, color: TextColor) -> Self {
        self.footer_color = color;
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    fn snapshot(&self) -> HeaderFooter {
        HeaderFooter {
            header: self.header.clone(),
            footer: self.footer.clone(),
            header_color: self.header_color,
            footer_color: self.footer_color,
        }
    }

    fn differs_from(&self, sent: &HeaderFooter) -> bool {
        self.header != sent.header
            || self.footer != sent.footer
            || self.header_color != sent.header_color
            || self.footer_color != sent.footer_color
    }

    fn packet(&self) -> SetTabListHeaderFooter {
        SetTabListHeaderFooter {
            header: text_component(&self.header, self.header_color),
            footer: text_component(&self.footer, self.footer_color),
        }
    }
}

fn reset_packet() -> SetTabListHeaderFooter {
    SetTabListHeaderFooter {
        header: text_component("", TextColor::White),
        footer: text_component("", TextColor::White),
    }
}

#[derive(Debug, Clone)]
struct HeaderFooter {
    header: String,
    footer: String,
    header_color: TextColor,
    footer_color: TextColor,
}

#[derive(Component, Debug, Default)]
pub struct TabListState {
    sent: Option<HeaderFooter>,
    viewers: HashSet<Entity>,
}

impl TabListState {
    pub fn viewers(&self) -> impl Iterator<Item = Entity> + '_ {
        self.viewers.iter().copied()
    }
}

/// `None` fields fall back to what the engine knows: the measured keep-alive
/// round trip for `latency`, the server's configured mode for `game_mode`.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
#[require(TabEntryState)]
pub struct TabEntry {
    pub display_name: Option<String>,
    pub color: TextColor,
    pub listed: bool,
    pub latency: Option<i32>,
    pub game_mode: Option<u8>,
    pub list_order: i32,
    pub show_hat: bool,
}

impl Default for TabEntry {
    fn default() -> Self {
        Self::new()
    }
}

impl TabEntry {
    pub fn new() -> Self {
        Self {
            display_name: None,
            color: TextColor::White,
            listed: true,
            latency: None,
            game_mode: None,
            list_order: 0,
            show_hat: true,
        }
    }

    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    pub fn color(mut self, color: TextColor) -> Self {
        self.color = color;
        self
    }

    pub fn listed(mut self, listed: bool) -> Self {
        self.listed = listed;
        self
    }

    pub fn hidden(self) -> Self {
        self.listed(false)
    }

    pub fn latency(mut self, millis: i32) -> Self {
        self.latency = Some(millis);
        self
    }

    pub fn game_mode(mut self, game_mode: u8) -> Self {
        self.game_mode = Some(game_mode);
        self
    }

    pub fn list_order(mut self, order: i32) -> Self {
        self.list_order = order;
        self
    }

    pub fn show_hat(mut self, show: bool) -> Self {
        self.show_hat = show;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntrySnapshot {
    display_name: Option<String>,
    color: TextColor,
    listed: bool,
    latency: i32,
    game_mode: u8,
    list_order: i32,
    show_hat: bool,
}

impl EntrySnapshot {
    pub(crate) fn resolve(
        entry: Option<&TabEntry>,
        keep_alive: Option<&KeepAliveState>,
        default_game_mode: u8,
    ) -> Self {
        let measured = keep_alive.map_or(0, |state| state.latency);
        match entry {
            Some(entry) => Self {
                display_name: entry.display_name.clone(),
                color: entry.color,
                listed: entry.listed,
                latency: entry.latency.unwrap_or(measured),
                game_mode: entry.game_mode.unwrap_or(default_game_mode),
                list_order: entry.list_order,
                show_hat: entry.show_hat,
            },
            None => Self {
                display_name: None,
                color: TextColor::White,
                listed: true,
                latency: measured,
                game_mode: default_game_mode,
                list_order: 0,
                show_hat: true,
            },
        }
    }

    pub(crate) fn initial(&self, uuid: Uuid, name: &str) -> PlayerInfoEntry {
        PlayerInfoEntry {
            name: name.to_string(),
            ..self.wire(uuid)
        }
    }

    fn wire(&self, uuid: Uuid) -> PlayerInfoEntry {
        PlayerInfoEntry {
            uuid,
            name: String::new(),
            properties: Vec::new(),
            game_mode: i32::from(self.game_mode),
            listed: self.listed,
            latency: self.latency,
            display_name: self
                .display_name
                .as_deref()
                .map(|name| text_component(name, self.color)),
            list_order: self.list_order,
            show_hat: self.show_hat,
        }
    }

    fn changes_since(&self, sent: &Self) -> PlayerInfoActions {
        let mut actions = PlayerInfoActions::empty();
        if self.game_mode != sent.game_mode {
            actions |= PlayerInfoActions::UPDATE_GAME_MODE;
        }
        if self.listed != sent.listed {
            actions |= PlayerInfoActions::UPDATE_LISTED;
        }
        if self.latency != sent.latency {
            actions |= PlayerInfoActions::UPDATE_LATENCY;
        }
        if self.display_name != sent.display_name
            || (self.display_name.is_some() && self.color != sent.color)
        {
            actions |= PlayerInfoActions::UPDATE_DISPLAY_NAME;
        }
        if self.list_order != sent.list_order {
            actions |= PlayerInfoActions::UPDATE_LIST_ORDER;
        }
        if self.show_hat != sent.show_hat {
            actions |= PlayerInfoActions::UPDATE_HAT;
        }
        actions
    }
}

#[derive(Component, Debug, Default)]
pub struct TabEntryState {
    sent: Option<EntrySnapshot>,
    dirty: bool,
}

impl TabEntryState {
    pub(crate) fn sent(snapshot: EntrySnapshot) -> Self {
        Self {
            sent: Some(snapshot),
            dirty: false,
        }
    }

    pub(crate) fn last_sent(&self) -> Option<&EntrySnapshot> {
        self.sent.as_ref()
    }
}

pub struct TabListPlugin;

impl Plugin for TabListPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(reset_removed_tab_list)
            .add_observer(mark_removed_entry)
            .add_systems(
                PostUpdate,
                (sync_tab_lists, sync_tab_entries).in_set(VoidSystems::TabListSync),
            );
    }
}

fn sync_tab_lists(players: Players, mut lists: Query<(&TabList, &mut TabListState)>) {
    if lists.is_empty() {
        return;
    }
    let ready = players.ready();
    for (list, mut state) in lists.iter_mut() {
        let members = || {
            ready
                .iter()
                .filter(|r| list.audience.includes(r))
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

        let changed = state
            .sent
            .as_ref()
            .is_none_or(|sent| list.differs_from(sent));
        if changed {
            let kept = state
                .viewers
                .iter()
                .copied()
                .filter(|viewer| desired.as_ref().is_none_or(|d| d.contains(viewer)));
            players.send_to(kept, list.packet());
            state.sent = Some(list.snapshot());
        }

        if let Some(desired) = desired {
            for joined in desired.difference(&state.viewers) {
                players.send(*joined, list.packet());
            }
            state.viewers = desired;
        }
    }
}

fn sync_tab_entries(
    players: Players,
    config: Option<Res<ServerConfigResource>>,
    mut rows: Query<
        (
            &PlayerUuid,
            Option<Ref<TabEntry>>,
            Option<Ref<KeepAliveState>>,
            &mut TabEntryState,
        ),
        With<PlayerReady>,
    >,
) {
    let default_game_mode = config.as_ref().map_or(0, |config| config.game_mode);
    let mut batches: Vec<PlayerInfoUpdate> = Vec::new();
    for (uuid, entry, keep_alive, mut state) in rows.iter_mut() {
        let changed = state.dirty
            || entry.as_ref().is_some_and(|entry| entry.is_changed())
            || keep_alive.as_ref().is_some_and(|state| state.is_changed());
        if !changed {
            continue;
        }
        state.dirty = false;
        let current =
            EntrySnapshot::resolve(entry.as_deref(), keep_alive.as_deref(), default_game_mode);
        let Some(sent) = &state.sent else {
            state.sent = Some(current);
            continue;
        };
        let actions = current.changes_since(sent);
        if actions.is_empty() {
            continue;
        }
        let row = current.wire(uuid.0);
        match batches.iter_mut().find(|batch| batch.actions == actions) {
            Some(batch) => batch.entries.push(row),
            None => batches.push(PlayerInfoUpdate::single(actions, row)),
        }
        state.sent = Some(current);
    }
    if batches.is_empty() {
        return;
    }
    let ready = players.ready();
    for batch in batches {
        ready.send(batch);
    }
}

fn reset_removed_tab_list(
    event: On<Remove, TabList>,
    players: Players,
    mut lists: Query<&mut TabListState>,
) {
    let Ok(mut state) = lists.get_mut(event.entity) else {
        return;
    };
    for viewer in state.viewers.iter() {
        players.send(*viewer, reset_packet());
    }
    state.viewers.clear();
    state.sent = None;
}

fn mark_removed_entry(event: On<Remove, TabEntry>, mut states: Query<&mut TabEntryState>) {
    if let Ok(mut state) = states.get_mut(event.entity) {
        state.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;
    use ussr_nbt::owned::Tag;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, PlayerDimension};
    use crate::config::{ServerConfigBuilder, ServerConfigResource};
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
        .insert_resource(ServerConfigResource::from(
            &ServerConfigBuilder::new().game_mode(2).build(),
        ))
        .insert_non_send_resource(incoming_tx)
        .add_plugins(TabListPlugin);
        (app, outgoing_rx)
    }

    fn viewer(app: &mut App, id: u32) -> Entity {
        app.world_mut().spawn((ClientId(id), PlayerReady)).id()
    }

    fn known_player(app: &mut App, id: u32, entry: TabEntry) -> Entity {
        let snapshot = EntrySnapshot::resolve(Some(&entry), None, 2);
        app.world_mut()
            .spawn((
                ClientId(id),
                PlayerReady,
                PlayerUuid(Uuid::from_u128(id.into())),
                KeepAliveState::default(),
                entry,
                TabEntryState::sent(snapshot),
            ))
            .id()
    }

    fn plain(text: &str) -> String {
        format!("{text}|white")
    }

    fn text_of(nbt: &ussr_nbt::owned::Nbt) -> String {
        let mut text = String::new();
        let mut color = String::new();
        for (key, value) in nbt.compound.tags.iter() {
            if let Tag::String(value) = value {
                match key.to_string().as_str() {
                    "text" => text = value.to_string(),
                    "color" => color = value.to_string(),
                    _ => {}
                }
            }
        }
        format!("{text}|{color}")
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Sent {
        HeaderFooter(u32, String, String),
        Info(
            u32,
            u8,
            Vec<(Uuid, i32, bool, i32, Option<String>, i32, bool)>,
        ),
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent: Vec<Sent> = rx
            .try_iter()
            .map(|out| match out.packet {
                ClientboundPacket::Play(PlayPacket::SetTabListHeaderFooter(packet)) => {
                    Sent::HeaderFooter(
                        out.client_id,
                        text_of(&packet.header),
                        text_of(&packet.footer),
                    )
                }
                ClientboundPacket::Play(PlayPacket::PlayerInfoUpdate(packet)) => Sent::Info(
                    out.client_id,
                    packet.actions.bits(),
                    packet
                        .entries
                        .iter()
                        .map(|e| {
                            (
                                e.uuid,
                                e.game_mode,
                                e.listed,
                                e.latency,
                                e.display_name.as_ref().map(text_of),
                                e.list_order,
                                e.show_hat,
                            )
                        })
                        .collect(),
                ),
                other => panic!("unexpected packet {other:?}"),
            })
            .collect();
        sent.sort();
        sent
    }

    fn info_bytes(rx: &Receiver<OutgoingPacket>) -> Vec<(u32, Vec<u8>)> {
        let mut sent: Vec<(u32, Vec<u8>)> = rx
            .try_iter()
            .map(|out| {
                let ClientboundPacket::Play(packet) = out.packet else {
                    panic!("unexpected packet {:?}", out.packet);
                };
                let mut buf = Vec::new();
                voidmc_codec::Encode::encode(&packet, &mut buf);
                (out.client_id, buf)
            })
            .collect();
        sent.sort();
        sent
    }

    #[test]
    fn builders_set_fields() {
        let list = TabList::new()
            .header("Alpine Rush")
            .footer("void.mc")
            .header_color(TextColor::Gold)
            .footer_color(TextColor::Gray)
            .viewers([Entity::PLACEHOLDER]);
        assert_eq!(list.header, "Alpine Rush");
        assert_eq!(list.footer, "void.mc");
        assert_eq!(list.header_color, TextColor::Gold);
        assert_eq!(list.footer_color, TextColor::Gray);
        assert!(matches!(list.audience, Audience::Explicit(_)));
        assert!(matches!(TabList::new().audience, Audience::All));

        let entry = TabEntry::new()
            .display_name("Leo")
            .color(TextColor::Gold)
            .hidden()
            .latency(42)
            .game_mode(3)
            .list_order(-5)
            .show_hat(false);
        assert_eq!(
            entry,
            TabEntry {
                display_name: Some("Leo".into()),
                color: TextColor::Gold,
                listed: false,
                latency: Some(42),
                game_mode: Some(3),
                list_order: -5,
                show_hat: false,
            }
        );
        assert_eq!(TabEntry::default(), TabEntry::new().listed(true));
    }

    #[test]
    fn oversized_texts_round_trip_below_the_nbt_limit() {
        let text = "😀".repeat(11000);
        let packet = TabList::new()
            .header(text.clone())
            .footer(text.clone())
            .packet();
        crate::messages::assert_guarded(&packet.header, &text);
        crate::messages::assert_guarded(&packet.footer, &text);
        let entry =
            EntrySnapshot::resolve(Some(&TabEntry::new().display_name(text.clone())), None, 0)
                .wire(Uuid::nil());
        crate::messages::assert_guarded(entry.display_name.as_ref().unwrap(), &text);
    }

    #[test]
    fn header_footer_reaches_viewers_once_and_on_change() {
        let (mut app, rx) = test_app();
        viewer(&mut app, 1);
        viewer(&mut app, 2);
        let list = app
            .world_mut()
            .spawn(TabList::new().header("Top").footer("Bottom"))
            .id();

        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::HeaderFooter(1, plain("Top"), plain("Bottom")),
                Sent::HeaderFooter(2, plain("Top"), plain("Bottom")),
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());

        {
            let mut list = app.world_mut().get_mut::<TabList>(list).unwrap();
            list.footer = "void.mc".into();
            list.footer_color = TextColor::Gray;
        }
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::HeaderFooter(1, plain("Top"), "void.mc|gray".into()),
                Sent::HeaderFooter(2, plain("Top"), "void.mc|gray".into()),
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn header_footer_late_joiners_leavers_and_removal() {
        let (mut app, rx) = test_app();
        let first = viewer(&mut app, 1);
        let list = app.world_mut().spawn(TabList::new().header("Top")).id();
        app.update();
        drain(&rx);

        viewer(&mut app, 2);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::HeaderFooter(2, plain("Top"), plain(""))]
        );

        app.world_mut().entity_mut(first).remove::<PlayerReady>();
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::HeaderFooter(1, plain(""), plain(""))]
        );

        app.world_mut().spawn((
            ClientId(3),
            PlayerReady,
            PlayerDimension(DimensionId::Nether),
        ));
        app.world_mut().get_mut::<TabList>(list).unwrap().audience =
            Audience::InDimension(DimensionId::Nether);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::HeaderFooter(2, plain(""), plain("")),
                Sent::HeaderFooter(3, plain("Top"), plain("")),
            ]
        );

        app.world_mut().entity_mut(list).remove::<TabList>();
        assert_eq!(
            drain(&rx),
            vec![Sent::HeaderFooter(3, plain(""), plain(""))]
        );
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world()
                .get::<TabListState>(list)
                .unwrap()
                .viewers()
                .count(),
            0
        );
    }

    #[test]
    fn entry_changes_send_only_the_changed_actions_to_everyone() {
        let (mut app, rx) = test_app();
        let leo = known_player(&mut app, 1, TabEntry::new());
        viewer(&mut app, 2);
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut()
            .get_mut::<TabEntry>(leo)
            .unwrap()
            .display_name = Some("Leo".into());
        app.update();
        let row = (Uuid::from_u128(1), 2, true, 0, Some(plain("Leo")), 0, true);
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Info(1, 0x20, vec![row.clone()]),
                Sent::Info(2, 0x20, vec![row]),
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());

        {
            let mut entry = app.world_mut().get_mut::<TabEntry>(leo).unwrap();
            entry.color = TextColor::Gold;
            entry.listed = false;
            entry.list_order = 3;
            entry.show_hat = false;
            entry.game_mode = Some(1);
        }
        app.update();
        let row = (
            Uuid::from_u128(1),
            1,
            false,
            0,
            Some("Leo|gold".into()),
            3,
            false,
        );
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Info(1, 0xEC, vec![row.clone()]),
                Sent::Info(2, 0xEC, vec![row]),
            ]
        );

        app.world_mut()
            .get_mut::<TabEntry>(leo)
            .unwrap()
            .display_name = None;
        app.update();
        let row = (Uuid::from_u128(1), 1, false, 0, None, 3, false);
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Info(1, 0x20, vec![row.clone()]),
                Sent::Info(2, 0x20, vec![row]),
            ]
        );

        app.world_mut().get_mut::<TabEntry>(leo).unwrap().color = TextColor::Red;
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn measured_latency_is_pushed_only_on_change_and_overridable() {
        let (mut app, rx) = test_app();
        let leo = known_player(&mut app, 1, TabEntry::new());
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<KeepAliveState>(leo)
            .unwrap()
            .latency = 42;
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::Info(
                1,
                0x10,
                vec![(Uuid::from_u128(1), 2, true, 42, None, 0, true)]
            )]
        );

        app.world_mut()
            .get_mut::<KeepAliveState>(leo)
            .unwrap()
            .awaiting_response = true;
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().get_mut::<TabEntry>(leo).unwrap().latency = Some(7);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::Info(
                1,
                0x10,
                vec![(Uuid::from_u128(1), 2, true, 7, None, 0, true)]
            )]
        );

        app.world_mut()
            .get_mut::<KeepAliveState>(leo)
            .unwrap()
            .latency = 99;
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn rows_changed_in_one_tick_are_batched_per_action_mask() {
        let (mut app, rx) = test_app();
        let alpha = known_player(&mut app, 1, TabEntry::new());
        let beta = known_player(&mut app, 2, TabEntry::new());
        let gamma = known_player(&mut app, 3, TabEntry::new());
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut()
            .get_mut::<KeepAliveState>(alpha)
            .unwrap()
            .latency = 1;
        app.world_mut()
            .get_mut::<KeepAliveState>(beta)
            .unwrap()
            .latency = 300;
        app.world_mut().get_mut::<TabEntry>(gamma).unwrap().listed = false;
        app.update();

        let mut latency = vec![0x46, 0x10, 0x02];
        latency.extend(Uuid::from_u128(1).as_bytes());
        latency.push(0x01);
        latency.extend(Uuid::from_u128(2).as_bytes());
        latency.extend([0xAC, 0x02]);
        let mut listed = vec![0x46, 0x08, 0x01];
        listed.extend(Uuid::from_u128(3).as_bytes());
        listed.push(0x00);
        assert_eq!(
            info_bytes(&rx),
            vec![
                (1, listed.clone()),
                (1, latency.clone()),
                (2, listed.clone()),
                (2, latency.clone()),
                (3, listed),
                (3, latency),
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn replayed_keep_alive_does_not_resend_latency() {
        let (mut app, rx) = test_app();
        app.add_plugins(crate::plugins::play::PlayPlugin);
        let leo = known_player(&mut app, 1, TabEntry::new());
        let sent_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            - 100;
        app.world_mut().entity_mut(leo).insert(KeepAliveState {
            last_sent_id: sent_at,
            awaiting_response: true,
            latency: 0,
        });
        app.update();
        assert!(drain(&rx).is_empty());

        let reply = || crate::network::PacketEvent {
            client_id: 1,
            entity: leo,
            packet: voidmc_protocol::serverbound::KeepAlive {
                keep_alive_id: sent_at,
            },
        };
        app.world_mut().trigger(reply());
        app.update();
        let sent = drain(&rx);
        assert_eq!(sent.len(), 1);
        assert!(matches!(&sent[0], Sent::Info(1, 0x10, rows) if rows[0].3 >= 100));
        let latency = app.world().get::<KeepAliveState>(leo).unwrap().latency;

        for _ in 0..3 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.world_mut().trigger(reply());
            app.update();
        }
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world().get::<KeepAliveState>(leo).unwrap().latency,
            latency
        );
    }

    #[test]
    fn removing_the_entry_reverts_to_defaults() {
        let (mut app, rx) = test_app();
        let leo = known_player(
            &mut app,
            1,
            TabEntry::new().display_name("Leo").hidden().latency(5),
        );
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().entity_mut(leo).remove::<TabEntry>();
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::Info(
                1,
                0x38,
                vec![(Uuid::from_u128(1), 2, true, 0, None, 0, true)]
            )]
        );
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn unknown_snapshot_is_recorded_without_sending() {
        let (mut app, rx) = test_app();
        let leo = app
            .world_mut()
            .spawn((
                ClientId(1),
                PlayerReady,
                PlayerUuid(Uuid::from_u128(1)),
                TabEntry::new().display_name("Leo"),
            ))
            .id();
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().get_mut::<TabEntry>(leo).unwrap().list_order = 1;
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::Info(
                1,
                0x40,
                vec![(Uuid::from_u128(1), 2, true, 0, Some(plain("Leo")), 1, true)]
            )]
        );
    }
}
