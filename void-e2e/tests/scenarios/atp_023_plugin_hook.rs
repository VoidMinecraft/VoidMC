//! VMC-ATP-023: an application extends the server through the Bevy plugin
//! hook (`VoidServer::add_plugin`, which `TestServer::builder().plugin` wraps)
//! without touching framework code.

use bevy_app::{App, Startup, Update};
use bevy_ecs::prelude::*;
use voidmc::{Command, CommandBuilder, CommandRegistry, Messages, PluginList};
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::{Bot, TestServer};

const GREETING: &str = "hello from a plugin";

/// Shared resource the plugin inserts and its consumers read.
#[derive(Resource)]
struct Greeting(&'static str);

/// Advanced by the plugin's `Update` system, once per tick.
#[derive(Resource, Default)]
struct PluginTicks(u64);

/// Written by the plugin's `Startup` system and its observer.
#[derive(Resource, Default)]
struct ProbeState {
    ticks_at_startup: Option<u64>,
    probes: u32,
}

/// Triggered by `/probe`, handled by the plugin's observer.
#[derive(Event)]
struct Probed {
    player: Entity,
    ticks: u64,
}

fn count_ticks(mut ticks: ResMut<PluginTicks>) {
    ticks.0 += 1;
}

fn record_startup(ticks: Res<PluginTicks>, mut state: ResMut<ProbeState>) {
    state.ticks_at_startup = Some(ticks.0);
}

fn on_probed(
    probe: On<Probed>,
    greeting: Res<Greeting>,
    mut state: ResMut<ProbeState>,
    messages: Messages,
) {
    state.probes += 1;
    let startup = state
        .ticks_at_startup
        .map_or_else(|| "never".to_string(), |ticks| ticks.to_string());
    messages
        .message(
            probe.player,
            format!(
                "probe #{} greeting={} ticks={} startup_ticks={startup}",
                state.probes, greeting.0, probe.ticks
            ),
        )
        .send();
}

fn probe_command() -> Command {
    CommandBuilder::new("probe")
        .description("Reads the probe plugin's state")
        .handler(|ctx| {
            let player = ctx.entity;
            let ticks = ctx.with_world(|world| world.resource::<PluginTicks>().0);
            ctx.with_world_mut(|world| world.trigger(Probed { player, ticks }));
        })
        .build()
}

/// The whole extension: resources, a startup and a per-tick system, an
/// observer, and a command that consumes them, installed from the outside.
fn probe_plugin(app: &mut App) {
    app.insert_resource(Greeting(GREETING))
        .insert_resource(PluginList(vec!["probe-plugin".to_string()]))
        .init_resource::<PluginTicks>()
        .init_resource::<ProbeState>()
        .add_systems(Startup, record_startup)
        .add_systems(Update, count_ticks)
        .add_observer(on_probed);
    app.world_mut()
        .resource_mut::<CommandRegistry>()
        .register(probe_command());
}

struct ProbeReply {
    number: u32,
    greeting: String,
    ticks: u64,
    startup_ticks: String,
}

fn parse_reply(text: &str) -> Option<ProbeReply> {
    let rest = text.strip_prefix("probe #")?;
    let (number, rest) = rest.split_once(" greeting=")?;
    let (greeting, rest) = rest.split_once(" ticks=")?;
    let (ticks, startup_ticks) = rest.split_once(" startup_ticks=")?;
    Some(ProbeReply {
        number: number.parse().ok()?,
        greeting: greeting.to_string(),
        ticks: ticks.parse().ok()?,
        startup_ticks: startup_ticks.to_string(),
    })
}

async fn probe(bot: &Bot) -> ProbeReply {
    bot.command("probe").await;
    bot.expect("the probe observer's reply", |packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => parse_reply(&chat.content.to_string()),
        _ => None,
    })
    .await
}

#[tokio::test]
async fn plugin_resources_systems_and_observers_serve_its_command() {
    let server = TestServer::builder().plugin(probe_plugin).start().await;
    let alice = server.join("Alice").await;

    // Applied before gameplay: the command the plugin registered is in the
    // tree the client receives while joining.
    let (nodes, root) = alice
        .expect("the command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => {
                Some((tree.entries.clone(), tree.root_index as usize))
            }
            _ => None,
        })
        .await;
    assert!(
        nodes[root]
            .children
            .iter()
            .any(|child| nodes[*child as usize].name() == Some("probe")),
        "the plugin's /probe is missing from the join command tree"
    );

    let first = probe(&alice).await;
    assert_eq!(first.number, 1, "the observer ran once for the first probe");
    assert_eq!(
        first.greeting, GREETING,
        "the observer reads the plugin's resource"
    );
    assert_eq!(
        first.startup_ticks, "0",
        "the plugin's Startup system ran before its first Update tick"
    );
    assert!(
        first.ticks > 0,
        "the plugin's Update system ran before the player could act"
    );

    server.wait_ticks(3).await;
    let second = probe(&alice).await;
    assert_eq!(
        second.number, 2,
        "the observer's write to the shared resource persisted"
    );
    assert!(
        second.ticks > first.ticks,
        "the Update system keeps running every tick: {} then {}",
        first.ticks,
        second.ticks
    );

    // A built-in consumer reads a resource the plugin inserted.
    alice.command("plugins").await;
    alice
        .expect("the /plugins reply", |packet| match packet {
            ClientboundGamePacket::SystemChat(chat) => {
                (chat.content.to_string() == "Plugins (1): probe-plugin").then_some(())
            }
            _ => None,
        })
        .await;

    let probes = server
        .with_world(|world| world.resource::<ProbeState>().probes)
        .await;
    assert_eq!(probes, 2, "the server holds the observer's writes");

    alice.assert_healthy();
    server.stop().await;
}
