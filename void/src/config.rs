use bevy_ecs::prelude::*;

use crate::registry::RegistryDataStore;
use crate::world::generation::{DefaultWorldGenerator, WorldGenerator};

/// Spawn position in the world.
pub struct SpawnPosition {
    pub x: f64,
    pub z: f64,
    /// If `None`, Y is auto-computed from the world generator.
    pub y: Option<f64>,
}

impl Default for SpawnPosition {
    fn default() -> Self {
        Self {
            x: 0.0,
            z: 0.0,
            y: None,
        }
    }
}

/// Full server configuration. Consumed once by `VoidServer::new`.
pub struct ServerConfig {
    pub address: String,
    pub tick_rate: u64,
    pub max_players: i32,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub game_mode: u8,
    pub spawn_position: SpawnPosition,
    pub spawn_chunk_radius: i32,
    pub initial_chunk_radius: i32,
    pub motd: String,
    pub hardcore: bool,
    pub world_generator: Box<dyn WorldGenerator>,
    pub registries: RegistryDataStore,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:25565".to_string(),
            tick_rate: 20,
            max_players: 100,
            view_distance: 10,
            simulation_distance: 10,
            game_mode: 1,
            spawn_position: SpawnPosition::default(),
            spawn_chunk_radius: 10,
            initial_chunk_radius: 3,
            motd: "Welcome to Void Server!".to_string(),
            hardcore: false,
            world_generator: Box::new(DefaultWorldGenerator::default()),
            registries: RegistryDataStore::default(),
        }
    }
}

/// Builder for ergonomic `ServerConfig` construction.
pub struct ServerConfigBuilder {
    config: ServerConfig,
}

impl ServerConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: ServerConfig::default(),
        }
    }

    pub fn address(mut self, address: impl Into<String>) -> Self {
        self.config.address = address.into();
        self
    }

    pub fn tick_rate(mut self, tick_rate: u64) -> Self {
        self.config.tick_rate = tick_rate;
        self
    }

    pub fn max_players(mut self, max_players: i32) -> Self {
        self.config.max_players = max_players;
        self
    }

    pub fn view_distance(mut self, view_distance: i32) -> Self {
        self.config.view_distance = view_distance;
        self
    }

    pub fn simulation_distance(mut self, simulation_distance: i32) -> Self {
        self.config.simulation_distance = simulation_distance;
        self
    }

    pub fn game_mode(mut self, game_mode: u8) -> Self {
        self.config.game_mode = game_mode;
        self
    }

    pub fn spawn_position(mut self, spawn: SpawnPosition) -> Self {
        self.config.spawn_position = spawn;
        self
    }

    pub fn spawn_chunk_radius(mut self, radius: i32) -> Self {
        self.config.spawn_chunk_radius = radius;
        self
    }

    pub fn initial_chunk_radius(mut self, radius: i32) -> Self {
        self.config.initial_chunk_radius = radius;
        self
    }

    pub fn motd(mut self, motd: impl Into<String>) -> Self {
        self.config.motd = motd.into();
        self
    }

    pub fn hardcore(mut self, hardcore: bool) -> Self {
        self.config.hardcore = hardcore;
        self
    }

    pub fn world_generator(mut self, generator: impl WorldGenerator + 'static) -> Self {
        self.config.world_generator = Box::new(generator);
        self
    }

    pub fn configure_registries(mut self, f: impl FnOnce(&mut RegistryDataStore)) -> Self {
        f(&mut self.config.registries);
        self
    }

    pub fn build(self) -> ServerConfig {
        self.config
    }
}

impl Default for ServerConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Runtime-readable config resource (plain data, no `Box<dyn>`).
#[derive(Resource, Clone)]
pub struct ServerConfigResource {
    pub address: String,
    pub tick_rate: u64,
    pub max_players: i32,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub game_mode: u8,
    pub spawn_x: f64,
    pub spawn_z: f64,
    pub spawn_y: Option<f64>,
    pub spawn_chunk_radius: i32,
    pub initial_chunk_radius: i32,
    pub motd: String,
    pub hardcore: bool,
}

impl From<&ServerConfig> for ServerConfigResource {
    fn from(config: &ServerConfig) -> Self {
        Self {
            address: config.address.clone(),
            tick_rate: config.tick_rate,
            max_players: config.max_players,
            view_distance: config.view_distance,
            simulation_distance: config.simulation_distance,
            game_mode: config.game_mode,
            spawn_x: config.spawn_position.x,
            spawn_z: config.spawn_position.z,
            spawn_y: config.spawn_position.y,
            spawn_chunk_radius: config.spawn_chunk_radius,
            initial_chunk_radius: config.initial_chunk_radius,
            motd: config.motd.clone(),
            hardcore: config.hardcore,
        }
    }
}
