use bevy_ecs::prelude::*;
use voidmc_protocol::types::{BlockFace, BlockPosition, Hand};

use crate::world::DimensionId;

// Semantic game events — triggered via world.trigger() and handled by observers
#[derive(Event)]
pub struct PlayerJoinEvent {
    pub client_id: u32,
    pub entity: Entity,
}

#[derive(Event)]
pub struct PlayerQuitEvent {
    pub client_id: u32,
    pub entity: Entity,
}

#[derive(Event)]
pub struct PlayerMoveEvent {
    pub entity: Entity,
    pub old_x: f64,
    pub old_y: f64,
    pub old_z: f64,
    pub new_x: f64,
    pub new_y: f64,
    pub new_z: f64,
}

#[derive(Event)]
pub struct PlayerRotateEvent {
    pub entity: Entity,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Event)]
pub struct PlayerReadyEvent {
    pub client_id: u32,
    pub entity: Entity,
}

#[derive(Event)]
pub struct ChatCommandEvent {
    pub entity: Entity,
    pub client_id: u32,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Event)]
pub struct ChatMessageEvent {
    pub entity: Entity,
    pub client_id: u32,
    pub message: String,
}

#[derive(Event)]
pub struct PlayerSneakEvent {
    pub entity: Entity,
    pub sneaking: bool,
}

#[derive(Event)]
pub struct PlayerSprintEvent {
    pub entity: Entity,
    pub sprinting: bool,
}

#[derive(Event)]
pub struct PlayerInteractEntityEvent {
    pub entity: Entity,
    pub target_id: i32,
    pub attack: bool,
    pub hand: Option<Hand>,
    pub target_pos: Option<(f32, f32, f32)>,
    pub sneaking: bool,
}

#[derive(Event)]
pub struct PlayerStartDiggingEvent {
    pub entity: Entity,
    pub position: BlockPosition,
    pub face: BlockFace,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerCancelDiggingEvent {
    pub entity: Entity,
    pub position: BlockPosition,
    pub face: BlockFace,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerFinishDiggingEvent {
    pub entity: Entity,
    pub position: BlockPosition,
    pub face: BlockFace,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerDropItemEvent {
    pub entity: Entity,
    pub drop_stack: bool,
}

#[derive(Event)]
pub struct PlayerSwapHandsEvent {
    pub entity: Entity,
}

#[derive(Event)]
pub struct PlayerUseItemOnBlockEvent {
    pub entity: Entity,
    pub hand: Hand,
    pub position: BlockPosition,
    pub face: BlockFace,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub cursor_z: f32,
    pub inside_block: bool,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerUseItemEvent {
    pub entity: Entity,
    pub hand: Hand,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerSwingArmEvent {
    pub entity: Entity,
    pub hand: Hand,
}

#[derive(Event)]
pub struct PlayerChangeSlotEvent {
    pub entity: Entity,
    pub slot: i16,
}

#[derive(Event)]
pub struct PlayerToggleFlyEvent {
    pub entity: Entity,
    pub flying: bool,
}

#[derive(Event)]
pub struct PlayerCloseContainerEvent {
    pub entity: Entity,
    pub window_id: u8,
}

/// Fired after a block has been mutated in the world. Carries enough context
/// for downstream systems (broadcasting, persistence, gameplay reactions) to
/// react without re-reading chunk data.
#[derive(Event)]
pub struct BlockChangeEvent {
    /// The dimension the change occurred in.
    pub dimension: DimensionId,
    /// World-space block position of the change.
    pub position: BlockPosition,
    /// Block-state id before the change.
    pub old_state: i32,
    /// Block-state id after the change.
    pub new_state: i32,
    /// The player entity that caused the change, when applicable.
    pub source: Option<Entity>,
}

/// Fired when a player finishes breaking a block and the framework has
/// committed the change. `BlockChangeEvent` is also fired for the same
/// mutation; this event is the higher-level semantic signal.
#[derive(Event)]
pub struct BlockBreakEvent {
    pub entity: Entity,
    pub dimension: DimensionId,
    pub position: BlockPosition,
    pub broken_state: i32,
}

/// Fired when a player places a block via `UseItemOn` and the framework has
/// committed the change.
#[derive(Event)]
pub struct BlockPlaceEvent {
    pub entity: Entity,
    pub dimension: DimensionId,
    pub position: BlockPosition,
    pub face: BlockFace,
    pub placed_state: i32,
}
