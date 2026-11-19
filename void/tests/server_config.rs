//! `ServerConfig` against its reference page and against `VoidServer::run`:
//! every field is documented with the type and default it has in code, every
//! builder method sets its field, an untouched builder holds the defaults,
//! startup pre-generates and reports the configured spawn area, and an
//! address that cannot be bound stops startup before the game loop.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, Once};
use std::thread;

use bevy_app::{App, AppExit, Update};
use bevy_ecs::prelude::*;
use voidmc::{
    ChunkIndex, ChunkPos, DefaultWorldGenerator, FrameLimits, RegistryDataStore, ServerConfig,
    ServerConfigBuilder, ServerConfigResource, SpawnPosition, VoidServer, WorldGenerator,
    default_registry_data,
};

const REFERENCE: &str = "../void-docs/docs/reference/server/configuration.md";

fn reference() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(REFERENCE);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

type Rows = BTreeMap<String, (String, String)>;

/// `field -> (type, default)` of the "ServerConfig Fields" table. A cell that
/// starts with a code span keeps only that span: "`1` (Creative)" is `1`.
fn documented_fields(reference: &str) -> Rows {
    let section = reference
        .split("## ServerConfig Fields")
        .nth(1)
        .expect("the reference has a ServerConfig Fields section");
    let section = section.split("\n## ").next().unwrap();
    let code = |cell: &str| match cell.strip_prefix('`') {
        Some(rest) => rest.split('`').next().unwrap().to_string(),
        None => cell.to_string(),
    };
    section
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            (code(cells[1]), (code(cells[2]), code(cells[3])))
        })
        .collect()
}

/// Checks the type of a field at compile time.
fn typed<T: ?Sized>(value: &T) -> &T {
    value
}

fn is_default_generator(generator: &dyn WorldGenerator) -> bool {
    let reference = DefaultWorldGenerator::default();
    (-64..64).step_by(7).all(|x| {
        (-64..64)
            .step_by(5)
            .all(|z| generator.surface_height_at(x, z) == reference.surface_height_at(x, z))
    })
}

fn registry_ids(registries: &RegistryDataStore) -> Vec<(String, Vec<String>)> {
    registries
        .registries
        .iter()
        .map(|registry| {
            let entries = registry.entries.iter().map(|e| e.entry_id.clone());
            (registry.registry_id.clone(), entries.collect())
        })
        .collect()
}

fn default_registry_ids() -> Vec<(String, Vec<String>)> {
    let ids = registry_ids(&RegistryDataStore::default());
    assert_eq!(ids.len(), default_registry_data().len());
    ids
}

/// Every field of `config`, written like the reference writes its default.
/// The destructuring names every field, so a new field does not compile
/// until it is added here, and then fails the reference test until it is
/// documented.
fn fields(config: ServerConfig) -> Rows {
    let ServerConfig {
        address,
        tick_rate,
        max_players,
        view_distance,
        simulation_distance,
        game_mode,
        spawn_position,
        spawn_chunk_radius,
        initial_chunk_radius,
        motd,
        hardcore,
        metrics_debug,
        metrics_tps_output,
        max_packets_per_tick,
        packet_ingest_budget_ms,
        max_chunk_generations_per_tick,
        slow_tick_ms,
        frame_limits,
        compression_threshold,
        world_generator,
        registries,
    } = config;
    let SpawnPosition { x, z, y } = typed::<SpawnPosition>(&spawn_position);
    let custom = |is_default: bool, documented: &str| {
        (if is_default { documented } else { "custom" }).to_string()
    };
    [
        (
            "address",
            "String",
            format!("{:?}", typed::<String>(&address)),
        ),
        ("tick_rate", "u64", typed::<u64>(&tick_rate).to_string()),
        ("max_players", "i32", typed::<i32>(&max_players).to_string()),
        (
            "view_distance",
            "i32",
            typed::<i32>(&view_distance).to_string(),
        ),
        (
            "simulation_distance",
            "i32",
            typed::<i32>(&simulation_distance).to_string(),
        ),
        ("game_mode", "u8", typed::<u8>(&game_mode).to_string()),
        (
            "spawn_position",
            "SpawnPosition",
            format!("{{ x: {x:?}, z: {z:?}, y: {y:?} }}"),
        ),
        (
            "spawn_chunk_radius",
            "i32",
            typed::<i32>(&spawn_chunk_radius).to_string(),
        ),
        (
            "initial_chunk_radius",
            "i32",
            typed::<i32>(&initial_chunk_radius).to_string(),
        ),
        ("motd", "String", format!("{:?}", typed::<String>(&motd))),
        ("hardcore", "bool", typed::<bool>(&hardcore).to_string()),
        (
            "metrics_debug",
            "bool",
            typed::<bool>(&metrics_debug).to_string(),
        ),
        (
            "metrics_tps_output",
            "Option<String>",
            format!("{:?}", typed::<Option<String>>(&metrics_tps_output)),
        ),
        (
            "max_packets_per_tick",
            "usize",
            typed::<usize>(&max_packets_per_tick).to_string(),
        ),
        (
            "packet_ingest_budget_ms",
            "u64",
            typed::<u64>(&packet_ingest_budget_ms).to_string(),
        ),
        (
            "max_chunk_generations_per_tick",
            "usize",
            typed::<usize>(&max_chunk_generations_per_tick).to_string(),
        ),
        (
            "slow_tick_ms",
            "u64",
            typed::<u64>(&slow_tick_ms).to_string(),
        ),
        (
            "frame_limits",
            "FrameLimits",
            custom(
                *typed::<FrameLimits>(&frame_limits) == FrameLimits::default(),
                "See below",
            ),
        ),
        (
            "compression_threshold",
            "Option<u32>",
            format!("{:?}", typed::<Option<u32>>(&compression_threshold)),
        ),
        (
            "world_generator",
            "Box<dyn WorldGenerator>",
            custom(
                is_default_generator(&**typed::<Box<dyn WorldGenerator>>(&world_generator)),
                "DefaultWorldGenerator",
            ),
        ),
        (
            "registries",
            "RegistryDataStore",
            custom(
                registry_ids(typed::<RegistryDataStore>(&registries)) == default_registry_ids(),
                "RegistryDataStore::default()",
            ),
        ),
    ]
    .into_iter()
    .map(|(name, ty, default)| (name.to_string(), (ty.to_string(), default)))
    .collect()
}

/// A configuration in which every field differs from its default.
fn customised() -> ServerConfig {
    ServerConfigBuilder::new()
        .address("127.0.0.1:0")
        .tick_rate(40)
        .max_players(3)
        .view_distance(5)
        .simulation_distance(6)
        .game_mode(2)
        .spawn_position(SpawnPosition {
            x: 40.5,
            z: -23.5,
            y: Some(130.0),
        })
        .spawn_chunk_radius(2)
        .initial_chunk_radius(1)
        .motd("Ünïcode ✦ MOTD")
        .hardcore(true)
        .metrics_debug(true)
        .metrics_tps_output("tps.csv")
        .max_packets_per_tick(1)
        .packet_ingest_budget_ms(2)
        .max_chunk_generations_per_tick(2)
        .slow_tick_ms(1)
        .frame_limits(FrameLimits {
            max_inbound_frame_bytes: 4096,
            ..FrameLimits::default()
        })
        .compression_threshold(Some(128))
        .world_generator(DefaultWorldGenerator {
            base_height: 100,
            amplitude: 0.0,
            ..DefaultWorldGenerator::default()
        })
        .configure_registries(|registries| {
            registries.remove_entry("minecraft:painting_variant", "minecraft:alban");
        })
        .build()
}

#[test]
fn the_reference_documents_every_field_with_its_type_and_default() {
    let reference = reference();
    assert_eq!(
        documented_fields(&reference),
        fields(ServerConfig::default())
    );

    let limits = FrameLimits::default();
    assert_eq!(limits.max_inbound_frame_bytes, 2 * 1024 * 1024);
    assert_eq!(limits.max_outbound_frame_bytes, 8 * 1024 * 1024);
    assert!(
        reference.contains(
            "The production defaults allow 2 MiB inbound frames and 8 MiB outbound frames."
        ),
        "the reference states the default frame limits"
    );
}

#[test]
fn every_builder_method_sets_its_field_and_a_new_builder_restores_the_defaults() {
    let defaults = fields(ServerConfig::default());
    let custom = fields(customised());
    for (name, (_, default)) in &defaults {
        assert_ne!(
            &custom[name].1, default,
            "the builder leaves `{name}` at its default"
        );
    }

    assert_eq!(fields(ServerConfigBuilder::new().build()), defaults);
    assert_eq!(fields(ServerConfigBuilder::default().build()), defaults);
}

#[test]
fn the_runtime_resource_mirrors_the_configuration() {
    let config = customised();
    let ServerConfigResource {
        address,
        tick_rate,
        max_players,
        view_distance,
        simulation_distance,
        game_mode,
        spawn_x,
        spawn_z,
        spawn_y,
        spawn_chunk_radius,
        initial_chunk_radius,
        motd,
        hardcore,
        metrics_debug,
        metrics_tps_output,
        max_packets_per_tick,
        packet_ingest_budget_ms,
        max_chunk_generations_per_tick,
        slow_tick_ms,
    } = ServerConfigResource::from(&config);
    assert_eq!(address, config.address);
    assert_eq!(tick_rate, config.tick_rate);
    assert_eq!(max_players, config.max_players);
    assert_eq!(view_distance, config.view_distance);
    assert_eq!(simulation_distance, config.simulation_distance);
    assert_eq!(game_mode, config.game_mode);
    assert_eq!(spawn_x, config.spawn_position.x);
    assert_eq!(spawn_z, config.spawn_position.z);
    assert_eq!(spawn_y, config.spawn_position.y);
    assert_eq!(spawn_chunk_radius, config.spawn_chunk_radius);
    assert_eq!(initial_chunk_radius, config.initial_chunk_radius);
    assert_eq!(motd, config.motd);
    assert_eq!(hardcore, config.hardcore);
    assert_eq!(metrics_debug, config.metrics_debug);
    assert_eq!(metrics_tps_output, config.metrics_tps_output);
    assert_eq!(max_packets_per_tick, config.max_packets_per_tick);
    assert_eq!(packet_ingest_budget_ms, config.packet_ingest_budget_ms);
    assert_eq!(
        max_chunk_generations_per_tick,
        config.max_chunk_generations_per_tick
    );
    assert_eq!(slow_tick_ms, config.slow_tick_ms);
}

static MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Records the message of every info-or-higher event, on every thread.
struct Capture;

impl tracing::Subscriber for Capture {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::INFO
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        MESSAGES.lock().unwrap().push(message.0);
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

fn capture_logs() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        tracing::subscriber::set_global_default(Capture).expect("no other global subscriber");
    });
}

/// Runs `VoidServer::run` with `config` on its own thread. On the first
/// `Update`, `inspect` runs on the world and the server exits. Returns how
/// the thread ended and what `inspect` returned, if it ran.
fn run_until_first_update<R: Send + 'static>(
    config: ServerConfig,
    inspect: impl FnOnce(&mut World) -> R + Send + Sync + 'static,
) -> (thread::Result<()>, Option<R>) {
    let (tx, rx) = flume::bounded(1);
    let inspect = Mutex::new(Some(inspect));
    let server = thread::spawn(move || {
        VoidServer::new(config)
            .add_plugin(move |app: &mut App| {
                app.add_systems(Update, move |world: &mut World| {
                    if let Some(inspect) = inspect.lock().unwrap().take() {
                        let _ = tx.send(inspect(world));
                        world.write_message(AppExit::Success);
                    }
                });
            })
            .run();
    });
    let ended = server.join();
    (ended, rx.try_recv().ok())
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[test]
fn startup_pre_generates_and_reports_the_configured_spawn_area() {
    capture_logs();
    let spawn = ChunkPos::from_block(100.0, -40.0);
    assert_eq!((spawn.x, spawn.z), (6, -3));

    for radius in [0, 2] {
        let before = MESSAGES.lock().unwrap().len();
        let config = ServerConfigBuilder::new()
            .address("127.0.0.1:0")
            .spawn_position(SpawnPosition {
                x: 100.0,
                z: -40.0,
                y: None,
            })
            .spawn_chunk_radius(radius)
            .build();
        let (ended, generated) = run_until_first_update(config, |world| {
            let index = world.resource::<ChunkIndex>();
            index.0.keys().map(|(_, pos)| *pos).collect::<HashSet<_>>()
        });
        ended.expect("the server ran and stopped cleanly");

        let expected: HashSet<ChunkPos> = spawn.chunks_in_radius(radius).into_iter().collect();
        assert_eq!(expected.len() as i32, (2 * radius + 1).pow(2));
        assert_eq!(generated, Some(expected.clone()), "radius {radius}");

        let reported: Vec<String> = MESSAGES.lock().unwrap()[before..]
            .iter()
            .filter(|message| message.ends_with("spawn area chunks"))
            .cloned()
            .collect();
        assert_eq!(
            reported,
            [format!("Generated {} spawn area chunks", expected.len())]
        );
    }
}

#[test]
fn an_address_already_in_use_stops_startup_before_the_game_loop() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = taken.local_addr().unwrap().to_string();

    let config = ServerConfigBuilder::new().address(&address).build();
    let (ended, ticked) = run_until_first_update(config, |_| ());

    let panic = ended.expect_err("binding a port in use fails");
    let message = panic_message(&*panic);
    assert!(
        message.starts_with(&format!("Failed to bind {address}: ")),
        "{message}"
    );
    assert!(ticked.is_none(), "no tick ran");
}

#[test]
fn an_unparsable_address_stops_startup_before_the_game_loop() {
    for address in ["127.0.0.1", "127.0.0.1:99999"] {
        let config = ServerConfigBuilder::new().address(address).build();
        let (ended, ticked) = run_until_first_update(config, |_| ());

        let panic = ended.expect_err("an unparsable address fails");
        let message = panic_message(&*panic);
        assert!(
            message.starts_with(&format!("Failed to bind {address}: ")),
            "{message}"
        );
        assert!(ticked.is_none(), "no tick ran");
    }
}
