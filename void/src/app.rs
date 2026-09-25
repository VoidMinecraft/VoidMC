use std::time::Duration;

use bevy_app::{App, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, TerminalCtrlCHandlerPlugin};
use bevy_ecs::prelude::*;

use crate::Server;
use crate::commands::plugin::CommandPlugin;
use crate::commands::{Command, CommandRegistry};
use crate::config::{ServerConfig, ServerConfigResource};
use crate::metrics::MetricsPlugin;
use crate::network::{ConnectionCommand, ConnectionEvent, NetworkPlugin, OutgoingPacket};
use crate::plugins::DefaultPlugins;
use crate::registry::RegistryDataStore;
use crate::server_status::ServerStatusSnapshot;
use crate::systems::GameSystemsPlugin;
use crate::world::{
    ChunkDimension, ChunkIndex, ChunkLoaderResource, ChunkPos, ChunkPosition, DimensionId,
    generation::WorldGen, load_or_generate,
};

/// The main entry point for running a Void server.
pub struct VoidServer {
    config: ServerConfig,
    plugins: Vec<Box<dyn FnOnce(&mut App)>>,
    commands: Vec<Command>,
}

impl VoidServer {
    /// Creates a new server from the given configuration.
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            plugins: Vec::new(),
            commands: Vec::new(),
        }
    }

    /// Registers a custom Bevy plugin or systems hook.
    pub fn add_plugin(mut self, f: impl FnOnce(&mut App) + 'static) -> Self {
        self.plugins.push(Box::new(f));
        self
    }

    /// Registers a command to be added to the CommandRegistry at startup.
    pub fn add_command(mut self, command: Command) -> Self {
        self.commands.push(command);
        self
    }

    /// Starts the server — spawns the network thread and runs the Bevy app.
    /// This function blocks until the server shuts down.
    pub fn run(self) {
        assert!(
            self.config.max_packets_per_tick > 0,
            "max_packets_per_tick must be positive"
        );
        assert!(
            self.config.packet_ingest_budget_ms > 0,
            "packet_ingest_budget_ms must be positive"
        );
        let config_resource = ServerConfigResource::from(&self.config);
        let server_status = ServerStatusSnapshot::new(&self.config);
        let tick_duration = Duration::from_millis(1000 / self.config.tick_rate);
        let address = self.config.address.clone();
        let frame_limits = self.config.frame_limits;

        let world_gen = WorldGen(self.config.world_generator);

        let (events_tx, events_rx) = flume::bounded::<ConnectionEvent>(16 * 1024);
        let (outgoing_tx, _) = flume::bounded::<OutgoingPacket>(1);
        let (control_tx, control_rx) =
            flume::bounded::<ConnectionCommand>(crate::network::MAX_CONNECTIONS + 1);
        let network_control = control_tx.clone();

        // Start the network server in a separate thread
        let network_server_status = server_status.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                let mut server = Server::new_with_limits(&address, frame_limits)
                    .await
                    .expect("Failed to start server");
                server
                    .run_with_status(
                        events_tx,
                        control_rx,
                        network_control,
                        network_server_status,
                    )
                    .await;
            })
        });

        let mut app = App::new();

        app.add_plugins((
            TaskPoolPlugin::default(),
            ScheduleRunnerPlugin::run_loop(tick_duration),
            // Ctrl-C / SIGTERM emits AppExit so the server stops gracefully
            // (the same signal `/stop` uses).
            TerminalCtrlCHandlerPlugin,
        ))
        .add_plugins(NetworkPlugin::new(events_rx, outgoing_tx))
        .add_plugins(DefaultPlugins)
        .add_plugins(CommandPlugin)
        .add_plugins(GameSystemsPlugin);

        if self.config.metrics_debug {
            app.add_plugins(MetricsPlugin::new(self.config.metrics_tps_output.clone()));
        }

        app.insert_resource(self.config.registries)
            .insert_resource(config_resource)
            .insert_resource(server_status)
            .insert_resource(world_gen)
            .init_resource::<ChunkIndex>()
            .add_systems(Startup, init_world);

        // Apply user plugins first (so they can modify the registry)
        for plugin_fn in self.plugins {
            plugin_fn(&mut app);
        }

        // Register commands added via add_command()
        {
            let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
            for command in self.commands {
                registry.register(command);
            }
        }

        app.run();
        let _ = control_tx.send(ConnectionCommand::Shutdown);
    }
}

fn init_world(
    mut commands: Commands,
    mut chunk_index: ResMut<ChunkIndex>,
    world_gen: Res<WorldGen>,
    loader: Option<Res<ChunkLoaderResource>>,
    registries: Res<RegistryDataStore>,
    config: Res<ServerConfigResource>,
) {
    let spawn_chunk = ChunkPos::from_block(config.spawn_x, config.spawn_z);
    let radius = config.spawn_chunk_radius;

    let mut count = 0;
    for pos in spawn_chunk.chunks_in_radius(radius) {
        let mut chunk_data =
            load_or_generate(loader.as_deref(), &world_gen, DimensionId::Overworld, &pos);
        chunk_data.normalize_biomes(registries.biome_count());
        let entity = commands
            .spawn((
                ChunkPosition(pos),
                chunk_data,
                ChunkDimension(DimensionId::Overworld),
            ))
            .id();
        chunk_index.0.insert((DimensionId::Overworld, pos), entity);
        count += 1;
    }

    tracing::info!("Generated {} spawn area chunks", count);
}
