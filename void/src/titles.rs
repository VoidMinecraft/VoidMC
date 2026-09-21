//! Fire-and-forget on-screen titles: `titles.title(player, "Round 2")
//! .subtitle("Get ready").times(10, 70, 20).send()`, `.broadcast(..)` for every
//! ready player, and `.clear(player)` / `.reset(player)` to hide what is shown.
//! Packets go out in the vanilla order (times, subtitle, title) so the client
//! only starts the animation once everything is in place.

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc_protocol::clientbound::{
    ClearTitles, ClientboundPacket, SetSubtitleText, SetTitleText, SetTitlesAnimation,
};

use crate::messages::{TextColor, text_component};
use crate::players::{Audience, Players, Recipients, WorldPlayers};

#[derive(SystemParam)]
pub struct Titles<'w, 's> {
    players: Players<'w, 's>,
}

impl Titles<'_, '_> {
    pub fn title(&self, player: Entity, text: impl Into<String>) -> TitleRequest<'_> {
        TitleRequest::new(Sink::Players(&self.players), Target::Player(player)).title(text)
    }

    pub fn subtitle(&self, player: Entity, text: impl Into<String>) -> TitleRequest<'_> {
        TitleRequest::new(Sink::Players(&self.players), Target::Player(player)).subtitle(text)
    }

    pub fn broadcast(&self, text: impl Into<String>) -> TitleRequest<'_> {
        TitleRequest::new(
            Sink::Players(&self.players),
            Target::Audience(Audience::All),
        )
        .title(text)
    }

    pub fn clear(&self, player: Entity) -> ClearRequest<'_> {
        ClearRequest::new(Sink::Players(&self.players), Target::Player(player), false)
    }

    pub fn reset(&self, player: Entity) -> ClearRequest<'_> {
        ClearRequest::new(Sink::Players(&self.players), Target::Player(player), true)
    }
}

pub struct WorldTitles<'w> {
    players: WorldPlayers<'w>,
}

impl<'w> WorldTitles<'w> {
    pub fn new(world: &'w World) -> Self {
        Self {
            players: WorldPlayers::new(world),
        }
    }

    pub fn title(&self, player: Entity, text: impl Into<String>) -> TitleRequest<'_> {
        TitleRequest::new(Sink::World(&self.players), Target::Player(player)).title(text)
    }

    pub fn subtitle(&self, player: Entity, text: impl Into<String>) -> TitleRequest<'_> {
        TitleRequest::new(Sink::World(&self.players), Target::Player(player)).subtitle(text)
    }

    pub fn broadcast(&self, text: impl Into<String>) -> TitleRequest<'_> {
        TitleRequest::new(Sink::World(&self.players), Target::Audience(Audience::All)).title(text)
    }

    pub fn clear(&self, player: Entity) -> ClearRequest<'_> {
        ClearRequest::new(Sink::World(&self.players), Target::Player(player), false)
    }

    pub fn reset(&self, player: Entity) -> ClearRequest<'_> {
        ClearRequest::new(Sink::World(&self.players), Target::Player(player), true)
    }
}

enum Sink<'a> {
    Players(&'a Players<'a, 'a>),
    World(&'a WorldPlayers<'a>),
}

impl<'a> Sink<'a> {
    fn ready(&self) -> Recipients<'a> {
        match self {
            Sink::Players(players) => players.ready(),
            Sink::World(players) => players.ready(),
        }
    }

    fn send(&self, player: Entity, packet: ClientboundPacket) {
        match self {
            Sink::Players(players) => players.send(player, packet),
            Sink::World(players) => players.send(player, packet),
        }
    }

    fn deliver(&self, target: &Target, packets: impl IntoIterator<Item = ClientboundPacket>) {
        match target {
            Target::Player(player) => packets
                .into_iter()
                .for_each(|packet| self.send(*player, packet)),
            Target::Audience(audience) => {
                let recipients = audience.resolve(self.ready());
                packets
                    .into_iter()
                    .for_each(|packet| recipients.send(packet));
            }
        }
    }
}

enum Target {
    Player(Entity),
    Audience(Audience),
}

impl Target {
    fn except(self, entity: Entity) -> Self {
        let audience = match self {
            Target::Audience(audience) => audience,
            Target::Player(_) => Audience::All,
        };
        Target::Audience(audience.except(entity))
    }
}

#[must_use = "a title request does nothing until `.send()`"]
pub struct TitleRequest<'a> {
    sink: Sink<'a>,
    target: Target,
    title: Option<(String, TextColor)>,
    subtitle: Option<(String, TextColor)>,
    times: Option<SetTitlesAnimation>,
}

impl<'a> TitleRequest<'a> {
    fn new(sink: Sink<'a>, target: Target) -> Self {
        Self {
            sink,
            target,
            title: None,
            subtitle: None,
            times: None,
        }
    }

    pub fn title(mut self, text: impl Into<String>) -> Self {
        let color = self.title.map_or(TextColor::White, |(_, color)| color);
        self.title = Some((text.into(), color));
        self
    }

    /// The client only shows a subtitle while a title is on screen; a request
    /// without a title only updates the stored subtitle.
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        let color = self.subtitle.map_or(TextColor::White, |(_, color)| color);
        self.subtitle = Some((text.into(), color));
        self
    }

    /// Colours the title; use [`subtitle_color`](Self::subtitle_color) for the subtitle.
    pub fn color(mut self, color: TextColor) -> Self {
        if let Some((_, current)) = &mut self.title {
            *current = color;
        }
        self
    }

    pub fn subtitle_color(mut self, color: TextColor) -> Self {
        if let Some((_, current)) = &mut self.subtitle {
            *current = color;
        }
        self
    }

    /// Fade-in, stay and fade-out in ticks; without it the client keeps the
    /// times it last received (10 / 70 / 20 after a reset).
    pub fn times(mut self, fade_in: i32, stay: i32, fade_out: i32) -> Self {
        self.times = Some(SetTitlesAnimation {
            fade_in,
            stay,
            fade_out,
        });
        self
    }

    /// Replaces the target with the ready players the audience selects; the
    /// `player` given to `title`/`subtitle` is discarded and delivery becomes
    /// ready-only.
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

    /// The packets `send()` would send, in wire order.
    pub fn packets(&self) -> Vec<ClientboundPacket> {
        let mut packets = Vec::with_capacity(3);
        if let Some(times) = self.times {
            packets.push(times.into());
        }
        if let Some((text, color)) = &self.subtitle {
            packets.push(
                SetSubtitleText {
                    text: text_component(text, *color),
                }
                .into(),
            );
        }
        if let Some((text, color)) = &self.title {
            packets.push(
                SetTitleText {
                    text: text_component(text, *color),
                }
                .into(),
            );
        }
        packets
    }

    pub fn send(self) {
        self.sink.deliver(&self.target, self.packets());
    }
}

#[must_use = "a clear request does nothing until `.send()`"]
pub struct ClearRequest<'a> {
    sink: Sink<'a>,
    target: Target,
    reset_times: bool,
}

impl<'a> ClearRequest<'a> {
    fn new(sink: Sink<'a>, target: Target, reset_times: bool) -> Self {
        Self {
            sink,
            target,
            reset_times,
        }
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.target = Target::Audience(audience);
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    pub fn except(mut self, entity: Entity) -> Self {
        self.target = self.target.except(entity);
        self
    }

    pub fn packet(&self) -> ClearTitles {
        ClearTitles {
            reset_times: self.reset_times,
        }
    }

    pub fn send(self) {
        self.sink.deliver(&self.target, [self.packet().into()]);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bevy_app::{App, Update};
    use flume::Receiver;
    use ussr_nbt::owned::{Nbt, Tag};
    use voidmc_protocol::clientbound::PlayPacket;

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
            .spawn((ClientId(client_id), PlayerReady, PlayerDimension(dimension)))
            .id()
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Sent {
        Times(i32, i32, i32),
        Subtitle(String, String),
        Title(String, String),
        Clear(bool),
    }

    fn fields(nbt: &Nbt) -> (String, String) {
        let get = |key: &str| match nbt
            .compound
            .tags
            .iter()
            .find(|(name, _)| name.to_string() == key)
        {
            Some((_, Tag::String(value))) => value.to_string(),
            other => panic!("expected string {key}, got {other:?}"),
        };
        (get("text"), get("color"))
    }

    fn classify(packet: ClientboundPacket) -> Sent {
        let ClientboundPacket::Play(packet) = packet else {
            panic!("expected a play packet");
        };
        match packet {
            PlayPacket::SetTitlesAnimation(p) => Sent::Times(p.fade_in, p.stay, p.fade_out),
            PlayPacket::SetSubtitleText(p) => {
                let (text, color) = fields(&p.text);
                Sent::Subtitle(text, color)
            }
            PlayPacket::SetTitleText(p) => {
                let (text, color) = fields(&p.text);
                Sent::Title(text, color)
            }
            PlayPacket::ClearTitles(p) => Sent::Clear(p.reset_times),
            other => panic!("unexpected packet {other:?}"),
        }
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<(u32, Sent)> {
        rx.try_iter()
            .map(|out| (out.client_id, classify(out.packet)))
            .collect()
    }

    fn per_client(sent: Vec<(u32, Sent)>) -> Vec<(u32, Vec<Sent>)> {
        let mut clients: BTreeMap<u32, Vec<Sent>> = BTreeMap::new();
        for (client, packet) in sent {
            clients.entry(client).or_default().push(packet);
        }
        clients.into_iter().collect()
    }

    #[test]
    fn full_title_is_sent_in_vanilla_order() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);

        app.add_systems(Update, move |titles: Titles| {
            titles
                .title(alice, "Round 2")
                .subtitle("Get ready")
                .color(TextColor::Gold)
                .subtitle_color(TextColor::Gray)
                .times(10, 70, 20)
                .send();
        });
        app.update();

        assert_eq!(
            drain(&rx),
            vec![
                (1, Sent::Times(10, 70, 20)),
                (1, Sent::Subtitle("Get ready".into(), "gray".into())),
                (1, Sent::Title("Round 2".into(), "gold".into())),
            ]
        );
    }

    #[test]
    fn optional_parts_are_omitted_and_colours_default_to_white() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        let joining = app.world_mut().spawn(ClientId(2)).id();

        app.add_systems(Update, move |titles: Titles| {
            titles.title(alice, "only").send();
            titles.subtitle(alice, "sub only").send();
            titles.title(joining, "welcome").times(0, 20, 5).send();
            titles
                .subtitle(alice, "late title")
                .title("t")
                .color(TextColor::Red)
                .send();
            let _abandoned = titles.title(alice, "never sent");
        });
        app.update();

        assert_eq!(
            drain(&rx),
            vec![
                (1, Sent::Title("only".into(), "white".into())),
                (1, Sent::Subtitle("sub only".into(), "white".into())),
                (2, Sent::Times(0, 20, 5)),
                (2, Sent::Title("welcome".into(), "white".into())),
                (1, Sent::Subtitle("late title".into(), "white".into())),
                (1, Sent::Title("t".into(), "red".into())),
            ]
        );
    }

    #[test]
    fn colour_before_text_is_kept_and_without_text_is_ignored() {
        let (app, _rx) = test_app();
        let titles = WorldTitles::new(app.world());
        let request = titles
            .broadcast("x")
            .color(TextColor::Aqua)
            .subtitle_color(TextColor::Blue)
            .title("y");
        let packets: Vec<Sent> = request.packets().into_iter().map(classify).collect();
        assert_eq!(packets, vec![Sent::Title("y".into(), "aqua".into())]);
    }

    #[test]
    fn broadcasts_reach_every_ready_player_in_order() {
        let (mut app, rx) = test_app();
        ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Nether);
        app.world_mut().spawn(ClientId(3));

        app.add_systems(Update, |titles: Titles| {
            titles
                .broadcast("GO!")
                .subtitle("now")
                .times(0, 20, 5)
                .send();
        });
        app.update();

        let expected = || {
            vec![
                Sent::Times(0, 20, 5),
                Sent::Subtitle("now".into(), "white".into()),
                Sent::Title("GO!".into(), "white".into()),
            ]
        };
        assert_eq!(
            per_client(drain(&rx)),
            vec![(1, expected()), (2, expected())]
        );
    }

    #[test]
    fn audience_viewers_and_except_replace_the_target() {
        let (mut app, rx) = test_app();
        let overworld = ready_player(&mut app, 1, DimensionId::Overworld);
        let nether = ready_player(&mut app, 2, DimensionId::Nether);
        let end = ready_player(&mut app, 3, DimensionId::End);
        let joining = app.world_mut().spawn(ClientId(4)).id();

        app.add_systems(Update, move |titles: Titles| {
            titles
                .broadcast("nether")
                .audience(Audience::InDimension(DimensionId::Nether))
                .send();
            titles
                .title(overworld, "party")
                .viewers([end, joining])
                .send();
            titles.broadcast("not you").except(nether).send();
            titles.title(end, "all but end").except(end).send();
            titles
                .clear(overworld)
                .viewers([nether])
                .except(nether)
                .send();
        });
        app.update();

        assert_eq!(
            per_client(drain(&rx)),
            vec![
                (
                    1,
                    vec![
                        Sent::Title("not you".into(), "white".into()),
                        Sent::Title("all but end".into(), "white".into()),
                    ]
                ),
                (
                    2,
                    vec![
                        Sent::Title("nether".into(), "white".into()),
                        Sent::Title("all but end".into(), "white".into()),
                    ]
                ),
                (
                    3,
                    vec![
                        Sent::Title("party".into(), "white".into()),
                        Sent::Title("not you".into(), "white".into()),
                    ]
                ),
            ]
        );
    }

    #[test]
    fn clear_and_reset_set_the_reset_flag() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Overworld);
        let joining = app.world_mut().spawn(ClientId(3)).id();

        app.add_systems(Update, move |titles: Titles| {
            titles.clear(alice).send();
            titles.reset(joining).send();
            titles.reset(alice).audience(Audience::All).send();
        });
        app.update();

        assert_eq!(
            per_client(drain(&rx)),
            vec![
                (1, vec![Sent::Clear(false), Sent::Clear(true)]),
                (2, vec![Sent::Clear(true)]),
                (3, vec![Sent::Clear(true)]),
            ]
        );
    }

    #[test]
    fn world_titles_twin_matches() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Overworld);

        let titles = WorldTitles::new(app.world());
        titles.subtitle(alice, "twin").send();
        titles.title(alice, "one").send();
        titles.broadcast("all").times(1, 2, 3).send();
        titles.clear(alice).send();
        titles.reset(alice).send();

        assert_eq!(
            per_client(drain(&rx)),
            vec![
                (
                    1,
                    vec![
                        Sent::Subtitle("twin".into(), "white".into()),
                        Sent::Title("one".into(), "white".into()),
                        Sent::Times(1, 2, 3),
                        Sent::Title("all".into(), "white".into()),
                        Sent::Clear(false),
                        Sent::Clear(true),
                    ]
                ),
                (
                    2,
                    vec![
                        Sent::Times(1, 2, 3),
                        Sent::Title("all".into(), "white".into())
                    ]
                ),
            ]
        );
    }

    #[test]
    fn oversized_title_and_subtitle_are_guarded() {
        let (app, _rx) = test_app();
        let titles = WorldTitles::new(app.world());
        let text = "😀".repeat(11000);
        let packets = titles
            .broadcast(text.clone())
            .subtitle(text.clone())
            .packets();
        assert_eq!(packets.len(), 2);
        for packet in packets {
            let nbt = match packet {
                ClientboundPacket::Play(PlayPacket::SetSubtitleText(p)) => p.text,
                ClientboundPacket::Play(PlayPacket::SetTitleText(p)) => p.text,
                other => panic!("unexpected packet {other:?}"),
            };
            assert_guarded(&nbt, &text);
        }
    }
}
