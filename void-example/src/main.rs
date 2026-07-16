use std::env;

use bevy_app::Update;
use bevy_ecs::prelude::Commands;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_flame::FlameLayer;
use tracing_subscriber::prelude::*;
use voidmc::components::PlayerName;
use voidmc::events::{PlayerReadyEvent, PlayerStartDiggingEvent};
use voidmc::item_behavior::{ItemBehavior, ItemBehaviorRegistry, ItemUseContext, UseResult};
use voidmc::plugins::inventory::InventoryDirty;
use voidmc::{
    CommandBuilder, CommandRegistry, Inventory, ItemStack, On, Query, ServerConfigBuilder,
    VoidServer, register_default_commands,
};
use voidmc_world_io::{PersistenceConfig, WorldPersistencePlugin};

mod circle;

struct LogGuards {
    _file: WorkerGuard,
    _flame: Option<tracing_flame::FlushGuard<std::io::BufWriter<std::fs::File>>>,
}

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
        let metrics_debug = env_flag("VOID_METRICS_DEBUG");
        let tps_output = env_string("VOID_TPS_OUTPUT");
        let flame_output = env_string("VOID_FLAME_OUTPUT");
        let flame_include_idle = env_flag("VOID_FLAME_INCLUDE_IDLE");
        let packet_debug = env_flag("VOID_PACKET_DEBUG");
        let metrics_mode = env::var("VOID_METRICS_MODE").ok();
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

    fn enabled(&self) -> bool {
        self.metrics_debug || self.flame_enabled
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metrics_env = MetricsEnv::from_env();
    let _log_guards = setup_logging(&metrics_env)?;

    let mut config_builder = ServerConfigBuilder::new()
        .spawn_chunk_radius(4)
        .initial_chunk_radius(4);

    if metrics_env.enabled() {
        config_builder = config_builder.metrics_debug(true);
    }

    if let Some(tps_output) = metrics_env.tps_output.clone() {
        config_builder = config_builder.metrics_tps_output(tps_output);
    }

    VoidServer::new(config_builder.build())
        .add_plugin(|app| {
            // Persist modified chunks to ./world and reload them on restart.
            app.add_plugins(WorldPersistencePlugin::new(
                PersistenceConfig::new("world").save_interval_ticks(200),
            ));
        })
        .add_plugin(|app| {
            // Register all default commands
            let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
            register_default_commands(&mut registry, &[]);
            registry.register(circle::circle_command());

            // Observe block-breaking events
            app.add_observer(on_player_dig);
            app.add_systems(Update, circle::circle_system);
        })
        .add_plugin(|app| {
            // Demo of the item-behaviour API: a stick becomes a "glowstone wand",
            // overriding the default right-click action.
            app.world_mut()
                .resource_mut::<ItemBehaviorRegistry>()
                .register_for("minecraft:stick", WandBehavior);

            // Give every player a small starter kit when they join.
            app.add_observer(give_starter_kit);
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

    let flame_setup = if metrics_env.flame_enabled {
        let flame_path = metrics_env
            .flame_output
            .clone()
            .unwrap_or_else(|| format!("logs/trace-{timestamp}.folded"));
        // Idle samples are hidden by default so the SVG focuses on server work.
        // They can be included to show overall utilization.
        let (flame_layer, flame_guard) = FlameLayer::with_file(&flame_path)?;
        let flame_layer = flame_layer
            .with_empty_samples(metrics_env.flame_include_idle)
            .with_threads_collapsed(true)
            .with_module_path(false)
            .with_file_and_line(false);
        Some((flame_layer, flame_guard))
    } else {
        None
    };

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

    let flame_guard = if let Some((flame_layer, flame_guard)) = flame_setup {
        registry.with(flame_layer).init();
        Some(flame_guard)
    } else {
        registry.init();
        None
    };

    Ok(LogGuards {
        _file: guard,
        _flame: flame_guard,
    })
}

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

fn env_string(name: &str) -> Option<String> {
    env::var(name)
        .ok()
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

/// Gives each joining player a few stacks to build with, then flags the
/// inventory for re-sync.
fn give_starter_kit(
    event: On<PlayerReadyEvent>,
    mut inventories: Query<&mut Inventory>,
    mut commands: Commands,
) {
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
    ] {
        if let Some(stack) = ItemStack::of(name, 64) {
            inventory.give(stack);
        }
    }
    commands.entity(event.entity).insert(InventoryDirty);
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
