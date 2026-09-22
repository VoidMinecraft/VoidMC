//! Fire-and-forget advancement toasts: `toasts.toast(player, "New record!")
//! .description("1:23.4").icon(gold_ingot).frame(ToastFrame::Challenge).send()`
//! pops the top-right advancement popup, `.broadcast(..)` shows it to every
//! ready player. Each request is a synthetic advancement granted and removed in
//! two back-to-back [`UpdateAdvancements`] packets, so nothing lingers in the
//! client's advancement screen.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc_protocol::clientbound::{
    Advancement, AdvancementHolder, AdvancementProgress, AdvancementProgressEntry,
    ClientboundPacket, CriterionProgressEntry, DisplayInfo, ItemStackTemplate, UpdateAdvancements,
};
use voidmc_protocol::slot::DataComponentPatch;

pub use voidmc_protocol::clientbound::AdvancementFrame as ToastFrame;

use crate::components::Position;
use crate::item::{ItemId, ItemStack};
use crate::messages::{Target, TextColor, text_component};
use crate::players::{Audience, Players, Recipients, WorldPlayers};
use crate::sounds::{Sound, SoundPosition};

const ID_PREFIX: &str = "void:toast/";
const CRITERION: &str = "shown";
const DEFAULT_ICON: ItemId = ItemId(voidmc_data::v26_1_2::items::PAPER);

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(SystemParam)]
pub struct Toasts<'w, 's> {
    players: Players<'w, 's>,
    positions: Query<'w, 's, &'static Position>,
}

impl Toasts<'_, '_> {
    fn sink(&self) -> Sink<'_> {
        Sink::Players {
            players: &self.players,
            positions: &self.positions,
        }
    }

    pub fn toast(&self, player: Entity, title: impl Into<String>) -> ToastRequest<'_> {
        ToastRequest::new(self.sink(), Target::Player(player), title)
    }

    pub fn broadcast(&self, title: impl Into<String>) -> ToastRequest<'_> {
        ToastRequest::new(self.sink(), Target::Audience(Audience::All), title)
    }
}

pub struct WorldToasts<'w> {
    players: WorldPlayers<'w>,
    world: &'w World,
}

impl<'w> WorldToasts<'w> {
    pub fn new(world: &'w World) -> Self {
        Self {
            players: WorldPlayers::new(world),
            world,
        }
    }

    fn sink(&self) -> Sink<'_> {
        Sink::World {
            players: &self.players,
            world: self.world,
        }
    }

    pub fn toast(&self, player: Entity, title: impl Into<String>) -> ToastRequest<'_> {
        ToastRequest::new(self.sink(), Target::Player(player), title)
    }

    pub fn broadcast(&self, title: impl Into<String>) -> ToastRequest<'_> {
        ToastRequest::new(self.sink(), Target::Audience(Audience::All), title)
    }
}

enum Sink<'a> {
    Players {
        players: &'a Players<'a, 'a>,
        positions: &'a Query<'a, 'a, &'static Position>,
    },
    World {
        players: &'a WorldPlayers<'a>,
        world: &'a World,
    },
}

impl<'a> Sink<'a> {
    fn ready(&self) -> Recipients<'a> {
        match self {
            Sink::Players { players, .. } => players.ready(),
            Sink::World { players, .. } => players.ready(),
        }
    }

    fn send(&self, player: Entity, packet: impl Into<ClientboundPacket>) {
        match self {
            Sink::Players { players, .. } => players.send(player, packet),
            Sink::World { players, .. } => players.send(player, packet),
        }
    }

    fn position(&self, player: Entity) -> Option<SoundPosition> {
        match self {
            Sink::Players { positions, .. } => positions.get(player).ok().map(SoundPosition::from),
            Sink::World { world, .. } => world.get::<Position>(player).map(SoundPosition::from),
        }
    }

    fn play(&self, player: Entity, sound: &Sound) {
        let Some(position) = self.position(player) else {
            tracing::warn!(?player, "toast sound target has no Position; dropped");
            return;
        };
        if let Some(packet) = sound.packet_at(position) {
            self.send(player, packet);
        }
    }
}

#[must_use = "a toast request does nothing until `.send()`"]
pub struct ToastRequest<'a> {
    sink: Sink<'a>,
    target: Target,
    title: String,
    title_color: TextColor,
    description: String,
    description_color: TextColor,
    icon: ItemStack,
    frame: ToastFrame,
    sound: Option<Sound>,
}

impl<'a> ToastRequest<'a> {
    fn new(sink: Sink<'a>, target: Target, title: impl Into<String>) -> Self {
        Self {
            sink,
            target,
            title: title.into(),
            title_color: TextColor::White,
            description: String::new(),
            description_color: TextColor::White,
            icon: ItemStack::new(DEFAULT_ICON, 1),
            frame: ToastFrame::Task,
            sound: None,
        }
    }

    /// Colours the title; use [`description_color`](Self::description_color)
    /// for the description.
    pub fn color(mut self, color: TextColor) -> Self {
        self.title_color = color;
        self
    }

    /// Shown when hovering the toast; the popup itself only renders the title.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = text.into();
        self
    }

    pub fn description_color(mut self, color: TextColor) -> Self {
        self.description_color = color;
        self
    }

    /// An [`ItemId`] or a full [`ItemStack`] (components such as damage or a
    /// custom model are honoured); the count is never displayed.
    pub fn icon(mut self, icon: impl Into<ItemStack>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Task (plain), Goal (rounded) or Challenge (spiky, the client also plays
    /// its own fanfare).
    pub fn frame(mut self, frame: ToastFrame) -> Self {
        self.frame = frame;
        self
    }

    /// Played at each recipient's own position unless the sound is placed
    /// with [`Sound::at`]; its audience is ignored.
    pub fn sound(mut self, sound: Sound) -> Self {
        self.sound = Some(sound);
        self
    }

    /// Replaces the target with the ready players the audience selects; the
    /// `player` given to `toast` is discarded and delivery becomes ready-only.
    pub fn audience(mut self, audience: Audience) -> Self {
        self.target = Target::Audience(audience);
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    /// Narrows the current audience (`Audience::All` for a single-player
    /// request) so `entity` is skipped.
    pub fn except(mut self, entity: Entity) -> Self {
        self.target = self.target.except(entity);
        self
    }

    /// The add and remove packets `send()` would send, in wire order; each
    /// call claims a fresh synthetic advancement id.
    pub fn packets(&self) -> (UpdateAdvancements, UpdateAdvancements) {
        let id = format!("{ID_PREFIX}{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
        let icon = if self.icon.is_empty() {
            ItemStack::new(DEFAULT_ICON, 1)
        } else {
            self.icon.clone()
        };
        let add = UpdateAdvancements {
            reset: false,
            added: vec![AdvancementHolder {
                id: id.clone(),
                advancement: Advancement {
                    parent: None,
                    display: Some(DisplayInfo {
                        title: text_component(&self.title, self.title_color),
                        description: text_component(&self.description, self.description_color),
                        icon: ItemStackTemplate {
                            item_id: icon.item.0,
                            count: i32::from(icon.count.max(1)),
                            components: DataComponentPatch {
                                components_to_add: icon.components,
                                components_to_remove: Vec::new(),
                            },
                        },
                        frame: self.frame,
                        show_toast: true,
                        hidden: true,
                        background: None,
                        x: 0.0,
                        y: 0.0,
                    }),
                    requirements: vec![vec![CRITERION.into()]],
                    sends_telemetry: false,
                },
            }],
            removed: Vec::new(),
            progress: vec![AdvancementProgressEntry {
                id: id.clone(),
                progress: AdvancementProgress {
                    criteria: vec![CriterionProgressEntry {
                        name: CRITERION.into(),
                        obtained_epoch_millis: Some(now_millis()),
                    }],
                },
            }],
            show_advancements: false,
        };
        let remove = UpdateAdvancements {
            reset: false,
            added: Vec::new(),
            removed: vec![id],
            progress: Vec::new(),
            show_advancements: false,
        };
        (add, remove)
    }

    pub fn send(self) {
        let (add, remove) = self.packets();
        match &self.target {
            Target::Player(player) => {
                self.sink.send(*player, add);
                self.sink.send(*player, remove);
                if let Some(sound) = &self.sound {
                    self.sink.play(*player, sound);
                }
            }
            Target::Audience(audience) => {
                let recipients = audience.resolve(self.sink.ready());
                recipients.send(add);
                recipients.send(remove);
                if let Some(sound) = &self.sound {
                    for player in recipients.entities() {
                        self.sink.play(player, sound);
                    }
                }
            }
        }
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bevy_app::{App, Update};
    use flume::Receiver;
    use ussr_nbt::owned::{Nbt, Tag};
    use voidmc_protocol::clientbound::{ManualPlayPacket, PlayPacket};
    use voidmc_protocol::slot::DataComponent;

    use super::*;
    use crate::components::{ClientId, PlayerDimension, PlayerReady};
    use crate::messages::assert_guarded;
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
        });
        app.insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx));
        (app, outgoing_rx)
    }

    fn ready_player(app: &mut App, client_id: u32, dimension: DimensionId) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(client_id),
                PlayerReady,
                PlayerDimension(dimension),
                Position {
                    x: f64::from(client_id),
                    y: 64.0,
                    z: 0.0,
                },
            ))
            .id()
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Sent {
        Add(String),
        Remove(String),
        Sound(i32),
    }

    fn classify(packet: ClientboundPacket) -> Sent {
        match packet {
            ClientboundPacket::ManualPlay(ManualPlayPacket::UpdateAdvancements(p)) => {
                assert!(!p.reset);
                assert!(!p.show_advancements);
                match (p.added.as_slice(), p.removed.as_slice()) {
                    ([holder], []) => {
                        assert_eq!(p.progress.len(), 1);
                        assert_eq!(p.progress[0].id, holder.id);
                        Sent::Add(holder.id.clone())
                    }
                    ([], [id]) => {
                        assert!(p.progress.is_empty());
                        Sent::Remove(id.clone())
                    }
                    other => panic!("unexpected advancement packet {other:?}"),
                }
            }
            ClientboundPacket::Play(PlayPacket::SoundEffect(p)) => Sent::Sound(p.x),
            other => panic!("unexpected packet {other:?}"),
        }
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<(u32, Sent)> {
        rx.try_iter()
            .map(|out| (out.client_id, classify(out.packet)))
            .collect()
    }

    fn per_client(sent: Vec<(u32, Sent)>) -> BTreeMap<u32, Vec<Sent>> {
        let mut clients: BTreeMap<u32, Vec<Sent>> = BTreeMap::new();
        for (client, packet) in sent {
            clients.entry(client).or_default().push(packet);
        }
        clients
    }

    fn assert_pair(packets: &[Sent]) -> &str {
        let [Sent::Add(added), Sent::Remove(removed)] = packets else {
            panic!("expected an add/remove pair, got {packets:?}");
        };
        assert_eq!(added, removed);
        assert!(added.starts_with(ID_PREFIX));
        added
    }

    fn field(nbt: &Nbt, key: &str) -> String {
        match nbt
            .compound
            .tags
            .iter()
            .find(|(name, _)| name.to_string() == key)
        {
            Some((_, Tag::String(value))) => value.to_string(),
            other => panic!("expected string {key}, got {other:?}"),
        }
    }

    fn display(packet: &UpdateAdvancements) -> &DisplayInfo {
        packet.added[0].advancement.display.as_ref().unwrap()
    }

    #[test]
    fn packets_carry_the_display_and_a_granted_criterion() {
        let (mut app, _rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        let gold = ItemId::from_name("gold_ingot").unwrap();

        app.add_systems(Update, move |toasts: Toasts| {
            let (add, remove) = toasts
                .toast(alice, "New record!")
                .description("1:23.4")
                .color(TextColor::Gold)
                .description_color(TextColor::Gray)
                .icon(gold)
                .frame(ToastFrame::Challenge)
                .packets();

            let holder = &add.added[0];
            assert!(holder.id.starts_with(ID_PREFIX));
            assert_eq!(holder.advancement.parent, None);
            assert_eq!(
                holder.advancement.requirements,
                vec![vec![CRITERION.to_string()]]
            );
            assert!(!holder.advancement.sends_telemetry);
            let info = display(&add);
            assert_eq!(field(&info.title, "text"), "New record!");
            assert_eq!(field(&info.title, "color"), "gold");
            assert_eq!(field(&info.description, "text"), "1:23.4");
            assert_eq!(field(&info.description, "color"), "gray");
            assert_eq!(info.icon, ItemStackTemplate::simple(gold.0, 1));
            assert_eq!(info.frame, ToastFrame::Challenge);
            assert!(info.show_toast);
            assert!(info.hidden);
            assert_eq!(info.background, None);
            assert_eq!(add.progress[0].id, holder.id);
            let criteria = &add.progress[0].progress.criteria;
            assert_eq!(criteria.len(), 1);
            assert_eq!(criteria[0].name, CRITERION);
            assert!(criteria[0].obtained_epoch_millis.unwrap() > 1_600_000_000_000);

            assert_eq!(
                remove,
                UpdateAdvancements {
                    reset: false,
                    added: vec![],
                    removed: vec![holder.id.clone()],
                    progress: vec![],
                    show_advancements: false,
                }
            );
        });
        app.update();
    }

    #[test]
    fn defaults_are_paper_icon_task_frame_white_and_empty_description() {
        let (mut app, _rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);

        app.add_systems(Update, move |toasts: Toasts| {
            let (add, _) = toasts.toast(alice, "plain").packets();
            let info = display(&add);
            assert_eq!(field(&info.title, "color"), "white");
            assert_eq!(field(&info.description, "text"), "");
            assert_eq!(info.icon, ItemStackTemplate::simple(DEFAULT_ICON.0, 1));
            assert_eq!(info.frame, ToastFrame::Task);

            let (add, _) = toasts
                .toast(alice, "empty icon")
                .icon(ItemStack::EMPTY)
                .packets();
            assert_eq!(
                display(&add).icon,
                ItemStackTemplate::simple(DEFAULT_ICON.0, 1)
            );

            let sword = ItemStack::of("diamond_sword", 3).unwrap().with_damage(12);
            let (add, _) = toasts.toast(alice, "stack").icon(sword.clone()).packets();
            let icon = &display(&add).icon;
            assert_eq!(icon.item_id, sword.item.0);
            assert_eq!(icon.count, 3);
            assert_eq!(
                icon.components.components_to_add,
                vec![DataComponent::Damage(12)]
            );
            assert!(icon.components.components_to_remove.is_empty());
        });
        app.update();
    }

    #[test]
    fn every_request_claims_a_unique_id() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);

        app.add_systems(Update, move |toasts: Toasts| {
            toasts.toast(alice, "first").send();
            toasts.toast(alice, "second").send();
            toasts.broadcast("third").send();
        });
        app.update();

        let sent = per_client(drain(&rx)).remove(&1).unwrap();
        assert_eq!(sent.len(), 6);
        let ids: Vec<&str> = sent.chunks(2).map(assert_pair).collect();
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert_ne!(ids[0], ids[2]);
    }

    #[test]
    fn toast_reaches_one_player_even_before_ready() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Overworld);
        let joining = app.world_mut().spawn(ClientId(3)).id();

        app.add_systems(Update, move |toasts: Toasts| {
            toasts.toast(alice, "hi").send();
            toasts.toast(joining, "welcome").send();
            let _abandoned = toasts.toast(alice, "never sent");
        });
        app.update();

        let sent = per_client(drain(&rx));
        assert_eq!(sent.keys().copied().collect::<Vec<_>>(), vec![1, 3]);
        assert_pair(&sent[&1]);
        assert_pair(&sent[&3]);
    }

    #[test]
    fn broadcast_shares_one_id_across_every_ready_player() {
        let (mut app, rx) = test_app();
        ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Nether);
        ready_player(&mut app, 3, DimensionId::End);
        app.world_mut().spawn(ClientId(4));

        app.add_systems(Update, |toasts: Toasts| {
            toasts.broadcast("all").send();
        });
        app.update();

        let sent = per_client(drain(&rx));
        assert_eq!(sent.keys().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
        let id = assert_pair(&sent[&1]).to_string();
        assert_eq!(assert_pair(&sent[&2]), id);
        assert_eq!(assert_pair(&sent[&3]), id);
    }

    #[test]
    fn audience_viewers_and_except_replace_the_target() {
        let (mut app, rx) = test_app();
        let overworld = ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Nether);
        let end = ready_player(&mut app, 3, DimensionId::End);

        app.add_systems(Update, move |toasts: Toasts| {
            toasts
                .broadcast("nether")
                .audience(Audience::InDimension(DimensionId::Nether))
                .send();
            toasts.toast(overworld, "party").viewers([end]).send();
            toasts.broadcast("not you").except(overworld).send();
            toasts.toast(end, "everyone but end").except(end).send();
        });
        app.update();

        let sent = per_client(drain(&rx));
        assert_eq!(sent[&1].len(), 2);
        assert_eq!(sent[&2].len(), 6);
        assert_eq!(sent[&3].len(), 4);
        let everyone_but_end = assert_pair(&sent[&1]).to_string();
        assert_eq!(assert_pair(&sent[&2][4..]), everyone_but_end);
        let nether = assert_pair(&sent[&2][..2]).to_string();
        let not_you = assert_pair(&sent[&2][2..4]).to_string();
        let party = assert_pair(&sent[&3][..2]).to_string();
        assert_eq!(assert_pair(&sent[&3][2..]), not_you);
        assert_ne!(nether, not_you);
        assert_ne!(party, not_you);
    }

    #[test]
    fn sound_plays_at_each_recipient_after_the_toast() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Nether);
        let silent = app
            .world_mut()
            .spawn((ClientId(3), PlayerReady, PlayerDimension(DimensionId::End)))
            .id();

        app.add_systems(Update, move |toasts: Toasts| {
            toasts
                .broadcast("fanfare")
                .sound(Sound::new("entity.player.levelup").volume(0.5))
                .send();
            toasts
                .toast(alice, "placed")
                .sound(
                    Sound::new("block.note_block.pling")
                        .at(DimensionId::Overworld, (40.0, 0.0, 0.0)),
                )
                .send();
            let _ = silent;
        });
        app.update();

        let sent = per_client(drain(&rx));
        assert_eq!(sent[&1].len(), 6);
        assert_pair(&sent[&1][..2]);
        assert_eq!(sent[&1][2], Sent::Sound(8));
        assert_pair(&sent[&1][3..5]);
        assert_eq!(sent[&1][5], Sent::Sound(320));
        assert_eq!(sent[&2].len(), 3);
        assert_eq!(sent[&2][2], Sent::Sound(16));
        assert_eq!(sent[&3].len(), 2);
        assert_pair(&sent[&3]);
    }

    #[test]
    fn world_toasts_twin_matches() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Overworld);

        let toasts = WorldToasts::new(app.world());
        toasts
            .toast(alice, "twin")
            .sound(Sound::new("entity.player.levelup"))
            .send();
        toasts.broadcast("all").frame(ToastFrame::Goal).send();

        let sent = per_client(drain(&rx));
        assert_eq!(sent[&1].len(), 5);
        assert_pair(&sent[&1][..2]);
        assert_eq!(sent[&1][2], Sent::Sound(8));
        assert_pair(&sent[&1][3..]);
        assert_eq!(sent[&2].len(), 2);
        assert_pair(&sent[&2]);
    }

    #[test]
    fn oversized_texts_are_guarded() {
        let (mut app, _rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        let long = "x".repeat(70_000);

        app.add_systems(Update, move |toasts: Toasts| {
            let (add, _) = toasts
                .toast(alice, long.clone())
                .description(long.clone())
                .packets();
            let info = display(&add);
            assert_guarded(&info.title, &long);
            assert_guarded(&info.description, &long);
        });
        app.update();
    }
}
