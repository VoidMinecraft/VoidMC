use std::collections::{BTreeSet, HashMap, HashSet};

use bevy_ecs::prelude::*;
use voidmc::components::{Position, SpawnedEntity};
use voidmc::{Command, CommandBuilder, EnumArg, Inventory, ItemId, ItemStack as ServerItemStack};
use voidmc_e2e::brigadier::suggestion::Suggestions;
use voidmc_e2e::core::position::Vec3;
use voidmc_e2e::inventory::ItemStack;
use voidmc_e2e::protocol::packets::ProtocolPacket;
use voidmc_e2e::protocol::packets::game::c_commands::{BrigadierNodeStub, NodeType};
use voidmc_e2e::protocol::packets::game::c_game_event::EventType;
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, s_command_suggestion::ServerboundCommandSuggestion,
};
use voidmc_e2e::registry::builtin::ItemKind;
use voidmc_e2e::{Bot, TestServer};

fn assert_valid_tree(nodes: &[BrigadierNodeStub], root: usize) {
    assert!(matches!(nodes[root].node_type, NodeType::Root));
    let mut reachable = HashSet::from([root]);
    let mut stack = vec![root];
    while let Some(index) = stack.pop() {
        let node = &nodes[index];
        for child in node.children.iter().chain(&node.redirect_node) {
            let child = *child as usize;
            assert!(
                child < nodes.len(),
                "node {index} points at missing node {child}"
            );
            if reachable.insert(child) {
                stack.push(child);
            }
        }
        if index != root {
            assert!(
                node.name()
                    .is_some_and(|name| !name.is_empty() && !name.contains(' '))
            );
        }
    }
    assert_eq!(
        reachable.len(),
        nodes.len(),
        "every node hangs off the root"
    );
}

#[tokio::test]
async fn command_tree_is_well_formed_and_commands_run() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    let (nodes, root) = alice
        .expect("the command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => {
                Some((tree.entries.clone(), tree.root_index as usize))
            }
            _ => None,
        })
        .await;
    assert_valid_tree(&nodes, root);
    let top_level: HashSet<&str> = nodes[root]
        .children
        .iter()
        .filter_map(|child| nodes[*child as usize].name())
        .collect();
    for command in ["tp", "give", "help", "say"] {
        assert!(
            top_level.contains(command),
            "/{command} missing from {top_level:?}"
        );
    }

    let destination = Vec3::new(10.5, 120.0, -3.5);
    alice.command("tp 10.5 120 -3.5").await;
    alice
        .expect("the teleport", |packet| match packet {
            ClientboundGamePacket::PlayerPosition(teleport) => {
                (teleport.change.pos == destination).then_some(())
            }
            _ => None,
        })
        .await;
    assert_eq!(alice.position(), destination);
    bob.wait_until("Alice teleported", |view| {
        view.entity_by_uuid(alice.uuid())
            .is_some_and(|(_, entity)| entity.position.distance_to(destination) < 1e-3)
    })
    .await;

    alice.command("give diamond 5").await;
    alice
        .wait_until("five diamonds", |view| {
            view.inventory.iter().any(|item| {
                matches!(item, ItemStack::Present(data) if data.kind == ItemKind::Diamond && data.count == 5)
            })
        })
        .await;

    alice.chat("hello from e2e").await;
    bob.expect("Alice's chat line", |packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => chat
            .content
            .to_string()
            .contains("hello from e2e")
            .then_some(()),
        _ => None,
    })
    .await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

fn pick_command() -> Command {
    CommandBuilder::new("pick")
        .arg(
            "flavour",
            EnumArg::new([("crème", 1), ("café", 2), ("💎", 3), ("plain", 4)]),
        )
        .handler(|_| {})
        .build()
}

#[tokio::test]
async fn server_suggestions_use_utf16_ranges() {
    let server = TestServer::builder().command(pick_command).start().await;
    let alice = server.join("Alice").await;

    let (nodes, root) = alice
        .expect("the command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => {
                Some((tree.entries.clone(), tree.root_index as usize))
            }
            _ => None,
        })
        .await;
    let pick = nodes[root]
        .children
        .iter()
        .map(|child| &nodes[*child as usize])
        .find(|node| node.name() == Some("pick"))
        .expect("/pick in the tree");
    let flavour = &nodes[pick.children[0] as usize];
    assert!(
        matches!(&flavour.node_type, NodeType::Argument { suggestions_type: Some(kind), .. } if kind.to_string() == "minecraft:ask_server"),
        "the client only asks the server when told to: {flavour:?}"
    );

    let inputs = ["/pick cr", "/pick ca", "/pick 💎", "/pick  é", "/unknown x"];
    for (id, text) in inputs.into_iter().enumerate() {
        alice
            .send(ServerboundCommandSuggestion {
                id: id as u32,
                command: text.to_string(),
            })
            .await;
    }
    alice.sync().await;

    let responses: HashMap<u32, Suggestions> = alice
        .queued(|packet| match packet {
            ClientboundGamePacket::CommandSuggestions(response) => {
                Some((response.id, response.suggestions.clone()))
            }
            _ => None,
        })
        .into_iter()
        .collect();
    let range = |id: u32| {
        let range = responses[&id].range();
        (range.start(), range.length())
    };

    let texts = |id: u32| -> Vec<String> {
        responses[&id]
            .list()
            .iter()
            .map(|suggestion| suggestion.text())
            .collect()
    };

    assert_eq!(texts(0), ["crème"]);
    assert_eq!(range(0), (6, 2));
    assert_eq!(texts(1), ["café"]);
    assert_eq!(range(1), (6, 2));
    assert_eq!(texts(2), ["💎"]);
    assert_eq!(range(2), (6, 2), "an emoji is two UTF-16 units");
    assert!(texts(3).is_empty());
    assert_eq!(range(3), (7, 1));
    assert!(!responses.contains_key(&4));

    alice.assert_healthy();
    server.stop().await;
}

/// The default commands and aliases only an operator may run.
const OPERATOR_COMMANDS: [&str; 10] = [
    "broadcast",
    "clear",
    "gamemode",
    "give",
    "gm",
    "kick",
    "say",
    "stop",
    "summon",
    "tp",
];

/// The default commands and aliases every player may run.
const OPEN_COMMANDS: [&str; 7] = ["help", "list", "msg", "ping", "pl", "plugins", "tell"];

const REFUSED: &str = "You do not have permission to use this command";

fn names(groups: &[&[&str]]) -> BTreeSet<String> {
    groups
        .iter()
        .flat_map(|group| group.iter())
        .map(|name| name.to_string())
        .collect()
}

fn chat_line(packet: &ClientboundGamePacket) -> Option<(String, String)> {
    match packet {
        ClientboundGamePacket::SystemChat(chat) if !chat.overlay => {
            let color = chat
                .content
                .get_base()
                .style
                .color
                .as_ref()
                .map(|color| color.to_string())
                .unwrap_or_default();
            Some((chat.content.to_string(), color))
        }
        _ => None,
    }
}

/// The chat lines the bot got since the last call, once the server has
/// handled everything the bot sent before it.
async fn chat_lines(bot: &Bot) -> Vec<(String, String)> {
    bot.sync().await;
    let lines = bot.queued(chat_line);
    bot.clear();
    lines
}

/// The top-level names of the next command tree the bot receives.
async fn next_tree(bot: &Bot) -> BTreeSet<String> {
    let (nodes, root) = bot
        .expect("a command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => {
                Some((tree.entries.clone(), tree.root_index as usize))
            }
            _ => None,
        })
        .await;
    assert_valid_tree(&nodes, root);
    nodes[root]
        .children
        .iter()
        .filter_map(|child| nodes[*child as usize].name())
        .map(str::to_string)
        .collect()
}

/// The next permission level the bot is told about: the entity status
/// 24 + level, on the bot's own entity.
async fn next_permission_level(bot: &Bot) -> u8 {
    let id = bot.entity_id();
    bot.expect("its permission level", |packet| match packet {
        ClientboundGamePacket::EntityEvent(event)
            if event.entity_id.0 == id && (24..=28).contains(&event.event_id) =>
        {
            Some(event.event_id - 24)
        }
        _ => None,
    })
    .await
}

fn diamonds(count: u8) -> ServerItemStack {
    ServerItemStack::new(ItemId::from_name("minecraft:diamond").unwrap(), count)
}

#[tokio::test]
async fn a_player_without_operator_is_refused_every_operator_command() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let alice_entity = server.player_entity(alice.uuid()).await;
    let bob_entity = server.player_entity(bob.uuid()).await;

    // Something in Alice's inventory for /clear to remove.
    server
        .with_world(move |world| {
            world
                .get_mut::<Inventory>(alice_entity)
                .unwrap()
                .give(diamonds(3));
        })
        .await;
    let state = move |world: &mut World| {
        let position = *world.get::<Position>(alice_entity).unwrap();
        let inventory = world.get::<Inventory>(alice_entity).unwrap();
        let slots: Vec<ServerItemStack> = (0..46).map(|slot| inventory.get(slot).clone()).collect();
        let summoned = world
            .query_filtered::<(), With<SpawnedEntity>>()
            .iter(world)
            .count();
        (
            (position.x, position.y, position.z),
            slots,
            summoned,
            world.entities().contains(bob_entity),
        )
    };
    let before = server.with_world(state).await;
    assert!(before.1.contains(&diamonds(3)));
    assert!(before.3);
    chat_lines(&alice).await;
    chat_lines(&bob).await;

    for command in [
        "kick Bob",
        "kick Bob go away",
        "give diamond 5",
        "tp 100 120 100",
        "tp ~ ~10 ~",
        "summon minecraft:pig",
        "summon minecraft:pig ~ ~ ~ --wander",
        "gamemode creative",
        "gm 1",
        "broadcast hello everyone",
        "say hello everyone",
        "clear",
        "stop",
    ] {
        alice.command(command).await;
        alice.sync().await;
        let effects = alice.queued(|packet| match packet {
            ClientboundGamePacket::PlayerPosition(_)
            | ClientboundGamePacket::ContainerSetContent(_)
            | ClientboundGamePacket::ContainerSetSlot(_)
            | ClientboundGamePacket::AddEntity(_)
            | ClientboundGamePacket::Disconnect(_) => Some(packet.name()),
            ClientboundGamePacket::GameEvent(event) if event.event == EventType::ChangeGameMode => {
                Some(packet.name())
            }
            _ => None,
        });
        assert_eq!(
            chat_lines(&alice).await,
            [(REFUSED.to_string(), "red".to_string())],
            "/{command} is refused with one red line"
        );
        assert!(effects.is_empty(), "/{command} had effects: {effects:?}");
    }

    // The server still runs, and nothing changed on it.
    server.wait_ticks(5).await;
    assert_eq!(server.with_world(state).await, before);
    assert_eq!(
        chat_lines(&bob).await,
        [],
        "Bob was neither kicked nor sent a message"
    );
    alice.command("ping").await;
    assert_eq!(
        chat_lines(&alice).await,
        [("Pong!".to_string(), "white".to_string())]
    );

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn a_player_without_operator_sees_and_runs_only_the_open_commands() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    assert_eq!(next_tree(&alice).await, names(&[&OPEN_COMMANDS]));
    assert_eq!(next_permission_level(&alice).await, 0);
    chat_lines(&alice).await;
    chat_lines(&bob).await;

    let white = |text: &str| (text.to_string(), "white".to_string());
    alice.command("help").await;
    assert_eq!(
        chat_lines(&alice).await,
        [white(
            "--- Available Commands ---\n  /help - List available commands\n  /list - List online players\n  /ping - Pong!\n  /plugins - List plugins\n  /tell - Send a private message to a player"
        )],
        "/help lists only the commands Alice may run"
    );
    for command in ["kick", "gm", "stop"] {
        alice.command(&format!("help {command}")).await;
        assert_eq!(
            chat_lines(&alice).await,
            [(format!("Unknown command: /{command}"), "red".to_string())],
            "/help {command} does not describe a refused command"
        );
    }

    alice.command("help ping").await;
    assert_eq!(
        chat_lines(&alice).await,
        [white("--- /ping ---\n  Pong!\n  Usage: /ping")]
    );
    alice.command("ping").await;
    assert_eq!(chat_lines(&alice).await, [white("Pong!")]);
    alice.command("plugins").await;
    assert_eq!(chat_lines(&alice).await, [white("No plugins loaded.")]);
    alice.command("pl").await;
    assert_eq!(chat_lines(&alice).await, [white("No plugins loaded.")]);
    alice.command("list").await;
    let listed = chat_lines(&alice).await;
    assert!(
        matches!(
            &listed[..],
            [(text, _)] if text == "There are 2 player(s) online: Alice, Bob"
                || text == "There are 2 player(s) online: Bob, Alice"
        ),
        "{listed:?}"
    );
    alice.command("tell Bob psst").await;
    assert_eq!(
        chat_lines(&alice).await,
        [white("You whisper to Bob: psst")]
    );
    alice.command("msg Bob again").await;
    assert_eq!(
        chat_lines(&alice).await,
        [white("You whisper to Bob: again")]
    );
    let gray = |text: &str| (text.to_string(), "gray".to_string());
    assert_eq!(
        chat_lines(&bob).await,
        [
            gray("Alice whispers to you: psst"),
            gray("Alice whispers to you: again")
        ]
    );

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn an_operator_sees_and_runs_the_operator_commands() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    assert_eq!(next_permission_level(&alice).await, 4);
    assert_eq!(
        next_tree(&alice).await,
        names(&[&OPEN_COMMANDS, &OPERATOR_COMMANDS])
    );
    chat_lines(&alice).await;
    chat_lines(&bob).await;

    alice.command("help").await;
    let listing = chat_lines(&alice).await;
    let [(listing, _)] = &listing[..] else {
        panic!("/help answers with one line: {listing:?}");
    };
    let listed: BTreeSet<String> = listing
        .lines()
        .skip(1)
        .filter_map(|entry| entry.strip_prefix("  /")?.split(' ').next())
        .map(str::to_string)
        .collect();
    let canonical: BTreeSet<String> = names(&[&OPEN_COMMANDS, &OPERATOR_COMMANDS])
        .into_iter()
        .filter(|name| !["gm", "msg", "pl"].contains(&name.as_str()))
        .collect();
    assert_eq!(
        listed, canonical,
        "/help lists every default command:\n{listing}"
    );

    for (command, reply) in [
        ("give diamond 5", "Gave 5 x "),
        ("clear", "Inventory cleared"),
        ("tp 10.5 120 -3.5", "Teleported to 10.5, 120.0, -3.5"),
        ("gamemode creative", "Game mode set to creative (1)"),
        ("gm survival", "Game mode set to survival (0)"),
        (
            "summon minecraft:pig 10 120 -3",
            "Summoned minecraft:pig at ",
        ),
        ("broadcast hello everyone", "[Broadcast] hello everyone"),
        ("say hello everyone", "[Alice] hello everyone"),
    ] {
        alice.command(command).await;
        let lines = chat_lines(&alice).await;
        assert!(
            matches!(&lines[..], [(text, color)] if text.starts_with(reply) && color != "red"),
            "/{command} answered {lines:?}"
        );
    }
    let bob_saw: Vec<String> = chat_lines(&bob)
        .await
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert_eq!(
        bob_saw,
        ["[Broadcast] hello everyone", "[Alice] hello everyone"]
    );

    alice.command("kick Bob").await;
    bob.expect("the kick", |packet| {
        matches!(packet, ClientboundGamePacket::Disconnect(_)).then_some(())
    })
    .await;
    assert_eq!(
        chat_lines(&alice).await,
        [(
            "Kicked Bob (reason: Kicked by an operator)".to_string(),
            "white".to_string()
        )]
    );

    alice.command("stop").await;
    server.wait_for_exit().await;
}

#[tokio::test]
async fn granting_and_removing_operator_updates_the_client() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let open = names(&[&OPEN_COMMANDS]);
    let all = names(&[&OPEN_COMMANDS, &OPERATOR_COMMANDS]);

    assert_eq!(next_permission_level(&alice).await, 0);
    assert_eq!(next_tree(&alice).await, open);

    server.op(&alice).await;
    assert_eq!(next_permission_level(&alice).await, 4);
    assert_eq!(next_tree(&alice).await, all);
    chat_lines(&alice).await;
    alice.command("gamemode creative").await;
    assert_eq!(
        chat_lines(&alice).await,
        [(
            "Game mode set to creative (1)".to_string(),
            "white".to_string()
        )]
    );

    // Granting it again changes nothing, so nothing is sent again.
    server.op(&alice).await;
    alice.sync().await;
    let resent = alice.queued(|packet| match packet {
        ClientboundGamePacket::Commands(_) | ClientboundGamePacket::EntityEvent(_) => {
            Some(packet.name())
        }
        _ => None,
    });
    assert!(resent.is_empty(), "a second grant re-sent {resent:?}");

    server.deop(&alice).await;
    assert_eq!(next_permission_level(&alice).await, 0);
    assert_eq!(next_tree(&alice).await, open);
    chat_lines(&alice).await;
    alice.command("gamemode survival").await;
    assert_eq!(
        chat_lines(&alice).await,
        [(REFUSED.to_string(), "red".to_string())]
    );

    alice.assert_healthy();
    server.stop().await;
}
