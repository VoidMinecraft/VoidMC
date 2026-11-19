use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use bevy_app::Update;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_flame::{FlameLayer, FlushGuard};
use tracing_subscriber::prelude::*;
use voidmc::components::PlayerName;
use voidmc::events::{PlayerReadyEvent, PlayerStartDiggingEvent};
use voidmc::item_behavior::{ItemBehavior, ItemBehaviorRegistry, ItemUseContext, UseResult};
use voidmc::plugins::redstone::RedstonePlugin;
use voidmc::{
    Audience, CommandBuilder, CommandRegistry, Inventory, ItemStack, Messages, ObservabilityConfig,
    On, Query, ServerConfig, ServerConfigBuilder, TextColor, VoidServer, register_default_commands,
};
use voidmc_navigation::{NavRemotePlugin, NavigationPlugin, RemoteAccess};
use voidmc_vanilla_commands::{Access, VanillaCommandsPlugin};
use voidmc_world_io::{PersistenceConfig, WorldPersistencePlugin};
use voidmc_worldedit::WorldEditPlugin;

mod biome;
mod boss_bar;
mod circle;
#[cfg(test)]
mod diagnostics_tests;
mod effects;
mod entities;
mod environment;
mod menu;
mod navigation;
mod particle;
mod redstone;
mod sidebar;
mod sign;
mod sound;
mod tab_list;
mod title;
mod toast;
mod world_border;
mod worldedit;

struct LogGuards {
    _file: WorkerGuard,
    _flame: Option<FlushGuard<TraceFile>>,
}

/// Where the flame layer writes its folded stacks.
type TraceFile = BufWriter<File>;

struct MetricsEnv {
    metrics_debug: bool,
    flame_enabled: bool,
    tps_output: Option<String>,
    flame_output: Option<String>,
    flame_include_idle: bool,
    packet_debug: bool,
}

impl MetricsEnv {
    fn from_env() -> Self {
        Self::from_lookup(env_var)
    }

    /// Reads the diagnostics settings through `var`, which returns a
    /// variable's value if it is set.
    fn from_lookup(var: impl Fn(&str) -> Option<String>) -> Self {
        let flag = |name| is_on(var(name));
        let string = |name| non_empty(var(name));
        let metrics_debug = flag("VOID_METRICS_DEBUG");
        let tps_output = string("VOID_TPS_OUTPUT");
        let flame_output = string("VOID_FLAME_OUTPUT");
        let flame_include_idle = flag("VOID_FLAME_INCLUDE_IDLE");
        let packet_debug = flag("VOID_PACKET_DEBUG");
        let metrics_mode = var("VOID_METRICS_MODE");
        let flame_enabled =
            matches!(metrics_mode.as_deref(), Some("flame")) || flame_output.is_some();

        Self {
            metrics_debug,
            flame_enabled,
            tps_output,
            flame_output,
            flame_include_idle,
            packet_debug,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metrics_env = MetricsEnv::from_env();
    let _log_guards = setup_logging(&metrics_env)?;

    VoidServer::new(server_config(&metrics_env, env_var))
        .add_plugin(|app| {
            // Persist modified chunks to ./world and reload them on restart.
            app.add_plugins(WorldPersistencePlugin::new(
                PersistenceConfig::new("world").save_interval_ticks(200),
            ));
        })
        .add_plugin(|app| {
            // Vanilla /team and /scoreboard, open to everyone on this demo server.
            app.add_plugins(VanillaCommandsPlugin {
                access: Access::Everyone,
            });
        })
        .add_plugin(|app| {
            // Register all default commands
            let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
            register_default_commands(&mut registry, &[]);
            registry.register(circle::circle_command());
            registry.register(boss_bar::bossbar_command());
            registry.register(sound::sound_command());
            registry.register(biome::biome_command());
            registry.register(particle::particle_command());
            registry.register(entities::spawn_command());
            registry.register(entities::display_command());
            registry.register(sign::sign_command());
            registry.register(menu::menu_command());
            registry.register(effects::effect_command());
            registry.register(effects::speed_command());
            registry.register(world_border::border_command());
            registry.register(environment::time_command());
            registry.register(environment::weather_command());
            registry.register(title::title_command());
            registry.register(toast::toast_command());
            registry.register(tab_list::tablist_command());
            registry.register(tab_list::nick_command());
            environment::spawn_environment(app.world_mut());

            // Observe block-breaking events
            app.add_observer(on_player_dig);
            app.add_observer(boss_bar::despawn_bossbar_on_quit);
            app.world_mut().spawn(sidebar::altitude_board());
            app.add_systems(
                Update,
                (
                    circle::circle_system,
                    entities::shield_system,
                    sidebar::altitude_system,
                ),
            );
        })
        .add_plugin(|app| {
            app.add_plugins((
                NavigationPlugin::new(),
                NavRemotePlugin::default().access(RemoteAccess::Everyone),
            ));
            app.init_resource::<navigation::DemoSpawned>();
            app.add_observer(navigation::spawn_on_first_join);
            app.world_mut()
                .resource_mut::<CommandRegistry>()
                .register(navigation::navdemo_command());
        })
        .add_plugin(|app| {
            app.add_plugins(WorldEditPlugin::default());
            app.add_observer(worldedit::grant_wand);
        })
        .add_plugin(|app| {
            // Levers, buttons, flat redstone dust and note blocks; `/circuit`
            // builds a test circuit next to the player.
            app.add_plugins(RedstonePlugin);
            app.world_mut()
                .resource_mut::<CommandRegistry>()
                .register(redstone::circuit_command());
        })
        .add_plugin(|app| {
            // Demo of the item-behaviour API: a stick becomes a "glowstone wand",
            // overriding the default right-click action.
            app.world_mut()
                .resource_mut::<ItemBehaviorRegistry>()
                .register_for("minecraft:stick", WandBehavior);

            // Give every player a small starter kit when they join.
            app.add_observer(give_starter_kit);
            app.add_observer(announce_join);
        })
        .add_command(
            CommandBuilder::new("hello")
                .description("Greet the player")
                .handler(|ctx| {
                    ctx.reply("Hello from void-example!");
                })
                .build(),
        )
        .run();

    Ok(())
}

/// The configuration the example server runs with. `var` looks up the
/// address variables, as [`MetricsEnv::from_lookup`] does the diagnostics.
fn server_config(metrics_env: &MetricsEnv, var: impl Fn(&str) -> Option<String>) -> ServerConfig {
    let mut config_builder = ServerConfigBuilder::new()
        .spawn_chunk_radius(4)
        .initial_chunk_radius(4)
        .motd("Welcome to Void Server!")
        .max_players(100)
        .configure_registries(biome::register_crimson_sky);

    if metrics_env.metrics_debug {
        config_builder = config_builder.metrics_debug(true);
    }

    if let Some(tps_output) = metrics_env.tps_output.clone() {
        config_builder = config_builder.metrics_tps_output(tps_output);
    }

    // `VOID_LISTEN_ADDR=127.0.0.1:0` picks a free port, as the diagnostics
    // tests do. Unset: the default `127.0.0.1:25565`.
    if let Some(address) = non_empty(var("VOID_LISTEN_ADDR")) {
        config_builder = config_builder.address(address);
    }

    // `VOID_OBSERVABILITY_ADDR=9464` serves /health and /metrics on
    // 127.0.0.1:9464; a full `host:port` picks the interface. Unset: off.
    if let Some(address) = non_empty(var("VOID_OBSERVABILITY_ADDR")) {
        config_builder = config_builder.observability(observability_config(&address));
    }

    config_builder.build()
}

fn setup_logging(metrics_env: &MetricsEnv) -> Result<LogGuards, Box<dyn std::error::Error>> {
    // Create logs directory if it doesn't exist
    std::fs::create_dir_all("logs")?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let log_file = std::fs::File::create(format!("logs/void-{timestamp}.log"))?;
    let (non_blocking, guard) = tracing_appender::non_blocking(log_file);

    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_level(true)
        .with_file(true)
        .pretty()
        .with_writer(std::io::stderr);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_level(true)
        .with_file(true)
        .with_writer(non_blocking);

    // An unusable trace path turns flame profiling off instead of stopping the
    // server; the warning is logged once the subscriber below is installed.
    let flame_setup = metrics_env.flame_enabled.then(|| {
        let flame_path = PathBuf::from(
            metrics_env
                .flame_output
                .clone()
                .unwrap_or_else(|| format!("logs/trace-{timestamp}.folded")),
        );
        let flame = flame_layer(&flame_path, metrics_env.flame_include_idle);
        (flame_path, flame)
    });

    let mut env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if metrics_env.packet_debug
        && let Ok(directive) = "voidmc::network=debug".parse()
    {
        env_filter = env_filter.add_directive(directive);
    }

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer);

    let flame_guard = match flame_setup {
        Some((path, Ok((flame_layer, flame_guard)))) => {
            registry.with(flame_layer).init();
            tracing::info!(path = %path.display(), "Writing flame trace to {}", path.display());
            Some(flame_guard)
        }
        Some((path, Err(error))) => {
            registry.init();
            tracing::warn!(
                error = %error,
                path = %path.display(),
                "Cannot write the flame trace; flame profiling is off"
            );
            None
        }
        None => {
            registry.init();
            None
        }
    };

    Ok(LogGuards {
        _file: guard,
        _flame: flame_guard,
    })
}

/// The folded-stack layer writing to `path`, creating missing parent
/// directories, and the guard that flushes it when dropped.
fn flame_layer<S>(
    path: &Path,
    include_idle: bool,
) -> std::io::Result<(FlameLayer<S, TraceFile>, FlushGuard<TraceFile>)>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let flame_layer = FlameLayer::new(BufWriter::new(File::create(path)?));
    let flame_guard = flame_layer.flush_on_drop();
    // Idle samples are hidden by default so the SVG focuses on server work.
    // They can be included to show overall utilization.
    let flame_layer = flame_layer
        .with_empty_samples(include_idle)
        .with_threads_collapsed(true)
        .with_module_path(false)
        .with_file_and_line(false);
    Ok((flame_layer, flame_guard))
}

fn observability_config(address: &str) -> ObservabilityConfig {
    match address.parse::<u16>() {
        Ok(port) => ObservabilityConfig::localhost(port),
        Err(_) => ObservabilityConfig::new(address),
    }
}

/// The value of the environment variable `name`, if it is set.
fn env_var(name: &str) -> Option<String> {
    env::var(name).ok()
}

fn is_on(value: Option<String>) -> bool {
    matches!(
        value.as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Example [`ItemBehavior`]: holding a stick and right-clicking a block places
/// glowstone and messages the player, instead of the default (a stick has no
/// default block action).
struct WandBehavior;

impl ItemBehavior for WandBehavior {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        ctx.reply("zap! placed glowstone");
        ctx.place_block(voidmc_data::v26_1_2::blocks::GLOWSTONE);
        UseResult::Handled
    }
}

/// Gives each joining player a few stacks to build with.
fn give_starter_kit(event: On<PlayerReadyEvent>, mut inventories: Query<&mut Inventory>) {
    let Ok(mut inventory) = inventories.get_mut(event.entity) else {
        return;
    };
    for name in [
        "minecraft:stone",
        "minecraft:oak_planks",
        "minecraft:cobblestone",
        "minecraft:glass",
        "minecraft:glowstone",
        "minecraft:stick",
        "minecraft:blaze_rod",
    ] {
        if let Some(stack) = ItemStack::of(name, 64) {
            inventory.give(stack);
        }
    }
}

/// Demo of the packet-send API: tell everyone else who just joined.
fn announce_join(event: On<PlayerReadyEvent>, messages: Messages, names: Query<&PlayerName>) {
    let name = names
        .get(event.entity)
        .map(|n| n.0.as_str())
        .unwrap_or("Someone");
    let joined = event.entity;
    messages
        .broadcast(format!("{name} joined the server"))
        .color(TextColor::Yellow)
        .audience(Audience::custom(move |recipient| {
            recipient.entity() != joined
        }))
        .send();
}

fn on_player_dig(event: On<PlayerStartDiggingEvent>, query: Query<&PlayerName>) {
    let name = query
        .get(event.entity)
        .map(|n| n.0.as_str())
        .unwrap_or("Unknown");
    tracing::info!(
        "{} broke a block at ({}, {}, {})",
        name,
        event.position.x,
        event.position.y,
        event.position.z,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_list_shows_the_welcome_motd_and_room_for_100_players() {
        let config = server_config(
            &MetricsEnv {
                metrics_debug: false,
                flame_enabled: false,
                tps_output: None,
                flame_output: None,
                flame_include_idle: false,
                packet_debug: false,
            },
            |_| None,
        );

        assert_eq!(config.motd, "Welcome to Void Server!");
        assert_eq!(config.max_players, 100);
        assert_eq!(config.address, "127.0.0.1:25565");
    }

    #[test]
    fn observability_address_defaults_to_loopback_for_a_bare_port() {
        assert_eq!(observability_config("9464").address, "127.0.0.1:9464");
        assert_eq!(observability_config("[::1]:9464").address, "[::1]:9464");
    }
}
