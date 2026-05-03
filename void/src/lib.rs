mod app;
mod client;
pub mod commands;
pub mod components;
pub mod config;
pub mod events;
pub mod network;
pub mod plugins;
pub mod registry;
mod server;
pub mod systems;
pub mod world;

pub use app::VoidServer;
pub use commands::defaults::{
    PluginList, broadcast_command, gamemode_command, help_command, kick_command, list_command,
    ping_command, plugins_command, register_default_commands, say_command, tell_command,
    tp_command,
};
pub use commands::parser::{
    BoolArg, DoubleArg, FloatArg, GameProfileArg, GreedyStringArg, IntegerArg, LongArg, StringArg,
};
pub use commands::{
    ArgParser, Command, CommandBuilder, CommandContext, CommandRegistry, ParseError,
};
pub use config::{ServerConfig, ServerConfigBuilder, ServerConfigResource, SpawnPosition};
pub use registry::{RegistryDataStore, default_registry_data};
pub use server::Server;
pub use voidmc_protocol::types::{BlockFace, BlockPosition, Hand};
pub use world::generation::{DefaultWorldGenerator, WorldGen, WorldGenerator};

// Re-export commonly used bevy_ecs types for plugin developers
pub use bevy_ecs::prelude::{On, Query};
