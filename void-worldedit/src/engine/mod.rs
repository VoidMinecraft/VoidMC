//! The VoidMC adapter: chunk storage as an [`Extent`](crate::Extent), a
//! tick-budgeted edit queue, per-player sessions with selection outlines,
//! the wand and brush tools, and the `//` commands.
//!
//! ```no_run
//! use voidmc_worldedit::WorldEditPlugin;
//! # fn plugin(app: &mut bevy_app::App) {
//! app.add_plugins(WorldEditPlugin::default().wand("minecraft:wooden_axe"));
//! # }
//! ```

mod args;
mod commands;
mod extent;
mod outline;
mod queue;
mod session;
mod tools;

use std::path::PathBuf;
use std::time::Duration;

use bevy_app::{App, Plugin, PostUpdate, Update};
use bevy_ecs::prelude::*;
use voidmc::components::Operator;
use voidmc::events::PlayerQuitEvent;
use voidmc::{CommandRegistry, ItemBehaviorRegistry, ItemStack, VoidSystems};

pub use extent::{CHUNK_RESEND_SECTIONS, CHUNK_RESEND_THRESHOLD, ChunkExtent};
pub use queue::{Edit, EditQueue};
pub use session::EditSession;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Permission {
    #[default]
    Operators,
    Everyone,
}

#[derive(Resource, Clone, Debug)]
pub struct WorldEditConfig {
    pub wand_item: String,
    pub permission: Permission,
    /// Blocks processed per tick across every queued edit (sections count 4096).
    pub blocks_per_tick: u64,
    /// Wall-clock cap per tick, checked between sections.
    pub time_per_tick: Duration,
    /// Largest region one command may touch.
    pub max_volume: u64,
    pub history_size: usize,
    pub history_bytes: usize,
    pub schematic_dir: PathBuf,
    /// Memory one schematic load may allocate in total; also the largest file,
    /// compressed or decompressed, it reads.
    pub schematic_memory: u64,
    pub show_selection: bool,
    pub brush_preview: bool,
}

impl Default for WorldEditConfig {
    fn default() -> Self {
        Self {
            wand_item: "minecraft:wooden_axe".to_string(),
            permission: Permission::Operators,
            blocks_per_tick: 1_000_000,
            time_per_tick: Duration::from_millis(15),
            max_volume: 32 * 1024 * 1024,
            history_size: 25,
            history_bytes: 128 * 1024 * 1024,
            schematic_dir: PathBuf::from("schematics"),
            schematic_memory: crate::schematic::DEFAULT_MAX_MEMORY,
            show_selection: true,
            brush_preview: true,
        }
    }
}

impl WorldEditConfig {
    pub fn allows(&self, world: &World, player: Entity) -> bool {
        match self.permission {
            Permission::Everyone => true,
            Permission::Operators => world.get::<Operator>(player).is_some(),
        }
    }

    pub fn wand(&self) -> Option<ItemStack> {
        ItemStack::of(&self.wand_item, 1)
    }
}

#[derive(Default)]
pub struct WorldEditPlugin {
    config: WorldEditConfig,
}

impl WorldEditPlugin {
    pub fn new(config: WorldEditConfig) -> Self {
        Self { config }
    }

    pub fn wand(mut self, item: &str) -> Self {
        self.config.wand_item = item.to_string();
        self
    }

    pub fn permission(mut self, permission: Permission) -> Self {
        self.config.permission = permission;
        self
    }

    pub fn blocks_per_tick(mut self, blocks: u64) -> Self {
        self.config.blocks_per_tick = blocks;
        self
    }

    pub fn time_per_tick(mut self, limit: Duration) -> Self {
        self.config.time_per_tick = limit;
        self
    }

    pub fn max_volume(mut self, blocks: u64) -> Self {
        self.config.max_volume = blocks;
        self
    }

    pub fn schematic_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.schematic_dir = dir.into();
        self
    }

    pub fn schematic_memory(mut self, bytes: u64) -> Self {
        self.config.schematic_memory = bytes;
        self
    }

    pub fn history(mut self, entries: usize, bytes: usize) -> Self {
        self.config.history_size = entries;
        self.config.history_bytes = bytes;
        self
    }

    pub fn show_selection(mut self, show: bool) -> Self {
        self.config.show_selection = show;
        self
    }

    pub fn brush_preview(mut self, show: bool) -> Self {
        self.config.brush_preview = show;
        self
    }
}

impl Plugin for WorldEditPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
            .init_resource::<EditQueue>()
            .init_resource::<CommandRegistry>()
            .init_resource::<ItemBehaviorRegistry>()
            .init_resource::<tools::BrushItems>()
            .add_observer(forget_player)
            .add_systems(
                Update,
                (
                    queue::run_edit_queue
                        .after(VoidSystems::CommandDrain)
                        .after(VoidSystems::ItemUseDrain),
                    tools::brush_preview,
                ),
            )
            .add_systems(PostUpdate, outline::sync_outlines);

        match ItemStack::of(&self.config.wand_item, 1) {
            Some(wand) => app
                .world_mut()
                .resource_mut::<ItemBehaviorRegistry>()
                .register(wand.item, tools::Wand),
            None => tracing::warn!(item = %self.config.wand_item, "WorldEdit wand item is unknown"),
        }

        let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
        for command in commands::commands() {
            registry.register(command);
        }
    }
}

fn forget_player(event: On<PlayerQuitEvent>, mut queue: ResMut<EditQueue>) {
    queue.detach(event.entity);
}
