use bevy_app::App;
use bevy_ecs::prelude::*;
use flume::Receiver;
use ussr_nbt::owned::Tag;
use voidmc::commands::dispatch_command;
use voidmc::components::{ClientId, Operator, PlayerName, PlayerReady};
use voidmc::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
use voidmc::plugins::scoreboard::ScoreboardPlugin;
use voidmc::{CommandRegistry, Team};
use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

use crate::{Access, Scoreboard, VanillaCommandsPlugin};

pub struct Harness {
    pub app: App,
    rx: Receiver<OutgoingPacket>,
    pub alice: Entity,
    pub bob: Entity,
}

pub struct Run {
    pub replies: Vec<String>,
    pub packets: Vec<(u32, PlayPacket)>,
}

impl Run {
    pub fn reply(&self) -> &str {
        assert_eq!(self.replies.len(), 1, "{:?}", self.replies);
        &self.replies[0]
    }
}

fn text_of(nbt: &ussr_nbt::owned::Nbt) -> String {
    nbt.compound
        .tags
        .iter()
        .find(|(key, _)| key.to_string() == "text")
        .and_then(|(_, value)| match value {
            Tag::String(text) => Some(text.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

impl Harness {
    pub fn new() -> Self {
        Self::with_access(Access::Operators)
    }

    pub fn with_access(access: Access) -> Self {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, rx) = flume::unbounded::<OutgoingPacket>();
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
        .init_resource::<CommandRegistry>()
        .add_plugins((ScoreboardPlugin, VanillaCommandsPlugin { access }));
        let alice = app
            .world_mut()
            .spawn((
                ClientId(1),
                PlayerReady,
                PlayerName("Alice".into()),
                Operator,
            ))
            .id();
        let bob = app
            .world_mut()
            .spawn((ClientId(2), PlayerReady, PlayerName("Bob".into())))
            .id();
        let mut harness = Self {
            app,
            rx,
            alice,
            bob,
        };
        harness.app.update();
        harness.rx.drain();
        harness
    }

    pub fn run_as(&mut self, player: Entity, line: &str) -> Run {
        let mut tokens = line.split_whitespace().map(String::from);
        let name = tokens.next().unwrap();
        dispatch_command(self.app.world_mut(), 0, player, &name, tokens.collect());
        self.app.update();
        let mut run = Run {
            replies: Vec::new(),
            packets: Vec::new(),
        };
        for out in self.rx.drain() {
            let ClientboundPacket::Play(packet) = out.packet else {
                continue;
            };
            match packet {
                PlayPacket::SystemChat(chat) if out.client_id == self.client(player) => {
                    run.replies.push(text_of(&chat.content))
                }
                PlayPacket::SystemChat(_) => {}
                packet => run.packets.push((out.client_id, packet)),
            }
        }
        run
    }

    pub fn run(&mut self, line: &str) -> Run {
        self.run_as(self.alice, line)
    }

    pub fn ok(&mut self, line: &str) -> Run {
        let run = self.run(line);
        assert_eq!(run.replies.len(), 1, "{line}: {:?}", run.replies);
        run
    }

    fn client(&self, player: Entity) -> u32 {
        self.app.world().get::<ClientId>(player).unwrap().0
    }

    pub fn board(&self) -> &Scoreboard {
        self.app.world().resource::<Scoreboard>()
    }

    pub fn team(&mut self, name: &str) -> Option<Team> {
        let world = self.app.world_mut();
        world
            .query::<&Team>()
            .iter(world)
            .find(|team| team.name == name)
            .cloned()
    }
}
