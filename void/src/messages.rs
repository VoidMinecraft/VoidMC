//! Fire-and-forget text messages: `messages.message(player, "hi").send()` for
//! system chat, `.action_bar(..)` for the overlay line above the hotbar, and
//! `.broadcast(..)` / `.broadcast_action_bar(..)` for every ready player. The
//! [`SystemChat`] packet is built here so call sites never touch NBT.

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use ussr_nbt::owned::{Nbt, Tag};
use voidmc_protocol::clientbound::SystemChat;

use crate::players::{Audience, Players, Recipients, WorldPlayers};

const DEFAULT_COLOR: &str = "white";

#[derive(SystemParam)]
pub struct Messages<'w, 's> {
    players: Players<'w, 's>,
}

impl Messages<'_, '_> {
    pub fn message(&self, player: Entity, text: impl Into<String>) -> MessageRequest<'_> {
        MessageRequest::new(
            Sink::Players(&self.players),
            Target::Player(player),
            text,
            false,
        )
    }

    pub fn action_bar(&self, player: Entity, text: impl Into<String>) -> MessageRequest<'_> {
        MessageRequest::new(
            Sink::Players(&self.players),
            Target::Player(player),
            text,
            true,
        )
    }

    pub fn broadcast(&self, text: impl Into<String>) -> MessageRequest<'_> {
        MessageRequest::new(
            Sink::Players(&self.players),
            Target::Audience(Audience::All),
            text,
            false,
        )
    }

    pub fn broadcast_action_bar(&self, text: impl Into<String>) -> MessageRequest<'_> {
        MessageRequest::new(
            Sink::Players(&self.players),
            Target::Audience(Audience::All),
            text,
            true,
        )
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

    pub fn message(&self, player: Entity, text: impl Into<String>) -> MessageRequest<'_> {
        MessageRequest::new(
            Sink::World(&self.players),
            Target::Player(player),
            text,
            false,
        )
    }

    pub fn action_bar(&self, player: Entity, text: impl Into<String>) -> MessageRequest<'_> {
        MessageRequest::new(
            Sink::World(&self.players),
            Target::Player(player),
            text,
            true,
        )
    }

    pub fn broadcast(&self, text: impl Into<String>) -> MessageRequest<'_> {
        MessageRequest::new(
            Sink::World(&self.players),
            Target::Audience(Audience::All),
            text,
            false,
        )
    }

    pub fn broadcast_action_bar(&self, text: impl Into<String>) -> MessageRequest<'_> {
        MessageRequest::new(
            Sink::World(&self.players),
            Target::Audience(Audience::All),
            text,
            true,
        )
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

enum Target {
    Player(Entity),
    Audience(Audience),
}

#[must_use = "a message request does nothing until `.send()`"]
pub struct MessageRequest<'a> {
    sink: Sink<'a>,
    target: Target,
    text: String,
    color: String,
    overlay: bool,
}

impl<'a> MessageRequest<'a> {
    fn new(sink: Sink<'a>, target: Target, text: impl Into<String>, overlay: bool) -> Self {
        Self {
            sink,
            target,
            text: text.into(),
            color: DEFAULT_COLOR.to_string(),
            overlay,
        }
    }

    /// A named colour (`"red"`, `"gray"`, …) or `"#rrggbb"`.
    pub fn color(mut self, color: impl Into<String>) -> Self {
        self.color = color.into();
        self
    }

    /// Replaces the target with the ready players the audience selects.
    pub fn audience(mut self, audience: Audience) -> Self {
        self.target = Target::Audience(audience);
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    pub fn packet(&self) -> SystemChat {
        SystemChat {
            content: text_component(&self.text, &self.color),
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

pub fn text_component(text: &str, color: &str) -> Nbt {
    Nbt {
        name: "".into(),
        compound: vec![
            ("text".into(), Tag::String(text.into())),
            ("color".into(), Tag::String(color.into())),
        ]
        .into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_app::{App, Update};
    use flume::Receiver;
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
    fn message_and_action_bar_target_one_player_with_the_right_overlay() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        let bob = ready_player(&mut app, 2, DimensionId::Overworld);
        let joining = app.world_mut().spawn(ClientId(3)).id();

        app.add_systems(Update, move |messages: Messages| {
            messages.message(alice, "hello").send();
            messages
                .action_bar(bob, "above the hotbar")
                .color("gold")
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
            let _abandoned = messages.broadcast("never sent");
        });
        app.update();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, 2);
        assert_eq!(sent[1].0, 3);
        assert_eq!(field(&sent[1].1, "text"), "party");
    }

    #[test]
    fn world_messages_twin_matches() {
        let (mut app, rx) = test_app();
        let alice = ready_player(&mut app, 1, DimensionId::Overworld);
        ready_player(&mut app, 2, DimensionId::Overworld);

        let messages = WorldMessages::new(app.world());
        messages.action_bar(alice, "twin").send();
        messages.broadcast("all").color("red").send();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[0].0, 1);
        assert!(sent[0].1.overlay);
        assert_eq!(client_ids(&sent[1..]), HashSet::from([1, 2]));
        assert_eq!(field(&sent[1].1, "color"), "red");
    }
}
