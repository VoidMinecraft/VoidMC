//! VMC-ATP-021: an application or plugin registers its own command with
//! `CommandBuilder`, through `VoidServer::add_command` (which
//! `TestServer::builder().command` wraps) or straight into the
//! `CommandRegistry` from a plugin, without touching framework code.

use std::collections::BTreeSet;
use std::sync::Arc;

use bevy_app::App;
use bevy_ecs::prelude::*;
use voidmc::{
    ArgParser, Command, CommandBuilder, CommandRegistry, GreedyStringArg, IntegerArg, MessageArg,
    StringArg,
};
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::protocol::packets::game::c_commands::{
    BrigadierNodeStub, BrigadierParser, NodeType,
};
use voidmc_e2e::{Bot, TestServer, TestServerBuilder};

const DESCRIPTION: &str = "Wave at the server a few times";

/// Application state the command's handler writes to.
#[derive(Resource, Default)]
struct Waves(i32);

fn wave_command() -> Command {
    CommandBuilder::new("wave")
        .description(DESCRIPTION)
        .alias("hi")
        .arg("times", IntegerArg::new(1, 5))
        .handler(|ctx| {
            let times = *ctx.get::<i32>("times").expect("a required argument");
            let name = ctx.player_name().unwrap_or_default();
            let total = ctx.with_world_mut(|world| {
                let mut waves = world.resource_mut::<Waves>();
                waves.0 += times;
                waves.0
            });
            ctx.reply(&format!("{name} waved {times} time(s), {total} in total"));
        })
        .build()
}

/// The application, minus any command registration.
fn waves_app(app: &mut App) {
    app.init_resource::<Waves>();
}

/// The same application, registering `/wave` from inside its plugin.
fn waves_app_with_command(app: &mut App) {
    waves_app(app);
    app.world_mut()
        .resource_mut::<CommandRegistry>()
        .register(wave_command());
}

fn system_chat(packet: &ClientboundGamePacket) -> Option<String> {
    match packet {
        ClientboundGamePacket::SystemChat(chat) => Some(chat.content.to_string()),
        _ => None,
    }
}

/// Runs `command` and returns the first system chat line `f` accepts.
async fn reply_to(bot: &Bot, command: &str, what: &str, f: impl Fn(&str) -> bool) -> String {
    bot.command(command).await;
    bot.expect(what, |packet| system_chat(packet).filter(|text| f(text)))
        .await
}

/// The command tree the bot received while joining, as its top-level
/// literals plus the nodes and root index for deeper checks.
async fn command_tree(bot: &Bot) -> (Vec<BrigadierNodeStub>, usize, BTreeSet<String>) {
    let (nodes, root) = bot
        .expect("the command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => {
                Some((tree.entries.clone(), tree.root_index as usize))
            }
            _ => None,
        })
        .await;
    let top_level = nodes[root]
        .children
        .iter()
        .filter_map(|child| nodes[*child as usize].name().map(str::to_string))
        .collect();
    (nodes, root, top_level)
}

fn top_level_node<'a>(
    nodes: &'a [BrigadierNodeStub],
    root: usize,
    name: &str,
) -> &'a BrigadierNodeStub {
    nodes[root]
        .children
        .iter()
        .map(|child| &nodes[*child as usize])
        .find(|node| node.name() == Some(name))
        .unwrap_or_else(|| panic!("/{name} missing from the command tree"))
}

/// Core commands keep working whether `/wave` is registered or not.
async fn unrelated_commands_work(bot: &Bot, top_level: &BTreeSet<String>) {
    for command in ["help", "ping", "tp", "give"] {
        assert!(
            top_level.contains(command),
            "/{command} missing from {top_level:?}"
        );
    }
    reply_to(bot, "ping", "the /ping reply", |text| text == "Pong!").await;
}

/// Joins a player and checks that `/wave` is discovered and runs its
/// handler. Returns the top-level commands the player discovered.
async fn wave_is_registered(server: &TestServer) -> BTreeSet<String> {
    let alice = server.join("Alice").await;

    let (nodes, root, top_level) = command_tree(&alice).await;
    let wave = top_level_node(&nodes, root, "wave");
    assert!(
        matches!(&wave.node_type, NodeType::Literal { .. }),
        "/wave is a literal: {wave:?}"
    );
    assert!(
        !wave.is_executable,
        "/wave needs its argument before it can run"
    );
    let [times] = wave.children[..] else {
        panic!("/wave has exactly one child: {wave:?}");
    };
    let times = &nodes[times as usize];
    assert!(
        matches!(
            &times.node_type,
            NodeType::Argument { name, parser: BrigadierParser::Integer(range), .. }
                if name == "times" && range.min == Some(1) && range.max == Some(5)
        ),
        "the argument is the builder's bounded integer: {times:?}"
    );
    assert!(times.is_executable, "/wave <times> runs: {times:?}");
    let alias = top_level_node(&nodes, root, "hi");
    assert_eq!(
        alias.children, wave.children,
        "the alias leads to the same argument"
    );
    unrelated_commands_work(&alice, &top_level).await;

    let help = reply_to(&alice, "help", "the /help listing", |text| {
        text.starts_with("--- Available Commands ---")
    })
    .await;
    assert!(
        help.lines()
            .any(|line| line.trim() == format!("/wave - {DESCRIPTION}")),
        "/help lists /wave with its description:\n{help}"
    );
    let details = reply_to(&alice, "help wave", "the /help wave details", |text| {
        text.starts_with("--- /wave ---")
    })
    .await;
    assert!(
        details.contains(DESCRIPTION) && details.contains("Usage: /wave <times:"),
        "/help wave shows the description and usage:\n{details}"
    );

    let reply = reply_to(&alice, "wave 3", "the /wave reply", |text| {
        text.contains("waved")
    })
    .await;
    assert_eq!(reply, "Alice waved 3 time(s), 3 in total");
    let reply = reply_to(&alice, "hi 2", "the /hi reply", |text| {
        text.contains("waved")
    })
    .await;
    assert_eq!(reply, "Alice waved 2 time(s), 5 in total");
    let waves = server.with_world(|world| world.resource::<Waves>().0).await;
    assert_eq!(waves, 5, "the handler ran on the server for both names");

    alice.assert_healthy();
    top_level
}

/// Joins a player and checks that `/wave` is nowhere to be found. Returns
/// the top-level commands the player discovered.
async fn wave_is_absent(server: &TestServer) -> BTreeSet<String> {
    let alice = server.join("Alice").await;

    let (_, _, top_level) = command_tree(&alice).await;
    for name in ["wave", "hi"] {
        assert!(
            !top_level.contains(name),
            "/{name} is not registered but is in {top_level:?}"
        );
    }
    unrelated_commands_work(&alice, &top_level).await;

    let help = reply_to(&alice, "help", "the /help listing", |text| {
        text.starts_with("--- Available Commands ---")
    })
    .await;
    assert!(
        help.lines().count() > 1,
        "/help still lists the other commands:\n{help}"
    );
    assert!(
        !help.contains("/wave"),
        "/help does not list the unregistered /wave:\n{help}"
    );
    reply_to(&alice, "help wave", "/help not knowing /wave", |text| {
        text == "Unknown command: /wave"
    })
    .await;

    for command in ["wave 3", "hi 2"] {
        let name = command.split_once(' ').unwrap().0;
        let expected = format!("Unknown command: /{name}");
        reply_to(&alice, command, "the unknown-command reply", |text| {
            text == expected
        })
        .await;
    }
    alice.sync().await;
    let replies = alice.queued(system_chat);
    assert!(
        !replies.iter().any(|text| text.contains("waved")),
        "no handler answered: {replies:?}"
    );
    let waves = server.with_world(|world| world.resource::<Waves>().0).await;
    assert_eq!(waves, 0, "no handler ran on the server");

    alice.assert_healthy();
    top_level
}

#[tokio::test]
async fn restarting_without_the_registration_removes_the_command() {
    let with_command = |builder: TestServerBuilder| builder.plugin(waves_app).command(wave_command);

    let server = with_command(TestServer::builder().operators())
        .start()
        .await;
    let registered = wave_is_registered(&server).await;
    server.stop().await;

    let server = TestServer::builder()
        .operators()
        .plugin(waves_app)
        .start()
        .await;
    let unregistered = wave_is_absent(&server).await;
    server.stop().await;

    let removed: BTreeSet<&str> = registered
        .difference(&unregistered)
        .map(String::as_str)
        .collect();
    assert_eq!(
        removed,
        BTreeSet::from(["hi", "wave"]),
        "dropping the registration removes exactly /wave and its alias"
    );
    assert!(
        unregistered.is_subset(&registered),
        "no other command depends on the registration: {unregistered:?} vs {registered:?}"
    );
}

#[tokio::test]
async fn command_registered_from_a_plugin_is_discovered_and_runs() {
    let server = TestServer::builder()
        .operators()
        .plugin(waves_app_with_command)
        .start()
        .await;
    wave_is_registered(&server).await;
    server.stop().await;
}

/// `/<name> <topic> [--loud|-l] <text>...`, whose handler replies with what
/// it was given, quoted: the text argument uses `parser`.
fn note_command(name: &str, parser: Arc<dyn ArgParser>) -> Command {
    CommandBuilder::new(name)
        .description("Pin a note to a topic")
        .arg("topic", StringArg::single_word())
        .flag("loud", Some('l'), "Shout the note")
        .arg_variadic_required("text", parser)
        .handler(|ctx| {
            let topic = ctx.get::<String>("topic").expect("a required argument");
            let text = ctx.get::<String>("text").expect("a required argument");
            let loud = ctx.flag("loud");
            ctx.reply(&format!("topic={topic:?} loud={loud} text={text:?}"));
        })
        .build()
}

/// A text argument of an application's command gets the rest of the line as
/// the player typed it, like a vanilla greedy string, with every kind of
/// text parser; the other arguments and the flags are read as before.
#[tokio::test]
async fn a_text_argument_gets_the_rest_of_the_line_as_typed() {
    let server = TestServer::builder()
        .command(|| note_command("note", Arc::new(GreedyStringArg)))
        .command(|| note_command("memo", StringArg::greedy()))
        .command(|| note_command("shout", Arc::new(MessageArg)))
        .start()
        .await;
    let alice = server.join("Alice").await;

    for name in ["note", "memo", "shout"] {
        for (line, expected) in [
            ("idea a   b", r#"topic="idea" loud=false text="a   b""#),
            (
                "   idea    a   b",
                r#"topic="idea" loud=false text="a   b""#,
            ),
            ("idea a b  ", r#"topic="idea" loud=false text="a b  ""#),
            ("idea a\tb", r#"topic="idea" loud=false text="a\tb""#),
            // A flag before the text is still a flag ...
            (
                "idea --loud a   b",
                r#"topic="idea" loud=true text="a   b""#,
            ),
            ("-l  idea  a  b", r#"topic="idea" loud=true text="a  b""#),
            // ... one after its first word is text, as before ...
            (
                "idea a  --loud  b",
                r#"topic="idea" loud=false text="a  --loud  b""#,
            ),
            // ... and `--` ends the flags: what follows is kept verbatim.
            (
                "idea --  --loud   b",
                r#"topic="idea" loud=false text="--loud   b""#,
            ),
        ] {
            let command = format!("{name} {line}");
            let reply = reply_to(&alice, &command, "the note reply", |text| {
                text.starts_with("topic=")
            })
            .await;
            assert_eq!(reply, expected, "/{command:?}");
        }
    }

    alice.assert_healthy();
    server.stop().await;
}
