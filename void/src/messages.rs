//! Fire-and-forget text messages: `messages.message(player, "hi").send()` for
//! system chat, `.action_bar(..)` for the overlay line above the hotbar, and
//! `.broadcast(..)` / `.broadcast_action_bar(..)` for every ready player. The
//! [`SystemChat`] packet is built here so call sites never touch NBT.

use std::fmt;

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use tracing::warn;
#[cfg(test)]
use ussr_nbt::owned::{Compound, List};
use ussr_nbt::owned::{Nbt, Tag};
use voidmc_codec::nbt::{fitting_len, modified_utf8_width};
use voidmc_protocol::clientbound::SystemChat;

use crate::players::{Audience, Players, Recipients, WorldPlayers};

pub(crate) const MAX_TEXT_BYTES: usize = voidmc_codec::nbt::MAX_STRING_BYTES;

/// Only these names and `#rrggbb` decode on the client; anything else
/// disconnects the recipient with a `DecoderException`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextColor {
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    #[default]
    White,
    Rgb(u32),
}

impl TextColor {
    pub const NAMED: [TextColor; 16] = [
        TextColor::Black,
        TextColor::DarkBlue,
        TextColor::DarkGreen,
        TextColor::DarkAqua,
        TextColor::DarkRed,
        TextColor::DarkPurple,
        TextColor::Gold,
        TextColor::Gray,
        TextColor::DarkGray,
        TextColor::Blue,
        TextColor::Green,
        TextColor::Aqua,
        TextColor::Red,
        TextColor::LightPurple,
        TextColor::Yellow,
        TextColor::White,
    ];

    pub const fn rgb(value: u32) -> Self {
        TextColor::Rgb(value & 0xFF_FFFF)
    }

    pub fn name(self) -> Option<&'static str> {
        Some(match self {
            TextColor::Black => "black",
            TextColor::DarkBlue => "dark_blue",
            TextColor::DarkGreen => "dark_green",
            TextColor::DarkAqua => "dark_aqua",
            TextColor::DarkRed => "dark_red",
            TextColor::DarkPurple => "dark_purple",
            TextColor::Gold => "gold",
            TextColor::Gray => "gray",
            TextColor::DarkGray => "dark_gray",
            TextColor::Blue => "blue",
            TextColor::Green => "green",
            TextColor::Aqua => "aqua",
            TextColor::Red => "red",
            TextColor::LightPurple => "light_purple",
            TextColor::Yellow => "yellow",
            TextColor::White => "white",
            TextColor::Rgb(_) => return None,
        })
    }

    pub fn legacy_code(self) -> Option<char> {
        let index = TextColor::NAMED.iter().position(|named| *named == self)?;
        char::from_digit(index as u32, 16)
    }

    pub fn parse(color: &str) -> Option<Self> {
        if let Some(hex) = color.strip_prefix('#') {
            if hex.is_empty() || hex.len() > 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            return u32::from_str_radix(hex, 16).ok().map(TextColor::Rgb);
        }
        TextColor::NAMED
            .into_iter()
            .find(|named| named.name() == Some(color))
    }

    pub fn parse_or_white(color: &str) -> Self {
        TextColor::parse(color).unwrap_or_else(|| {
            warn!(color, "invalid text colour, falling back to white");
            TextColor::White
        })
    }
}

impl fmt::Display for TextColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TextColor::Rgb(value) => write!(f, "#{:06x}", value & 0xFF_FFFF),
            named => f.write_str(named.name().unwrap_or_default()),
        }
    }
}

#[derive(SystemParam)]
pub struct Messages<'w, 's> {
    players: Players<'w, 's>,
}

impl Messages<'_, '_> {
    fn request(
        &self,
        target: Target,
        text: impl Into<String>,
        overlay: bool,
    ) -> MessageRequest<'_> {
        MessageRequest::new(Sink::Players(&self.players), target, text, overlay)
    }

    pub fn message(&self, player: Entity, text: impl Into<String>) -> MessageRequest<'_> {
        self.request(Target::Player(player), text, false)
    }

    pub fn action_bar(&self, player: Entity, text: impl Into<String>) -> MessageRequest<'_> {
        self.request(Target::Player(player), text, true)
    }

    pub fn broadcast(&self, text: impl Into<String>) -> MessageRequest<'_> {
        self.request(Target::Audience(Audience::All), text, false)
    }

    pub fn broadcast_action_bar(&self, text: impl Into<String>) -> MessageRequest<'_> {
        self.request(Target::Audience(Audience::All), text, true)
    }
}

pub struct WorldMessages<'w> {
    players: WorldPlayers<'w>,
}

impl<'w> WorldMessages<'w> {
    pub fn new(world: &'w World) -> Self {
        Self {
            players: WorldPlayers::new(world),
        }
    }

    fn request(
        &self,
        target: Target,
        text: impl Into<String>,
        overlay: bool,
    ) -> MessageRequest<'_> {
        MessageRequest::new(Sink::World(&self.players), target, text, overlay)
    }

    pub fn message(&self, player: Entity, text: impl Into<String>) -> MessageRequest<'_> {
        self.request(Target::Player(player), text, false)
    }

    pub fn action_bar(&self, player: Entity, text: impl Into<String>) -> MessageRequest<'_> {
        self.request(Target::Player(player), text, true)
    }

    pub fn broadcast(&self, text: impl Into<String>) -> MessageRequest<'_> {
        self.request(Target::Audience(Audience::All), text, false)
    }

    pub fn broadcast_action_bar(&self, text: impl Into<String>) -> MessageRequest<'_> {
        self.request(Target::Audience(Audience::All), text, true)
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

    fn send(&self, player: Entity, packet: SystemChat) {
        match self {
            Sink::Players(players) => players.send(player, packet),
            Sink::World(players) => players.send(player, packet),
        }
    }
}

pub(crate) enum Target {
    Player(Entity),
    Audience(Audience),
}

impl Target {
    pub(crate) fn except(self, entity: Entity) -> Self {
        let audience = match self {
            Target::Audience(audience) => audience,
            Target::Player(_) => Audience::All,
        };
        Target::Audience(audience.except(entity))
    }
}

#[must_use = "a message request does nothing until `.send()`"]
pub struct MessageRequest<'a> {
    sink: Sink<'a>,
    target: Target,
    text: String,
    color: TextColor,
    overlay: bool,
}

impl<'a> MessageRequest<'a> {
    fn new(sink: Sink<'a>, target: Target, text: impl Into<String>, overlay: bool) -> Self {
        Self {
            sink,
            target,
            text: text.into(),
            color: TextColor::White,
            overlay,
        }
    }

    pub fn color(mut self, color: TextColor) -> Self {
        self.color = color;
        self
    }

    /// Replaces the target with the ready players the audience selects; the
    /// `player` given to `message`/`action_bar` is discarded and delivery
    /// becomes ready-only.
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

    pub fn packet(&self) -> SystemChat {
        SystemChat {
            content: Text::truncated(self.text.as_str()).component(self.color),
            overlay: self.overlay,
        }
    }

    pub fn send(self) {
        let packet = self.packet();
        match &self.target {
            Target::Player(player) => self.sink.send(*player, packet),
            Target::Audience(audience) => audience.resolve(self.sink.ready()).send(packet),
        }
    }
}

/// A text the client decodes: at most [`Text::MAX_BYTES`] in modified UTF-8, the NBT string
/// limit. Longer, the client drops the connection. Only [`Text::new`] and
/// [`Text::truncated`] make one:
///
/// ```compile_fail
/// let text = voidmc::Text(String::from("built around the check"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Text(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextTooLong {
    pub bytes: usize,
}

impl fmt::Display for TextTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "text takes {} modified UTF-8 bytes, the client decodes at most {}",
            self.bytes,
            Text::MAX_BYTES
        )
    }
}

impl std::error::Error for TextTooLong {}

impl Text {
    pub const MAX_BYTES: usize = MAX_TEXT_BYTES;

    pub fn new(text: impl Into<String>) -> Result<Self, TextTooLong> {
        let text = text.into();
        if fitting_len(&text) == text.len() {
            Ok(Self(text))
        } else {
            Err(TextTooLong {
                bytes: modified_utf8_len(&text),
            })
        }
    }

    /// Keeps what fits, cut on a character boundary, and logs what it cut.
    pub fn truncated(text: impl Into<String>) -> Self {
        let mut text = text.into();
        let end = fitting_len(&text);
        if end < text.len() {
            warn!(
                bytes = modified_utf8_len(&text),
                limit = MAX_TEXT_BYTES,
                kept = modified_utf8_len(&text[..end]),
                "text exceeds the NBT string limit, truncating"
            );
            text.truncate(end);
        }
        Self(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    pub fn component(&self, color: TextColor) -> Nbt {
        component(&self.0, Some(color))
    }

    pub fn plain(&self) -> Nbt {
        component(&self.0, None)
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn component(text: &str, color: Option<TextColor>) -> Nbt {
    let mut compound = vec![("text".into(), Tag::String(text.into()))];
    if let Some(color) = color {
        compound.push(("color".into(), Tag::String(color.to_string().into())));
    }
    Nbt {
        name: "".into(),
        compound: compound.into(),
    }
}

fn modified_utf8_len(text: &str) -> usize {
    text.chars().map(modified_utf8_width).sum()
}

#[cfg(test)]
fn decode_wire(bytes: &[u8]) -> (Nbt, usize) {
    let opts = ussr_nbt::ReadOpts {
        name: false,
        ..ussr_nbt::ReadOpts::new()
    };
    let mut reader = bytes;
    let decoded = Nbt::read_with_opts(&mut reader, opts).expect("wire NBT decodes");
    (decoded, reader.len())
}

#[cfg(test)]
fn collect_strings(compound: &Compound, out: &mut Vec<String>) {
    fn walk_list(list: &List, out: &mut Vec<String>) {
        match list {
            List::String(values) => out.extend(values.iter().map(ToString::to_string)),
            List::List(lists) => lists.iter().for_each(|list| walk_list(list, out)),
            List::Compound(compounds) => compounds
                .iter()
                .for_each(|compound| collect_strings(compound, out)),
            _ => {}
        }
    }
    for (_, tag) in &compound.tags {
        match tag {
            Tag::String(value) => out.push(value.to_string()),
            Tag::List(list) => walk_list(list, out),
            Tag::Compound(inner) => collect_strings(inner, out),
            _ => {}
        }
    }
}

#[cfg(test)]
pub(crate) fn assert_guarded(nbt: &Nbt, text: &str) {
    let mut strings = Vec::new();
    collect_strings(&nbt.compound, &mut strings);
    let built = strings
        .into_iter()
        .max_by_key(String::len)
        .expect("a string field");
    assert!(built.chars().map(modified_utf8_width).sum::<usize>() <= MAX_TEXT_BYTES);
    assert!(built.len() < text.len());
    assert!(text.starts_with(&built));
}

#[cfg(test)]
fn decode_text(bytes: &[u8]) -> (usize, String) {
    let len = u16::from_be_bytes([bytes[8], bytes[9]]) as usize;
    let (decoded, _) = decode_wire(bytes);
    let text = decoded
        .compound
        .tags
        .iter()
        .find(|(name, _)| name.to_string() == "text")
        .map(|(_, tag)| match tag {
            Tag::String(value) => value.to_string(),
            other => panic!("unexpected tag {other:?}"),
        })
        .expect("text field");
    (len, text)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_app::{App, Update};
    use flume::Receiver;
    use voidmc_codec::Encode;
    use voidmc_protocol::clientbound;

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
        });
        app.insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx));
        (app, outgoing_rx)
    }

    fn ready_player(app: &mut App, client_id: u32, dimension: DimensionId) -> Entity {
        app.world_mut()
            .spawn((ClientId(client_id), PlayerReady, PlayerDimension(dimension)))
            .id()
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<(u32, SystemChat)> {
        rx.try_iter()
            .map(|out| {
                let clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SystemChat(
                    packet,
                )) = out.packet
                else {
                    panic!("expected system chat");
                };
                (out.client_id, packet)
            })
            .collect()
    }

    fn client_ids(sent: &[(u32, SystemChat)]) -> HashSet<u32> {
        sent.iter().map(|(id, _)| *id).collect()
    }

    fn field(packet: &SystemChat, key: &str) -> String {
        match packet
            .content
            .compound
            .tags
            .iter()
            .find(|(name, _)| name.to_string() == key)
        {
            Some((_, Tag::String(value))) => value.to_string(),
            other => panic!("expected string {key}, got {other:?}"),
        }
    }

    #[test]
    fn a_text_the_client_cannot_decode_is_refused() {
        for (text, bytes) in [
            ("é".repeat(40000), 80000),
            (
                "😀".repeat(MAX_TEXT_BYTES / 6 + 1),
                (MAX_TEXT_BYTES / 6 + 1) * 6,
            ),
            (
                "\0".repeat(MAX_TEXT_BYTES / 2 + 1),
                (MAX_TEXT_BYTES / 2 + 1) * 2,
            ),
        ] {
            assert_eq!(Text::new(text.as_str()), Err(TextTooLong { bytes }));
            let cut = Text::truncated(text.as_str());
            assert!(text.starts_with(cut.as_str()));
            assert!(cut.as_str().chars().map(modified_utf8_width).sum::<usize>() <= MAX_TEXT_BYTES);
        }
        let fits = "a".repeat(MAX_TEXT_BYTES);
        assert_eq!(Text::new(fits.as_str()).unwrap().as_str(), fits);
        assert_eq!(Text::truncated(fits.as_str()).into_string(), fits);
    }

    #[test]
    fn a_text_is_cut_before_the_encoder_sees_it() {
        let text = "😀".repeat(11000);
        let error = Text::new(text.as_str()).unwrap_err();
        assert!(error.to_string().contains("66000"), "{error}");
        assert_guarded(
            &Text::truncated(text.as_str()).component(TextColor::White),
            &text,
        );
    }

    #[test]
    fn legacy_codes_follow_the_vanilla_order() {
        assert_eq!(TextColor::Black.legacy_code(), Some('0'));
        assert_eq!(TextColor::Gold.legacy_code(), Some('6'));
        assert_eq!(TextColor::Green.legacy_code(), Some('a'));
        assert_eq!(TextColor::White.legacy_code(), Some('f'));
        assert_eq!(TextColor::rgb(0x123456).legacy_code(), None);
    }

    #[test]
    fn message_and_action_bar_target_one_player_with_the_right_overlay() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        let bob = ready_player(&mut app, 2, DimensionId::Overworld);
        let joining = app.world_mut().spawn(ClientId(3)).id();

        app.add_systems(Update, move |messages: Messages| {
            messages.message(alice, "hello").send();
            messages
                .action_bar(bob, "above the hotbar")
                .color(TextColor::Gold)
                .send();
            messages.message(joining, "welcome").send();
        });
        app.update();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[0].0, 1);
        assert!(!sent[0].1.overlay);
        assert_eq!(field(&sent[0].1, "text"), "hello");
        assert_eq!(field(&sent[0].1, "color"), "white");
        assert_eq!(sent[1].0, 2);
        assert!(sent[1].1.overlay);
        assert_eq!(field(&sent[1].1, "text"), "above the hotbar");
        assert_eq!(field(&sent[1].1, "color"), "gold");
        assert_eq!(sent[2].0, 3);
    }

    #[test]
    fn broadcasts_reach_every_ready_player() {
        let (mut app, rx) = test_app();
        ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Nether);
        app.world_mut().spawn(ClientId(3));

        app.add_systems(Update, |messages: Messages| {
            messages.broadcast("chat").send();
            messages.broadcast_action_bar("bar").send();
        });
        app.update();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 4);
        assert_eq!(client_ids(&sent[..2]), HashSet::from([1, 2]));
        assert!(sent[..2].iter().all(|(_, p)| !p.overlay));
        assert_eq!(client_ids(&sent[2..]), HashSet::from([1, 2]));
        assert!(sent[2..].iter().all(|(_, p)| p.overlay));
        assert_eq!(field(&sent[2].1, "text"), "bar");
    }

    #[test]
    fn audience_and_viewers_replace_the_target() {
        let (mut app, rx) = test_app();
        let overworld = ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Nether);
        let end = ready_player(&mut app, 3, DimensionId::End);

        app.add_systems(Update, move |messages: Messages| {
            messages
                .broadcast("nether only")
                .audience(Audience::InDimension(DimensionId::Nether))
                .send();
            messages.message(overworld, "party").viewers([end]).send();
            messages.broadcast("not you").except(overworld).send();
            messages.message(end, "everyone but end").except(end).send();
            let _abandoned = messages.broadcast("never sent");
        });
        app.update();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 6);
        assert_eq!(sent[0].0, 2);
        assert_eq!(sent[1].0, 3);
        assert_eq!(field(&sent[1].1, "text"), "party");
        assert_eq!(client_ids(&sent[2..4]), HashSet::from([2, 3]));
        assert_eq!(field(&sent[2].1, "text"), "not you");
        assert_eq!(client_ids(&sent[4..]), HashSet::from([1, 2]));
        assert_eq!(field(&sent[4].1, "text"), "everyone but end");
    }

    #[test]
    fn world_messages_twin_matches() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Overworld);

        let messages = WorldMessages::new(app.world());
        messages.action_bar(alice, "twin").send();
        messages.broadcast("all").color(TextColor::Red).send();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[0].0, 1);
        assert!(sent[0].1.overlay);
        assert_eq!(client_ids(&sent[1..]), HashSet::from([1, 2]));
        assert_eq!(field(&sent[1].1, "color"), "red");
    }

    #[test]
    fn named_colors_round_trip_through_their_vanilla_names() {
        let names = [
            "black",
            "dark_blue",
            "dark_green",
            "dark_aqua",
            "dark_red",
            "dark_purple",
            "gold",
            "gray",
            "dark_gray",
            "blue",
            "green",
            "aqua",
            "red",
            "light_purple",
            "yellow",
            "white",
        ];
        for (color, name) in TextColor::NAMED.into_iter().zip(names) {
            assert_eq!(color.to_string(), name);
            assert_eq!(TextColor::parse(name), Some(color));
        }
        assert_eq!(TextColor::parse("Gold"), None);
        assert_eq!(TextColor::parse("grey"), None);
        assert_eq!(TextColor::parse(""), None);
        assert_eq!(TextColor::parse("#"), None);
        assert_eq!(TextColor::parse("#1000000"), None);
        assert_eq!(TextColor::parse("#gg0000"), None);
    }

    #[test]
    fn rgb_colors_serialise_as_hex_and_mask_to_24_bits() {
        assert_eq!(TextColor::Rgb(0xff8800).to_string(), "#ff8800");
        assert_eq!(TextColor::Rgb(0x000001).to_string(), "#000001");
        assert_eq!(TextColor::Rgb(0xffff8800).to_string(), "#ff8800");
        assert_eq!(TextColor::rgb(0xffff8800), TextColor::Rgb(0xff8800));
        assert_eq!(TextColor::parse("#ff8800"), Some(TextColor::Rgb(0xff8800)));
        assert_eq!(TextColor::parse("#FF8800"), Some(TextColor::Rgb(0xff8800)));
        assert_eq!(TextColor::parse("#ff0"), Some(TextColor::Rgb(0xff0)));
        assert_eq!(TextColor::parse("#ffffff"), Some(TextColor::Rgb(0xffffff)));
        assert_eq!(
            field(&system_chat_packet("x", TextColor::rgb(0xabcdef)), "color"),
            "#abcdef"
        );
    }

    fn system_chat_packet(text: &str, color: TextColor) -> SystemChat {
        let (app, _rx) = test_app();
        WorldMessages::new(app.world())
            .broadcast(text)
            .color(color)
            .packet()
    }

    #[test]
    fn string_entry_points_fall_back_to_white_instead_of_kicking_the_client() {
        let nbt = crate::commands::text_to_nbt("hi", "Gold");
        let packet = SystemChat {
            content: nbt,
            overlay: false,
        };
        assert_eq!(field(&packet, "color"), "white");
        assert_eq!(
            field(&crate::commands::system_chat("hi", "gold"), "color"),
            "gold"
        );
        assert_eq!(TextColor::parse_or_white("#12345"), TextColor::Rgb(0x12345));

        let long = "😀".repeat(11000);
        assert_guarded(&crate::commands::text_to_nbt(&long, "gold"), &long);
        assert_guarded(&crate::commands::system_chat(&long, "gold").content, &long);
    }

    fn text_round_trips(packet: &SystemChat) -> (usize, String) {
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        decode_text(&bytes)
    }

    #[test]
    fn oversized_text_is_cut_on_a_char_boundary_below_the_nbt_limit() {
        let text = "é".repeat(40000);
        let packet = system_chat_packet(&text, TextColor::White);
        let sent = field(&packet, "text");
        assert_eq!(sent.len(), MAX_TEXT_BYTES - 1);
        assert!(sent.chars().all(|c| c == 'é'));

        let (len, decoded) = text_round_trips(&packet);
        assert_eq!(len, MAX_TEXT_BYTES - 1);
        assert_eq!(decoded, sent);

        let exact = "a".repeat(MAX_TEXT_BYTES);
        assert_eq!(
            field(&system_chat_packet(&exact, TextColor::White), "text").len(),
            MAX_TEXT_BYTES
        );
    }

    #[test]
    fn oversized_text_is_cut_on_the_modified_utf8_width() {
        for text in ["😀".repeat(11000), "a\0".repeat(30000)] {
            let packet = system_chat_packet(&text, TextColor::White);
            let sent = field(&packet, "text");
            assert!(sent.len() < text.len());
            assert!(text.starts_with(&sent));
            assert!(sent.chars().map(modified_utf8_width).sum::<usize>() <= MAX_TEXT_BYTES);

            let (len, decoded) = text_round_trips(&packet);
            assert!(len <= MAX_TEXT_BYTES);
            assert_eq!(decoded, sent);
        }

        let emoji = "😀".repeat(MAX_TEXT_BYTES / 6);
        assert_eq!(
            field(&system_chat_packet(&emoji, TextColor::White), "text").len(),
            emoji.len()
        );
        let (len, decoded) = text_round_trips(&system_chat_packet(&emoji, TextColor::White));
        assert_eq!(len, MAX_TEXT_BYTES / 6 * 6);
        assert_eq!(decoded, emoji);
    }

    #[test]
    fn plain_component_has_no_color_and_is_truncated() {
        let nbt = Text::truncated("hi").plain();
        assert_eq!(nbt.compound.tags.len(), 1);
        let mut bytes = Vec::new();
        nbt.encode(&mut bytes);
        assert_eq!(
            bytes,
            [
                &[0x0A, 0x08, 0x00, 0x04][..],
                b"text",
                &[0x00, 0x02],
                b"hi",
                &[0x00]
            ]
            .concat()
        );

        let text = "😀".repeat(11000);
        assert_guarded(&Text::truncated(text.as_str()).plain(), &text);
    }

    #[test]
    fn system_chat_wire_bytes_match_paper() {
        let mut bytes = Vec::new();
        system_chat_packet("hi", TextColor::Gold).encode(&mut bytes);
        let expected: Vec<u8> = [
            &[0x0A][..],
            &[0x08, 0x00, 0x04],
            b"text",
            &[0x00, 0x02],
            b"hi",
            &[0x08, 0x00, 0x05],
            b"color",
            &[0x00, 0x04],
            b"gold",
            &[0x00],
            &[0x00],
        ]
        .concat();
        assert_eq!(bytes, expected);

        let mut overlay = Vec::new();
        SystemChat {
            content: Text::default().component(TextColor::rgb(0xff8800)),
            overlay: true,
        }
        .encode(&mut overlay);
        assert_eq!(&overlay[overlay.len() - 9..], b"#ff8800\x00\x01");
    }
}
