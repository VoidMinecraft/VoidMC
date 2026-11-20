//! VMC-ATP-008: a connected player discovers the commands with `/help`, gets
//! an answer from `/ping` and `/plugins`, and gets one controlled reply for
//! any input that does not name a command or does not fit its arguments.
//!
//! Every reply is read the way a vanilla client shows it: the text and colour
//! of the `SystemChat` packets the player receives. Commands are sent both
//! as a `ChatCommand` packet and as a chat line starting with `/`, which the
//! server must treat the same way.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_app::{App, First};
use bevy_ecs::prelude::*;
use voidmc::components::{PlayerName, PlayerReady};
use voidmc::events::ChatCommandEvent;
use voidmc::network::NetworkChannels;
use voidmc::systems::keep_alive::KeepAliveTicker;
use voidmc::{Command, CommandBuilder, CommandRegistry, PluginList, help_command};
use voidmc_e2e::protocol::common::client_information::ClientInformation;
use voidmc_e2e::protocol::connect::Connection;
use voidmc_e2e::protocol::packets::config::{
    ClientboundConfigPacket, s_client_information::ServerboundClientInformation,
    s_finish_configuration::ServerboundFinishConfiguration,
    s_keep_alive::ServerboundKeepAlive as ConfigKeepAlive,
    s_select_known_packs::ServerboundSelectKnownPacks,
};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, ServerboundGamePacket, s_chat_command::ServerboundChatCommand,
    s_keep_alive::ServerboundKeepAlive,
};
use voidmc_e2e::protocol::packets::handshake::s_intention::ServerboundIntention;
use voidmc_e2e::protocol::packets::login::{
    ClientboundLoginPacket, s_hello::ServerboundHello,
    s_login_acknowledged::ServerboundLoginAcknowledged,
};
use voidmc_e2e::protocol::packets::{ClientIntention, PROTOCOL_VERSION};
use voidmc_e2e::{Bot, TestServer};

/// One chat line as the player sees it: its text and its colour.
type Line = (String, String);

fn line(text: &str, color: &str) -> Line {
    (text.to_string(), color.to_string())
}

fn system_chat(packet: &ClientboundGamePacket) -> Option<Line> {
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

/// Everything the bot was sent in chat since the last call, once the server
/// has handled every packet the bot sent before it.
async fn chat_lines(bot: &Bot) -> Vec<Line> {
    bot.sync().await;
    let lines = bot.queued(system_chat);
    bot.clear();
    lines
}

/// The lines the player gets back for `input`, typed once as a command packet
/// and once as a chat line starting with `/`; both must be the same.
async fn replies_to(bot: &Bot, input: &str) -> Vec<Line> {
    bot.command(input).await;
    let as_command = chat_lines(bot).await;
    bot.chat(&format!("/{input}")).await;
    let as_chat = chat_lines(bot).await;
    assert_eq!(
        as_command,
        as_chat,
        "/{} gets the same reply from both packets",
        shorten(input)
    );
    as_command
}

fn shorten(input: &str) -> String {
    let short: String = input.chars().take(40).collect();
    if short.len() < input.len() {
        format!("{short:?}... ({} bytes)", input.len())
    } else {
        format!("{short:?}")
    }
}

/// Joins `names` and leaves each bot with an empty inbox, returning the
/// top-level names of the command tree each one received.
async fn join_all(server: &TestServer, names: &[&str]) -> Vec<(Bot, Vec<String>)> {
    let mut bots = Vec::new();
    for name in names {
        let bot = server.join(name).await;
        let tree = top_level_commands(&bot).await;
        bots.push((bot, tree));
    }
    for (bot, _) in &bots {
        chat_lines(bot).await;
    }
    bots
}

async fn top_level_commands(bot: &Bot) -> Vec<String> {
    let (nodes, root) = bot
        .expect("the command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => {
                Some((tree.entries.clone(), tree.root_index as usize))
            }
            _ => None,
        })
        .await;
    nodes[root]
        .children
        .iter()
        .map(|child| {
            nodes[*child as usize]
                .name()
                .expect("top-level nodes are literals")
                .to_string()
        })
        .collect()
}

async fn assert_nothing_received(bot: &Bot) {
    let lines = chat_lines(bot).await;
    assert!(
        lines.is_empty(),
        "{} got replies meant for someone else: {lines:?}",
        bot.name()
    );
}

fn greet_command() -> Command {
    CommandBuilder::new("greet")
        .description("Say hello to the server")
        .alias("hey")
        .handler(|ctx| ctx.reply("Hello!"))
        .build()
}

fn quiet_command() -> Command {
    CommandBuilder::new("quiet").handler(|_| {}).build()
}

/// The default commands and the descriptions `/help` shows for them, as the
/// commands reference documents them, in `/help` order.
const DEFAULT_COMMANDS: [(&str, &str); 14] = [
    ("broadcast", "Broadcast a message to all players"),
    ("clear", "Clear your inventory"),
    ("gamemode", "Change game mode"),
    ("give", "Give yourself an item"),
    ("help", "List available commands"),
    ("kick", "Kick a player"),
    ("list", "List online players"),
    ("ping", "Pong!"),
    ("plugins", "List plugins"),
    ("say", "Send a message as yourself"),
    ("stop", "Gracefully stop the server"),
    ("summon", "Summon an entity at a position"),
    ("tell", "Send a private message to a player"),
    ("tp", "Teleport to coordinates"),
];

/// Splits a `/help` listing into its entries, in order.
fn help_entries(listing: &str) -> Vec<(String, String)> {
    let mut lines = listing.lines();
    assert_eq!(
        lines.next(),
        Some("--- Available Commands ---"),
        "the listing starts with its header:\n{listing}"
    );
    lines
        .map(|entry| {
            let entry = entry
                .strip_prefix("  /")
                .unwrap_or_else(|| panic!("entries read `  /name - description`: {entry:?}"));
            match entry.split_once(" - ") {
                Some((name, description)) => (name.to_string(), description.to_string()),
                None => (entry.to_string(), String::new()),
            }
        })
        .collect()
}

#[tokio::test]
async fn help_lists_every_command_once_like_the_registry_and_the_tree() {
    let server = TestServer::builder()
        .operators()
        .command(greet_command)
        .command(quiet_command)
        .start()
        .await;
    let mut bots = join_all(&server, &["Alice", "Bob"]).await;
    let (bob, _) = bots.pop().unwrap();
    let (alice, tree) = bots.pop().unwrap();

    // The registry on the server: canonical names with their descriptions.
    let registry: Vec<(String, String)> = server
        .with_world(|world| {
            let registry = world.resource::<CommandRegistry>();
            let mut names: Vec<String> = registry
                .command_names()
                .into_iter()
                .map(str::to_string)
                .collect();
            names.sort();
            names
                .into_iter()
                .map(|name| {
                    let description = registry.description(&name).unwrap().to_string();
                    (name, description)
                })
                .collect()
        })
        .await;
    let mut expected: Vec<(String, String)> = DEFAULT_COMMANDS
        .iter()
        .map(|(name, description)| (name.to_string(), description.to_string()))
        .chain([
            ("greet".to_string(), "Say hello to the server".to_string()),
            ("quiet".to_string(), String::new()),
        ])
        .collect();
    expected.sort();
    assert_eq!(
        registry, expected,
        "the registry holds the documented commands"
    );

    // The tree the client got at join: every canonical name and every
    // documented alias, each once, and nothing else.
    let tree_names: BTreeSet<&str> = tree.iter().map(String::as_str).collect();
    assert_eq!(tree_names.len(), tree.len(), "no duplicate in {tree:?}");
    let canonical: BTreeSet<&str> = registry.iter().map(|(name, _)| name.as_str()).collect();
    assert!(
        canonical.is_subset(&tree_names),
        "every command is in the tree: {tree:?}"
    );
    let aliases: BTreeSet<String> = tree_names
        .difference(&canonical)
        .map(|alias| alias.to_string())
        .collect();
    let resolved: BTreeMap<String, String> = server
        .with_world(move |world| {
            let registry = world.resource::<CommandRegistry>();
            aliases
                .into_iter()
                .map(|alias| {
                    let target = registry.resolve(&alias).unwrap_or("<none>").to_string();
                    (alias, target)
                })
                .collect()
        })
        .await;
    let documented_aliases: BTreeMap<String, String> = [
        ("gm", "gamemode"),
        ("hey", "greet"),
        ("msg", "tell"),
        ("pl", "plugins"),
    ]
    .into_iter()
    .map(|(alias, target)| (alias.to_string(), target.to_string()))
    .collect();
    assert_eq!(
        resolved, documented_aliases,
        "the other tree entries are the documented aliases"
    );

    // `/help`: one chat packet listing each canonical name once, sorted, with
    // its description, and no alias.
    let reply = replies_to(&alice, "help").await;
    let [(listing, color)] = &reply[..] else {
        panic!("/help answers with one chat packet: {reply:?}");
    };
    assert_eq!(color, "white");
    assert_eq!(
        help_entries(listing),
        registry,
        "/help agrees with the registry:\n{listing}"
    );
    assert!(listing.contains("\n  /quiet\n"), "{listing}");
    assert_nothing_received(&bob).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn help_for_one_command_resolves_aliases_and_has_no_pages() {
    let server = TestServer::builder()
        .operators()
        .command(quiet_command)
        .start()
        .await;
    let mut bots = join_all(&server, &["Alice", "Bob"]).await;
    let (bob, _) = bots.pop().unwrap();
    let (alice, _) = bots.pop().unwrap();

    let gamemode = vec![line(
        "--- /gamemode ---\n  Change game mode\n  Usage: /gamemode <mode:gamemode>",
        "white",
    )];
    assert_eq!(replies_to(&alice, "help gamemode").await, gamemode);
    assert_eq!(
        replies_to(&alice, "help gm").await,
        gamemode,
        "an alias shows its command"
    );
    assert_eq!(
        replies_to(&alice, "help ping").await,
        [line("--- /ping ---\n  Pong!\n  Usage: /ping", "white")]
    );
    assert_eq!(
        replies_to(&alice, "help quiet").await,
        [line("--- /quiet ---\n  Usage: /quiet", "white")],
        "a command without a description shows its usage only"
    );
    assert_eq!(
        replies_to(&alice, "help 2").await,
        [line("Unknown command: /2", "red")],
        "/help has no pages: a number is a command name"
    );
    assert_eq!(
        replies_to(&alice, "help ping extra").await,
        [
            line("Too many arguments: expected 1, got 2", "red"),
            line("Usage: /help [command:string]", "gray"),
        ]
    );
    assert_nothing_received(&bob).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn help_follows_the_registry_when_it_holds_little_or_nothing() {
    // Only `/help` registered: it lists itself, then a command added while
    // the player is online.
    let server = TestServer::builder()
        .without_default_commands()
        .command(help_command)
        .start()
        .await;
    let mut bots = join_all(&server, &["Alice"]).await;
    let (alice, tree) = bots.pop().unwrap();
    assert_eq!(tree, ["help"]);
    assert_eq!(
        replies_to(&alice, "help").await,
        [line(
            "--- Available Commands ---\n  /help - List available commands",
            "white"
        )]
    );
    assert_eq!(
        replies_to(&alice, "ping").await,
        [line("Unknown command: /ping", "red")]
    );

    server
        .with_world(|world| {
            world
                .resource_mut::<CommandRegistry>()
                .register(greet_command());
        })
        .await;
    assert_eq!(
        replies_to(&alice, "help").await,
        [line(
            "--- Available Commands ---\n  /greet - Say hello to the server\n  /help - List available commands",
            "white"
        )],
        "/help reads the registry when it runs"
    );
    assert_eq!(replies_to(&alice, "hey").await, [line("Hello!", "white")]);
    alice.assert_healthy();
    server.stop().await;

    // Nothing registered: the tree is empty and `/help` is an unknown
    // command like any other.
    let server = TestServer::builder()
        .without_default_commands()
        .start()
        .await;
    let mut bots = join_all(&server, &["Alice"]).await;
    let (alice, tree) = bots.pop().unwrap();
    assert!(tree.is_empty(), "no command in the tree: {tree:?}");
    assert_eq!(
        replies_to(&alice, "help").await,
        [line("Unknown command: /help", "red")]
    );
    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn ping_answers_only_the_sender_every_time() {
    let server = TestServer::start().await;
    let mut bots = join_all(&server, &["Alice", "Bob"]).await;
    let (bob, _) = bots.pop().unwrap();
    let (alice, _) = bots.pop().unwrap();

    for _ in 0..3 {
        alice.command("ping").await;
    }
    for _ in 0..2 {
        alice.chat("/ping").await;
    }
    assert_eq!(
        chat_lines(&alice).await,
        vec![line("Pong!", "white"); 5],
        "one Pong! per /ping"
    );
    assert_nothing_received(&bob).await;

    bob.command("ping").await;
    assert_eq!(chat_lines(&bob).await, [line("Pong!", "white")]);
    assert_nothing_received(&alice).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

/// `/plugins` and `/pl` on a server whose application set `plugins` up.
async fn plugins_reply(plugins: impl FnOnce(&mut App) + Send + 'static) -> Vec<Line> {
    let server = TestServer::builder().plugin(plugins).start().await;
    let mut bots = join_all(&server, &["Alice", "Bob"]).await;
    let (bob, _) = bots.pop().unwrap();
    let (alice, _) = bots.pop().unwrap();

    let reply = replies_to(&alice, "plugins").await;
    assert_eq!(replies_to(&alice, "pl").await, reply, "/pl is /plugins");
    assert_nothing_received(&bob).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
    reply
}

#[tokio::test]
async fn plugins_lists_the_application_names_or_says_there_are_none() {
    let none = [line("No plugins loaded.", "white")];
    assert_eq!(
        plugins_reply(|_| {}).await,
        none,
        "a server without a PluginList lists no plugins"
    );
    assert_eq!(
        plugins_reply(|app| {
            app.insert_resource(PluginList(Vec::new()));
        })
        .await,
        none
    );
    assert_eq!(
        plugins_reply(|app| {
            app.insert_resource(PluginList(vec![
                "Zeta".to_string(),
                "alpha".to_string(),
                "Mid Plugin".to_string(),
            ]));
        })
        .await,
        [line("Plugins (3): Zeta, alpha, Mid Plugin", "white")],
        "the names the application gave, in its order"
    );
}

fn unknown(name: &str) -> Vec<Line> {
    vec![line(&format!("Unknown command: /{name}"), "red")]
}

#[tokio::test]
async fn invalid_input_gets_one_controlled_reply() {
    let server = TestServer::builder().operators().start().await;
    let mut bots = join_all(&server, &["Alice", "Bob"]).await;
    let (bob, _) = bots.pop().unwrap();
    let (alice, _) = bots.pop().unwrap();

    let pong = vec![line("Pong!", "white")];
    let help_ping = vec![line("--- /ping ---\n  Pong!\n  Usage: /ping", "white")];
    let mut cases: Vec<(String, Vec<Line>)> = vec![
        ("nope".into(), unknown("nope")),
        ("HELP".into(), unknown("HELP")),
        ("Ping".into(), unknown("Ping")),
        ("/ping".into(), unknown("/ping")),
        // No command name at all: the vanilla client sends `/` alone as an
        // empty command.
        ("".into(), unknown("")),
        ("   ".into(), unknown("")),
        // Spaces around and between words are not part of any token.
        ("  ping  ".into(), pong.clone()),
        ("help    ping".into(), help_ping),
        (
            "ping extra".into(),
            vec![
                line("Too many arguments: expected 0, got 1", "red"),
                line("Usage: /ping", "gray"),
            ],
        ),
        (
            "plugins a b c".into(),
            vec![
                line("Too many arguments: expected 0, got 3", "red"),
                line("Usage: /plugins", "gray"),
            ],
        ),
        (
            "gamemode".into(),
            vec![
                line(
                    "Missing required argument <mode> (expected gamemode)",
                    "red",
                ),
                line("Usage: /gamemode <mode:gamemode>", "gray"),
            ],
        ),
        ("héllo".into(), unknown("héllo")),
        ("💎 x".into(), unknown("💎")),
        ("コマンド".into(), unknown("コマンド")),
        // Control characters are not chat text: the echoed name drops them.
        ("pi\0ng".into(), unknown("ping")),
        ("\0".into(), unknown("")),
        ("\u{7}bell".into(), unknown("bell")),
        ("\u{1b}[31mred".into(), unknown("[31mred")),
        ("del\u{7f}".into(), unknown("del")),
        // The echoed name drops the `§c` formatting code (see
        // `unknown_names_are_echoed_without_formatting_codes`).
        ("\u{a7}cping".into(), unknown("ping")),
    ];
    // The vanilla chat box stops at 256 characters, `/` included; longer
    // lines only come from other clients.
    for length in [255, 1_000, 32_000] {
        let name = "x".repeat(length);
        cases.push((name.clone(), unknown(&name)));
    }

    for (input, expected) in &cases {
        let reply = replies_to(&alice, input).await;
        assert!(
            reply == *expected,
            "/{} got {:?}, expected {:?}",
            shorten(input),
            reply
                .iter()
                .map(|(text, color)| (shorten(text), color))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|(text, color)| (shorten(text), color))
                .collect::<Vec<_>>()
        );
        alice.assert_healthy();
    }

    // The player is still connected and the server still answers.
    assert_eq!(replies_to(&alice, "ping").await, pong);
    assert_nothing_received(&bob).await;
    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

/// A chat line with its text, its colour, and whether it is one plain
/// component: the client turns each `§` code it applies into a component of
/// its own.
fn styled_system_chat(packet: &ClientboundGamePacket) -> Option<(String, String, bool)> {
    let (text, color) = system_chat(packet)?;
    let ClientboundGamePacket::SystemChat(chat) = packet else {
        return None;
    };
    Some((text, color, chat.content.get_base().siblings.is_empty()))
}

/// An unknown name is echoed back without the `§` formatting codes and the
/// control characters in it, by the dispatcher and by `/help <name>`: the
/// client would apply the codes to the reply. The reply is still sent, to
/// the sender only.
#[tokio::test]
async fn unknown_names_are_echoed_without_formatting_codes() {
    let server = TestServer::start().await;
    let mut bots = join_all(&server, &["Alice", "Bob"]).await;
    let (bob, _) = bots.pop().unwrap();
    let (alice, _) = bots.pop().unwrap();

    for (input, echoed) in [
        ("§knope", "nope"),
        ("§4§lred§r alert", "red"),
        ("no§ape", "nope"),
        ("§§knope", "knope"),
        ("trailing§", "trailing"),
        ("help §knope", "nope"),
        ("help §4§lred§r", "red"),
        ("help no\0pe\u{7}", "nope"),
        ("help §§knope", "knope"),
    ] {
        for as_chat in [false, true] {
            if as_chat {
                alice.chat(&format!("/{input}")).await;
            } else {
                alice.command(input).await;
            }
            alice.sync().await;
            let reply = alice.queued(styled_system_chat);
            alice.clear();
            assert_eq!(
                reply,
                [(
                    format!("Unknown command: /{echoed}"),
                    "red".to_string(),
                    true
                )],
                "/{input:?} (as chat: {as_chat}) is answered with one plain red line"
            );
        }
    }

    assert_nothing_received(&bob).await;
    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

/// Logs a client in and plays it into the game state without ever sending
/// `PlayerLoaded`, so the server never marks it ready.
async fn join_without_loading(
    address: SocketAddr,
    name: &str,
) -> Connection<ClientboundGamePacket, ServerboundGamePacket> {
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
            profile_id: uuid::Uuid::from_u128(0x0008),
        })
        .await
        .expect("login start");
    loop {
        match connection.read().await.expect("a login packet") {
            ClientboundLoginPacket::LoginCompression(packet) => {
                connection.set_compression_threshold(packet.compression_threshold);
            }
            ClientboundLoginPacket::LoginFinished(_) => {
                connection
                    .write(ServerboundLoginAcknowledged {})
                    .await
                    .expect("login acknowledged");
                break;
            }
            other => panic!("unexpected login packet: {other:?}"),
        }
    }
    let mut connection = connection.config();
    connection
        .write(ServerboundClientInformation {
            information: ClientInformation::default(),
        })
        .await
        .expect("client information");
    loop {
        match connection.read().await.expect("a configuration packet") {
            ClientboundConfigPacket::SelectKnownPacks(packet) => connection
                .write(ServerboundSelectKnownPacks {
                    known_packs: packet.known_packs,
                })
                .await
                .expect("select known packs"),
            ClientboundConfigPacket::KeepAlive(packet) => connection
                .write(ConfigKeepAlive { id: packet.id })
                .await
                .expect("configuration keep-alive"),
            ClientboundConfigPacket::FinishConfiguration(_) => {
                connection
                    .write(ServerboundFinishConfiguration {})
                    .await
                    .expect("finish configuration");
                break;
            }
            ClientboundConfigPacket::Disconnect(packet) => {
                panic!("disconnected during configuration: {}", packet.reason)
            }
            _ => {}
        }
    }
    connection.game()
}

/// Reads game packets, answering keep-alives, until `f` accepts one.
async fn read_until<T>(
    connection: &mut Connection<ClientboundGamePacket, ServerboundGamePacket>,
    what: &str,
    mut f: impl FnMut(&ClientboundGamePacket) -> Option<T>,
) -> T {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let packet = connection
                .read()
                .await
                .unwrap_or_else(|error| panic!("reading while waiting for {what}: {error}"));
            if let ClientboundGamePacket::KeepAlive(keep_alive) = &packet {
                connection
                    .write(ServerboundKeepAlive { id: keep_alive.id })
                    .await
                    .expect("keep-alive");
            }
            if let ClientboundGamePacket::Disconnect(disconnect) = &packet {
                panic!("kicked while waiting for {what}: {}", disconnect.reason);
            }
            if let Some(value) = f(&packet) {
                return value;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

#[tokio::test]
async fn commands_sent_before_the_player_is_ready_are_answered() {
    let server = TestServer::start().await;
    let mut bots = join_all(&server, &["Bob"]).await;
    let (bob, _) = bots.pop().unwrap();

    let mut early = join_without_loading(server.address(), "Early").await;
    read_until(&mut early, "the play login", |packet| {
        matches!(packet, ClientboundGamePacket::Login(_)).then_some(())
    })
    .await;
    early
        .write(ServerboundChatCommand {
            command: "ping".to_string(),
        })
        .await
        .expect("send /ping");
    early
        .write(ServerboundChatCommand {
            command: "nope".to_string(),
        })
        .await
        .expect("send /nope");
    let mut replies = Vec::new();
    while replies.len() < 2 {
        replies.push(read_until(&mut early, "the command replies", system_chat).await);
    }
    assert_eq!(
        replies,
        [line("Pong!", "white"), unknown("nope").remove(0)],
        "replies in the order the commands were sent"
    );

    let ready = server
        .with_world(|world| {
            let mut players = world.query::<(&PlayerName, Has<PlayerReady>)>();
            players
                .iter(world)
                .find(|(name, _)| name.0 == "Early")
                .map(|(_, ready)| ready)
        })
        .await;
    assert_eq!(
        ready,
        Some(false),
        "the player was answered before it was ready"
    );
    assert_nothing_received(&bob).await;

    bob.assert_healthy();
    server.stop().await;
}

/// The tick every command was queued in, with its sender and name.
#[derive(Resource, Default, Clone)]
struct CommandLog(Arc<Mutex<Vec<(Entity, String, u64)>>>);

#[derive(Resource, Default)]
struct Tick(u64);

fn count_ticks(mut tick: ResMut<Tick>) {
    tick.0 += 1;
}

fn log_commands(event: On<ChatCommandEvent>, tick: Res<Tick>, log: Res<CommandLog>) {
    log.0
        .lock()
        .unwrap()
        .push((event.entity, event.command.clone(), tick.0));
}

#[tokio::test]
async fn commands_from_two_players_in_one_tick_reach_their_senders() {
    let log = CommandLog::default();
    let records = log.0.clone();
    let server = TestServer::builder()
        .plugin(move |app| {
            app.insert_resource(log)
                .init_resource::<Tick>()
                .add_systems(First, count_ticks)
                .add_observer(log_commands);
        })
        .start()
        .await;
    let mut bots = join_all(&server, &["Alice", "Bob"]).await;
    let (bob, _) = bots.pop().unwrap();
    let (alice, _) = bots.pop().unwrap();
    let alice_entity = server.player_entity(alice.uuid()).await;
    let bob_entity = server.player_entity(bob.uuid()).await;

    // Stop keep-alives, then let the last answers in, so that the only
    // packets the bots send next are the four commands.
    server
        .with_world(|world| world.resource_mut::<KeepAliveTicker>().interval_ticks = u32::MAX)
        .await;
    for _ in 0..2 {
        alice.sync().await;
        bob.sync().await;
    }
    alice.clear();
    bob.clear();

    // Hold the game thread until the four commands wait in its inbox, so the
    // next tick takes them all.
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let hold = server.with_world(move |world| {
        let _ = held_tx.send(());
        let deadline = Instant::now() + Duration::from_secs(15);
        while world.resource::<NetworkChannels>().incoming.len() < 4 {
            assert!(
                Instant::now() < deadline,
                "the four commands never reached the server"
            );
            std::thread::yield_now();
        }
    });
    let send = async {
        held_rx.await.expect("the game thread was held");
        alice.command("help ping").await;
        bob.command("ping").await;
        alice.command("alice-only").await;
        bob.command("bob-only").await;
    };
    tokio::join!(hold, send);

    assert_eq!(
        chat_lines(&alice).await,
        [
            line("--- /ping ---\n  Pong!\n  Usage: /ping", "white"),
            unknown("alice-only").remove(0),
        ]
    );
    assert_eq!(
        chat_lines(&bob).await,
        [line("Pong!", "white"), unknown("bob-only").remove(0)]
    );

    let records = records.lock().unwrap().clone();
    let ticks: BTreeSet<u64> = records.iter().map(|(_, _, tick)| *tick).collect();
    assert_eq!(ticks.len(), 1, "all four commands in one tick: {records:?}");
    let senders: BTreeMap<String, Entity> = records
        .into_iter()
        .map(|(entity, command, _)| (command, entity))
        .collect();
    assert_eq!(
        senders,
        BTreeMap::from([
            ("alice-only".to_string(), alice_entity),
            ("bob-only".to_string(), bob_entity),
            ("help".to_string(), alice_entity),
            ("ping".to_string(), bob_entity),
        ])
    );

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}
