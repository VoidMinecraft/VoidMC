#![allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::collapsible_if,
    clippy::map_entry
)]

mod app;
mod client;
pub mod commands;
pub mod components;
pub mod config;
pub mod entity;
pub mod events;
pub mod inventory;
pub mod item;
pub mod item_behavior;
pub mod menu;
pub mod messages;
mod metrics;
pub mod network;
pub mod particles;
pub mod players;
pub mod plugins;
pub mod registry;
pub mod schedule;
mod server;
mod server_status;
pub mod sounds;
pub mod systems;
pub(crate) mod window;
pub mod world;

pub use app::VoidServer;
pub use commands::defaults::{
    PluginList, broadcast_command, clear_command, gamemode_command, give_command, help_command,
    kick_command, list_command, ping_command, plugins_command, register_default_commands,
    say_command, stop_command, summon_command, tell_command, tp_command,
};
pub use commands::parser::{
    BoolArg, DoubleArg, FloatArg, GameProfileArg, GreedyStringArg, IntegerArg, ItemArg, LongArg,
    ResourceLocationArg, StringArg, SummonableEntityArg, Vec3Arg,
};
pub use commands::plugin::CommandSystems;
pub use commands::{
    ArgParser, Command, CommandBuilder, CommandContext, CommandRegistry, ParseError,
};
pub use config::{ServerConfig, ServerConfigBuilder, ServerConfigResource, SpawnPosition};
pub use entity::{
    Billboard, BlockDisplay, CustomName, Display, DisplayTransform, EndCrystal, EntityBuilder,
    EntityHiddenEvent, EntityKind, EntityMetadata, EntityPlugin, EntityShownEvent, Glowing,
    Invisible, ItemDisplay, ItemDisplayContext, MetadataSource, MetadataSourceAppExt, Mount,
    NoGravity, Passengers, Silent, TextAlignment, TextDisplay,
};
pub use inventory::{Cooldown, Inventories, Inventory, WorldInventories};
pub use item::{ItemId, ItemStack};
pub use item_behavior::{
    BlockBreakContext, BlockUseTarget, ItemBehavior, ItemBehaviorRegistry, ItemUseContext,
    UseResult,
};
pub use menu::{
    ContainerIds, ContainerInput, Menu, MenuClickContext, MenuClickEvent, MenuCloseReason,
    MenuClosedEvent, MenuSlot, MenuType, Menus, OpenMenu, WorldMenus,
};
pub use messages::{MessageRequest, Messages, WorldMessages};
pub use particles::{Particle, ParticleColor, ParticleRequest, Particles, WorldParticles};
pub use players::{Audience, Players, Recipient, Recipients, WorldPlayers};
pub use plugins::abilities::PlayerAbilities;
pub use plugins::boss_bar::{BossBar, BossBarColor, BossBarDivision, BossBarFlags};
pub use plugins::teleport::{Teleport, TeleportOutcome};
pub use registry::{RegistryDataStore, default_registry_data};
pub use schedule::VoidSystems;
pub use server::Server;
pub use sounds::{Sound, SoundPosition, SoundSource, SoundStop, Sounds, WorldSounds};
pub use ussr_nbt::owned::Tag;
pub use voidmc_codec::{DecodeLimits, LimitKind};
pub use voidmc_net::socket::FrameLimits;
pub use voidmc_protocol::types::{BlockFace, BlockPosition, Hand};
pub use world::generation::{DefaultWorldGenerator, WorldGen, WorldGenerator};
pub use world::{
    Attribute, Banner, BannerLayer, BannerPattern, BiomeBuilder, BiomeError, BiomeId,
    BlockEntities, BlockEntity, BlockEntityError, BlockEntityKind, ChunkData, ChunkDimension,
    ChunkDirty, ChunkIndex, ChunkLoader, ChunkLoaderResource, ChunkPos, ChunkPosition, DimensionId,
    DyeColor, GrassColorModifier, Sign, SignSide, Skull, biome_at, block_entity_at,
    load_or_generate, remove_block_entity, set_biome, set_block_entity,
};

// Re-export commonly used bevy_ecs types for plugin developers
pub use bevy_ecs::prelude::{On, Query};
