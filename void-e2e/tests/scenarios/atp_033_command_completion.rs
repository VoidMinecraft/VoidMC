//! VMC-ATP-033: the server advertises its command tree and answers
//! tab-completion requests with controlled suggestions.
//!
//! A vanilla client completes literals and most arguments by itself from the
//! tree; it only sends a suggestion request for nodes marked
//! `minecraft:ask_server`. The tree checks below therefore stand for steps a
//! client does locally, and the request checks cover what the server answers,
//! including to requests a vanilla client would never send.

use std::any::Any;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use bevy_ecs::world::World;
use voidmc::components::PlayerReady;
use voidmc::{
    ArgParser, Command, CommandBuilder, CommandRegistry, EnumArg, GameProfileArg, StringArg,
};
use voidmc_e2e::brigadier::suggestion::Suggestions;
use voidmc_e2e::protocol::common::client_information::ClientInformation;
use voidmc_e2e::protocol::connect::Connection;
use voidmc_e2e::protocol::packets::config::{
    ClientboundConfigPacket, s_client_information::ServerboundClientInformation,
    s_finish_configuration::ServerboundFinishConfiguration,
    s_keep_alive::ServerboundKeepAlive as ConfigKeepAlive,
    s_select_known_packs::ServerboundSelectKnownPacks,
};
use voidmc_e2e::protocol::packets::game::c_commands::{
    BrigadierNodeStub, BrigadierNumber, BrigadierParser, BrigadierString, EntityParser, NodeType,
};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, s_command_suggestion::ServerboundCommandSuggestion,
};
use voidmc_e2e::protocol::packets::handshake::s_intention::ServerboundIntention;
use voidmc_e2e::protocol::packets::login::{
    ClientboundLoginPacket, s_hello::ServerboundHello,
    s_login_acknowledged::ServerboundLoginAcknowledged,
};
use voidmc_e2e::protocol::packets::{ClientIntention, PROTOCOL_VERSION};
use voidmc_e2e::{Bot, TestServer};
use voidmc_protocol::clientbound::commands::{Parser, StringType};

const ASK_SERVER: &str = "minecraft:ask_server";

/// The tree as a client holds it.
struct Tree {
    nodes: Vec<BrigadierNodeStub>,
    root: usize,
}

impl Tree {
    async fn received_by(bot: &Bot) -> Self {
        bot.expect("the command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => Some(Tree {
                nodes: tree.entries.clone(),
                root: tree.root_index as usize,
            }),
            _ => None,
        })
        .await
    }

    fn node(&self, index: u32) -> &BrigadierNodeStub {
        &self.nodes[index as usize]
    }

    fn child_index(&self, parent: usize, name: &str) -> usize {
        self.nodes[parent]
            .children
            .iter()
            .map(|child| *child as usize)
            .find(|child| self.nodes[*child].name() == Some(name))
            .unwrap_or_else(|| panic!("node {parent} has no child {name}"))
    }

    /// The node reached by following child names from the root.
    fn index_at(&self, path: &[&str]) -> usize {
        path.iter()
            .fold(self.root, |parent, name| self.child_index(parent, name))
    }

    fn at(&self, path: &[&str]) -> &BrigadierNodeStub {
        &self.nodes[self.index_at(path)]
    }

    fn top_level(&self) -> BTreeSet<String> {
        self.nodes[self.root]
            .children
            .iter()
            .map(|child| {
                let node = self.node(*child);
                assert!(
                    matches!(node.node_type, NodeType::Literal { .. }),
                    "root children are command literals: {node:?}"
                );
                node.name().unwrap().to_string()
            })
            .collect()
    }

    /// What a vanilla client lists at the command root for `typed` (the text
    /// after `/`): the root literals that start with it, ignoring case.
    fn complete_root(&self, typed: &str) -> Vec<String> {
        let typed = typed.to_lowercase();
        self.top_level()
            .into_iter()
            .filter(|name| name.to_lowercase().starts_with(&typed))
            .collect()
    }

    /// Indices in range, one root, every node reachable from it, no cycle
    /// through child edges, and redirects only on flag branches, pointing back
    /// at the node that offers the flag.
    fn assert_well_formed(&self) {
        let count = self.nodes.len();
        assert!(self.root < count, "root index {} out of range", self.root);
        for (index, node) in self.nodes.iter().enumerate() {
            for target in node.children.iter().chain(&node.redirect_node) {
                assert!(
                    (*target as usize) < count,
                    "node {index} points at missing node {target}"
                );
            }
            assert_eq!(
                matches!(node.node_type, NodeType::Root),
                index == self.root,
                "node {index} is a root but not the root, or the root is not a root node"
            );
            if index != self.root {
                assert!(
                    node.name()
                        .is_some_and(|name| !name.is_empty() && !name.contains(' ')),
                    "node {index} has no usable name: {node:?}"
                );
            }
        }

        // Depth-first search over child edges: grey nodes are on the stack.
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            White,
            Grey,
            Black,
        }
        let mut marks = vec![Mark::White; count];
        let mut stack = vec![(self.root, 0usize)];
        marks[self.root] = Mark::Grey;
        while let Some((index, next)) = stack.pop() {
            match self.nodes[index].children.get(next) {
                Some(child) => {
                    let child = *child as usize;
                    stack.push((index, next + 1));
                    match marks[child] {
                        Mark::White => {
                            marks[child] = Mark::Grey;
                            stack.push((child, 0));
                        }
                        Mark::Grey => panic!("child edges form a cycle through node {child}"),
                        Mark::Black => {}
                    }
                }
                None => marks[index] = Mark::Black,
            }
        }
        assert!(
            marks.iter().all(|mark| *mark == Mark::Black),
            "every node hangs off the root"
        );

        let mut flag_redirects = HashSet::new();
        for (parent, node) in self.nodes.iter().enumerate() {
            for child in &node.children {
                let flag = self.node(*child);
                if !flag.name().is_some_and(|name| name.starts_with('-'))
                    || !matches!(flag.node_type, NodeType::Literal { .. })
                {
                    continue;
                }
                match (flag.redirect_node, flag.children.as_slice()) {
                    (Some(target), []) => {
                        assert_eq!(target as usize, parent, "{:?} redirects away", flag.name());
                        assert!(flag.is_executable);
                        flag_redirects.insert(*child as usize);
                    }
                    (None, [value]) => {
                        let value_node = self.node(*value);
                        assert_eq!(
                            value_node.redirect_node,
                            Some(parent as u32),
                            "the value of {:?} redirects away",
                            flag.name()
                        );
                        assert!(value_node.children.is_empty() && value_node.is_executable);
                        flag_redirects.insert(*value as usize);
                    }
                    shape => panic!("unexpected flag branch {:?}: {shape:?}", flag.name()),
                }
            }
        }
        for (index, node) in self.nodes.iter().enumerate() {
            if node.redirect_node.is_some() {
                assert!(
                    flag_redirects.contains(&index),
                    "node {index} redirects outside a flag branch: {node:?}"
                );
            }
        }
    }
}

fn parser(node: &BrigadierNodeStub) -> (&BrigadierParser, Option<String>) {
    match &node.node_type {
        NodeType::Argument {
            parser,
            suggestions_type,
            ..
        } => (parser, suggestions_type.as_ref().map(ToString::to_string)),
        other => panic!("expected an argument node, got {other:?}"),
    }
}

fn integer(min: i32, max: i32) -> BrigadierParser {
    BrigadierParser::Integer(BrigadierNumber::new(Some(min), Some(max)))
}

fn single_player() -> BrigadierParser {
    BrigadierParser::Entity(EntityParser {
        single: true,
        players_only: true,
    })
}

fn flag_names(tree: &Tree, index: usize) -> BTreeSet<String> {
    tree.nodes[index]
        .children
        .iter()
        .filter_map(|child| tree.node(*child).name())
        .filter(|name| name.starts_with('-'))
        .map(str::to_string)
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| item.to_string()).collect()
}

const DEFAULT_LITERALS: [&str; 17] = [
    "broadcast",
    "clear",
    "gamemode",
    "give",
    "gm",
    "help",
    "kick",
    "list",
    "msg",
    "ping",
    "pl",
    "plugins",
    "say",
    "stop",
    "summon",
    "tell",
    "tp",
];

#[tokio::test]
async fn default_command_tree_reaches_the_client_complete_and_typed() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let tree = Tree::received_by(&alice).await;
    tree.assert_well_formed();

    // Every registered command and alias is a root literal, and nothing else.
    let top_level = tree.top_level();
    assert_eq!(top_level, set(&DEFAULT_LITERALS));
    let names = top_level.clone();
    let (registered, unresolved) = server
        .with_world(move |world| {
            let registry = world.resource::<CommandRegistry>();
            let registered: BTreeSet<String> = registry
                .command_names()
                .into_iter()
                .map(str::to_string)
                .collect();
            let unresolved: Vec<String> = names
                .iter()
                .filter(|name| registry.resolve(name).is_none())
                .cloned()
                .collect();
            (registered, unresolved)
        })
        .await;
    assert!(registered.is_subset(&top_level), "{registered:?}");
    assert!(
        unresolved.is_empty(),
        "literals the server cannot run: {unresolved:?}"
    );
    for (alias, command) in [("gm", "gamemode"), ("msg", "tell"), ("pl", "plugins")] {
        assert_eq!(
            tree.at(&[alias]).children,
            tree.at(&[command]).children,
            "/{alias} leads to the same arguments as /{command}"
        );
        assert_eq!(
            tree.at(&[alias]).is_executable,
            tree.at(&[command]).is_executable
        );
    }

    // Commands that run without arguments are executable at their literal.
    for command in ["ping", "plugins", "pl", "list", "clear", "stop", "help"] {
        assert!(tree.at(&[command]).is_executable, "/{command} runs bare");
    }
    for command in ["ping", "plugins", "list", "clear", "stop"] {
        assert!(
            tree.at(&[command]).children.is_empty(),
            "/{command} takes nothing"
        );
    }
    for command in [
        "tp",
        "gamemode",
        "give",
        "kick",
        "tell",
        "msg",
        "say",
        "broadcast",
        "summon",
    ] {
        assert!(
            !tree.at(&[command]).is_executable,
            "/{command} needs an argument"
        );
    }

    // Each argument carries its parser, string mode and range as Azalea
    // decodes them, and is executable exactly where the command can run.
    let expected: [(&[&str], BrigadierParser, bool); 13] = [
        (
            &["help", "command"],
            BrigadierParser::String(BrigadierString::SingleWord),
            true,
        ),
        (&["tp", "position"], BrigadierParser::Vec3, true),
        (&["gamemode", "mode"], BrigadierParser::GameMode, true),
        (&["give", "item"], BrigadierParser::ItemStack, true),
        (&["give", "item", "count"], integer(1, 64), true),
        (&["kick", "player"], single_player(), true),
        (
            &["kick", "player", "reason"],
            BrigadierParser::String(BrigadierString::GreedyPhrase),
            true,
        ),
        (&["tell", "player"], single_player(), false),
        (
            &["tell", "player", "message"],
            BrigadierParser::String(BrigadierString::GreedyPhrase),
            true,
        ),
        (
            &["say", "message"],
            BrigadierParser::String(BrigadierString::GreedyPhrase),
            true,
        ),
        (
            &["broadcast", "message"],
            BrigadierParser::String(BrigadierString::GreedyPhrase),
            true,
        ),
        (&["summon", "entity"], BrigadierParser::Identifier, true),
        (
            &["summon", "entity", "position"],
            BrigadierParser::Vec3,
            true,
        ),
    ];
    for (path, expected_parser, executable) in expected {
        let node = tree.at(path);
        assert_eq!(parser(node).0, &expected_parser, "/{}", path.join(" "));
        assert_eq!(
            node.is_executable,
            executable,
            "/{} executable",
            path.join(" ")
        );
    }

    // The summon entity is completed by the client from its own list; no
    // default argument asks the server, so a vanilla client never sends a
    // suggestion request for a default command.
    assert_eq!(
        parser(tree.at(&["summon", "entity"])).1.as_deref(),
        Some("minecraft:summonable_entities")
    );
    for node in &tree.nodes {
        if let NodeType::Argument {
            suggestions_type: Some(kind),
            name,
            ..
        } = &node.node_type
        {
            assert_eq!(
                kind.to_string(),
                "minecraft:summonable_entities",
                "{name} asks for unexpected suggestions"
            );
        }
    }
    // Players for /tell and /kick are a `minecraft:entity` argument, which the
    // client completes from its own player list.
    assert_eq!(parser(tree.at(&["tell", "player"])).1, None);
    assert_eq!(parser(tree.at(&["kick", "player"])).1, None);

    // The /summon flags, long and short, follow the entity and the position.
    let summon_flags = set(&["--wander", "-w", "--gravity", "-g", "--block-checks", "-b"]);
    for path in [
        &["summon", "entity"][..],
        &["summon", "entity", "position"][..],
    ] {
        let index = tree.index_at(path);
        assert_eq!(
            flag_names(&tree, index),
            summon_flags,
            "/{}",
            path.join(" ")
        );
    }
    assert!(flag_names(&tree, tree.index_at(&["summon"])).is_empty());

    alice.assert_healthy();
    server.stop().await;
}

/// The client walks the root literals for a typed prefix; the server is not
/// asked, and answers nothing if a client asks anyway.
#[tokio::test]
async fn command_names_complete_from_the_tree_and_root_requests_get_no_reply() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let tree = Tree::received_by(&alice).await;

    assert_eq!(tree.complete_root("he"), ["help"]);
    assert_eq!(tree.complete_root("HE"), ["help"]);
    assert_eq!(tree.complete_root("g"), ["gamemode", "give", "gm"]);
    assert_eq!(tree.complete_root("t"), ["tell", "tp"]);
    assert_eq!(tree.complete_root(""), DEFAULT_LITERALS);
    assert!(tree.complete_root("zz").is_empty());

    let root_requests = ["", "/", "/he", "he", "/help", "/zz", "/zz ", "/ help"];
    for (id, text) in root_requests.into_iter().enumerate() {
        alice
            .send(ServerboundCommandSuggestion {
                id: id as u32,
                command: text.to_string(),
            })
            .await;
    }
    alice
        .send(ServerboundCommandSuggestion {
            id: 100,
            command: "/give diamond ".to_string(),
        })
        .await;
    alice.sync().await;

    let answered = responses(&alice);
    assert_eq!(
        answered.keys().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([100]),
        "only the request naming a command and a space is answered"
    );
    assert!(answered[&100].0.is_empty());
    assert_eq!(answered[&100].1, (14, 0));

    alice.assert_healthy();
    server.stop().await;
}

/// A custom completion source: warps known to the application.
struct WarpArg;

impl ArgParser for WarpArg {
    fn type_name(&self) -> &str {
        "warp"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        Ok(Box::new(input.to_string()))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(StringType::SingleWord))
    }

    fn suggestions(&self, partial: &str, _world: &World) -> Vec<String> {
        ["spawn", "spire", "arena"]
            .into_iter()
            .filter(|warp| warp.starts_with(partial))
            .map(str::to_string)
            .collect()
    }

    fn suggestions_type(&self) -> Option<&str> {
        Some(ASK_SERVER)
    }
}

fn greet_command() -> Command {
    CommandBuilder::new("greet")
        .alias("hi")
        .arg("player", Arc::new(GameProfileArg))
        .arg(
            "mood",
            EnumArg::new([("happy", 1), ("hungry", 2), ("sad", 3)]),
        )
        .arg_optional("warp", Arc::new(WarpArg))
        .flag("loud", Some('l'), "Shout it")
        .flag_value("note", Some('n'), "A note", StringArg::quotable())
        .handler(|_| {})
        .build()
}

type Answer = (Vec<String>, (usize, usize));

/// The queued suggestion responses by transaction id: texts (as Azalea
/// decodes them, sorted) and the replaced range.
fn responses(bot: &Bot) -> HashMap<u32, Answer> {
    let mut answers = HashMap::new();
    for (id, suggestions) in bot.queued(|packet| match packet {
        ClientboundGamePacket::CommandSuggestions(response) => {
            Some((response.id, response.suggestions.clone()))
        }
        _ => None,
    }) {
        let previous = answers.insert(id, answer(&suggestions));
        assert!(previous.is_none(), "transaction {id} answered twice");
    }
    answers
}

fn answer(suggestions: &Suggestions) -> Answer {
    let range = suggestions.range();
    (
        suggestions.list().iter().map(|s| s.text()).collect(),
        (range.start(), range.length()),
    )
}

fn utf16(text: &str) -> usize {
    text.encode_utf16().count()
}

async fn ask_all(bot: &Bot, requests: &[(u32, &str)]) -> HashMap<u32, Answer> {
    bot.clear();
    for (id, text) in requests {
        bot.send(ServerboundCommandSuggestion {
            id: *id,
            command: text.to_string(),
        })
        .await;
    }
    bot.sync().await;
    responses(bot)
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[tokio::test]
async fn server_completes_ask_server_arguments_with_valid_matches_only() {
    let server = TestServer::builder().command(greet_command).start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // The application command is advertised like a default one.
    let tree = Tree::received_by(&alice).await;
    tree.assert_well_formed();
    assert!(tree.top_level().is_superset(&set(&["greet", "hi"])));
    assert_eq!(tree.at(&["hi"]).children, tree.at(&["greet"]).children);
    let player = tree.at(&["greet", "player"]);
    assert_eq!(
        parser(player),
        (&BrigadierParser::GameProfile, Some(ASK_SERVER.to_string()))
    );
    assert!(!player.is_executable);
    let mood = tree.at(&["greet", "player", "mood"]);
    assert_eq!(
        parser(mood),
        (
            &BrigadierParser::String(BrigadierString::SingleWord),
            Some(ASK_SERVER.to_string())
        )
    );
    assert!(mood.is_executable);
    let warp = tree.at(&["greet", "player", "mood", "warp"]);
    assert_eq!(
        parser(warp),
        (
            &BrigadierParser::String(BrigadierString::SingleWord),
            Some(ASK_SERVER.to_string())
        )
    );
    let note = tree.at(&["greet", "player", "mood", "--note", "note"]);
    assert_eq!(
        parser(note).0,
        &BrigadierParser::String(BrigadierString::QuotablePhrase)
    );
    assert_eq!(
        flag_names(&tree, tree.index_at(&["greet", "player", "mood"])),
        set(&["--loud", "-l", "--note", "-n"])
    );

    let emoji_line = "/greet 😀 s";
    let past_end = "/greet Alice sad spawn extra";
    let answers = ask_all(
        &alice,
        &[
            // First argument, every ready player including the requester.
            (1, "/greet "),
            // ASCII case is ignored; the alias and a line without the slash
            // complete the same way.
            (2, "/greet a"),
            (3, "/hi B"),
            (4, "greet b"),
            (5, "/greet x"),
            // The second argument once the first is typed.
            (6, "/greet Alice "),
            (7, "/greet Alice h"),
            (8, "/greet Alice HU"),
            (9, "/greet Alice z"),
            (10, emoji_line),
            // The custom provider, flags, and a token past the last argument.
            (11, "/greet Alice sad sp"),
            (12, "/greet Alice sad --"),
            (13, "/greet --loud -n hi Bob h"),
            (14, past_end),
            // Arguments the client completes itself get an empty list.
            (15, "/tp 1"),
            (16, "/summon minecraft:pi"),
            (17, "/summon pi"),
            (18, "/tell A"),
            (19, "/gamemode s"),
            // A command this server does not know gets no reply.
            (20, "/nope x"),
        ],
    )
    .await;

    let expect = |id: u32, texts: &[&str], range: (usize, usize)| {
        let got = answers
            .get(&id)
            .unwrap_or_else(|| panic!("request {id} got no reply"));
        assert_eq!(got, &(strings(texts), range), "request {id}");
    };
    expect(1, &["Alice", "Bob"], (7, 0));
    expect(2, &["Alice"], (7, 1));
    expect(3, &["Bob"], (4, 1));
    expect(4, &["Bob"], (6, 1));
    expect(5, &[], (7, 1));
    expect(6, &["happy", "hungry", "sad"], (13, 0));
    expect(7, &["happy", "hungry"], (13, 1));
    expect(8, &["hungry"], (13, 2));
    expect(9, &[], (13, 1));
    expect(10, &["sad"], (utf16(emoji_line) - 1, 1));
    assert_eq!(
        utf16(emoji_line) - 1,
        10,
        "the emoji counts two UTF-16 units"
    );
    expect(11, &["spawn", "spire"], (17, 2));
    expect(12, &["--loud", "--note"], (17, 2));
    expect(13, &["happy", "hungry"], (24, 1));
    expect(14, &[], (past_end.len() - 5, 5));
    expect(15, &[], (4, 1));
    expect(16, &[], (8, 12));
    expect(17, &[], (8, 2));
    expect(18, &[], (6, 1));
    expect(19, &[], (10, 1));
    assert!(!answers.contains_key(&20), "unknown command answered");

    // Player names follow joins and leaves.
    let carol = server.join("Carol").await;
    bob.disconnect().await;
    alice
        .wait_until("Bob gone from the tab list", |view| {
            !view.players.values().any(|name| name == "Bob")
        })
        .await;
    let answers = ask_all(
        &alice,
        &[(30, "/greet "), (31, "/greet b"), (32, "/greet C")],
    )
    .await;
    assert_eq!(answers[&30], (strings(&["Alice", "Carol"]), (7, 0)));
    assert_eq!(answers[&31], (Vec::new(), (7, 1)));
    assert_eq!(answers[&32], (strings(&["Carol"]), (7, 1)));

    alice.assert_healthy();
    carol.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn hostile_and_flooding_requests_get_controlled_replies() {
    let server = TestServer::builder().command(greet_command).start().await;
    let alice = server.join("Alice").await;

    let long_partial = "h".repeat(300);
    let long_line = format!("/greet Alice {long_partial}");
    // The protocol's limit for this string is 32500 characters.
    let huge_partial = "a".repeat(32_500 - "/greet ".len());
    let huge_line = format!("/greet {huge_partial}");
    let huge_greedy = format!("/say {}", "word ".repeat(6_499));
    let controls = "/greet \u{0}\u{1b}[31m\u{7f}";
    let after_controls = "/greet \u{1}\u{2} b";
    let answers = ask_all(
        &alice,
        &[
            (1, &long_line),
            (2, &huge_line),
            (3, &huge_greedy),
            (4, controls),
            (5, after_controls),
            // The text up to the cursor is what a client sends, so a cursor in
            // the middle of `Alice` is a request for its first letters.
            (6, "/greet Al"),
            (7, "/greet Alice sad spawn extra more"),
            (8, "/ping "),
            (9, "/ping extra"),
            (10, "/greet\u{0} "),
        ],
    )
    .await;
    assert_eq!(answers[&1], (Vec::new(), (13, 300)));
    assert_eq!(answers[&2], (Vec::new(), (7, huge_partial.len())));
    assert_eq!(answers[&3], (Vec::new(), (huge_greedy.len(), 0)));
    assert_eq!(answers[&4], (Vec::new(), (7, utf16(controls) - 7)));
    assert_eq!(answers[&5], (Vec::new(), (utf16(after_controls) - 1, 1)));
    assert_eq!(answers[&6], (strings(&["Alice"]), (7, 2)));
    assert_eq!(answers[&7], (Vec::new(), (29, 4)));
    assert_eq!(answers[&8], (Vec::new(), (6, 0)));
    assert_eq!(answers[&9], (Vec::new(), (6, 5)));
    assert!(
        !answers.contains_key(&10),
        "an unknown command name is not answered"
    );

    // Five hundred requests at once: every one answered once, in one piece.
    let flood: Vec<(u32, String)> = (1_000..1_500)
        .map(|id| {
            (
                id,
                if id % 2 == 0 { "/greet A" } else { "/nope " }.to_string(),
            )
        })
        .collect();
    let flood_refs: Vec<(u32, &str)> = flood
        .iter()
        .map(|(id, text)| (*id, text.as_str()))
        .collect();
    let answers = ask_all(&alice, &flood_refs).await;
    let expected_ids: BTreeSet<u32> = (1_000..1_500).step_by(2).collect();
    let answered_ids: BTreeSet<u32> = answers.keys().copied().collect();
    assert!(
        answered_ids == expected_ids,
        "flood: missing {:?}, unexpected {:?}",
        expected_ids.difference(&answered_ids).collect::<Vec<_>>(),
        answered_ids.difference(&expected_ids).collect::<Vec<_>>()
    );
    for (id, got) in answers {
        assert_eq!(got, (strings(&["Alice"]), (7, 1)), "flood request {id}");
    }

    // The connection is intact and the next request is answered.
    let answers = ask_all(&alice, &[(2_000, "/greet Alice s")]).await;
    assert_eq!(answers[&2_000], (strings(&["sad"]), (13, 1)));

    alice.assert_healthy();
    server.stop().await;
}

/// Logs a client in and stops before it reports itself loaded: it is in the
/// Play state but not ready. Returns its connection once the tree arrived.
async fn join_without_loading(
    server: &TestServer,
    name: &str,
) -> Connection<ClientboundGamePacket, voidmc_e2e::protocol::packets::game::ServerboundGamePacket> {
    let address = server.address();
    let mut connection = Connection::new(&address).await.expect("connect");
    connection
        .write(ServerboundIntention {
            protocol_version: PROTOCOL_VERSION,
            hostname: address.ip().to_string(),
            port: address.port(),
            intention: ClientIntention::Login,
        })
        .await
        .expect("handshake");
    let mut connection = connection.login();
    connection
        .write(ServerboundHello {
            name: name.to_string(),
            profile_id: azalea_crypto::offline::generate_uuid(name),
        })
        .await
        .expect("login start");
    loop {
        match connection.read().await.expect("login packet") {
            ClientboundLoginPacket::LoginCompression(packet) => {
                connection.set_compression_threshold(packet.compression_threshold)
            }
            ClientboundLoginPacket::LoginFinished(_) => break,
            other => panic!("unexpected login packet {other:?}"),
        }
    }
    connection
        .write(ServerboundLoginAcknowledged {})
        .await
        .expect("login acknowledged");
    let mut connection = connection.config();
    connection
        .write(ServerboundClientInformation {
            information: ClientInformation::default(),
        })
        .await
        .expect("client information");
    loop {
        match connection.read().await.expect("configuration packet") {
            ClientboundConfigPacket::SelectKnownPacks(packet) => connection
                .write(ServerboundSelectKnownPacks {
                    known_packs: packet.known_packs,
                })
                .await
                .expect("known packs"),
            ClientboundConfigPacket::KeepAlive(packet) => connection
                .write(ConfigKeepAlive { id: packet.id })
                .await
                .expect("keep-alive"),
            ClientboundConfigPacket::FinishConfiguration(_) => {
                connection
                    .write(ServerboundFinishConfiguration {})
                    .await
                    .expect("finish configuration");
                break;
            }
            _ => {}
        }
    }
    let mut connection = connection.game();
    loop {
        if let ClientboundGamePacket::Commands(_) = connection.read().await.expect("play packet") {
            return connection;
        }
    }
}

#[tokio::test]
async fn a_player_not_yet_ready_is_answered_but_not_listed() {
    let server = TestServer::builder().command(greet_command).start().await;
    let alice = server.join("Alice").await;

    let mut dora = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        join_without_loading(&server, "Dora"),
    )
    .await
    .expect("Dora to enter Play");
    let entity = server
        .player_entity(azalea_crypto::offline::generate_uuid("Dora"))
        .await;
    assert!(
        !server
            .with_world(move |world| world.get::<PlayerReady>(entity).is_some())
            .await,
        "Dora has not reported herself loaded"
    );

    dora.write(ServerboundCommandSuggestion {
        id: 7,
        command: "/greet ".to_string(),
    })
    .await
    .expect("send the request");
    let reply = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            match dora.read().await.expect("Dora stays connected") {
                ClientboundGamePacket::CommandSuggestions(response) => return response,
                ClientboundGamePacket::Disconnect(packet) => {
                    panic!("Dora was disconnected: {}", packet.reason)
                }
                _ => {}
            }
        }
    })
    .await
    .expect("Dora's suggestions");
    assert_eq!(reply.id, 7);
    assert_eq!(answer(&reply.suggestions), (strings(&["Alice"]), (7, 0)));

    let answers = ask_all(&alice, &[(1, "/greet ")]).await;
    assert_eq!(
        answers[&1],
        (strings(&["Alice"]), (7, 0)),
        "only ready players are offered"
    );

    alice.assert_healthy();
    server.stop().await;
}
