use std::time::Duration;

use bevy_app::{App, ScheduleRunnerPlugin, Startup, TaskPoolPlugin};
use bevy_ecs::prelude::*;

use crate::Server;
use crate::commands::plugin::CommandPlugin;
use crate::commands::{Command, CommandRegistry};
use crate::components::EntityIdCounter;
use crate::config::{ServerConfig, ServerConfigResource};
use crate::handlers::DefaultHandlersPlugin;
use crate::network::{IncomingPacket, NetworkPlugin, OutgoingPacket};
use crate::systems::GameSystemsPlugin;
use crate::world::{
    ChunkData, ChunkDimension, ChunkIndex, ChunkPos, ChunkPosition, DimensionId,
    generation::WorldGen,
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
        let config_resource = ServerConfigResource::from(&self.config);
        let tick_duration = Duration::from_millis(1000 / self.config.tick_rate);
        let address = self.config.address.clone();

        let world_gen = WorldGen(self.config.world_generator);

        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();

        // Start the network server in a separate thread
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                let mut server = Server::new(&address).await.expect("Failed to start server");
                server
                    .run(incoming_tx, outgoing_rx, disconnect_tx, kick_rx)
                    .await;
            })
        });

        let mut app = App::new();

        app.add_plugins((
            TaskPoolPlugin::default(),
            ScheduleRunnerPlugin::run_loop(tick_duration),
        ))
        .add_plugins(NetworkPlugin::new(
            incoming_rx,
            outgoing_tx,
            disconnect_rx,
            kick_tx,
        ))
        .add_plugins(DefaultHandlersPlugin)
        .add_plugins(CommandPlugin)
        .add_plugins(GameSystemsPlugin)
        .insert_resource(EntityIdCounter(1))
        .insert_resource(self.config.registries)
        .insert_resource(config_resource)
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
    }
}

fn init_world(
    mut commands: Commands,
    mut chunk_index: ResMut<ChunkIndex>,
    world_gen: Res<WorldGen>,
    config: Res<ServerConfigResource>,
) {
    let spawn_chunk = ChunkPos::from_block(config.spawn_x, config.spawn_z);
    let radius = config.spawn_chunk_radius;

    let mut count = 0;
    for pos in spawn_chunk.chunks_in_radius(radius) {
        let chunk = world_gen.0.generate_chunk(&pos);
        let entity = commands
            .spawn((
                ChunkPosition(pos),
                ChunkData::from_protocol_chunk(&chunk),
                ChunkDimension(DimensionId::Overworld),
            ))
            .id();
        chunk_index.0.insert((DimensionId::Overworld, pos), entity);
        count += 1;
    }

    tracing::info!("Generated {} spawn area chunks", count);
}
