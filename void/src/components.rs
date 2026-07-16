use std::collections::HashSet;

use bevy_ecs::prelude::*;
use uuid::Uuid;

use crate::item::ItemStack;
use crate::world::{ChunkPos, DimensionId};

#[derive(Component)]
pub struct ClientId(pub u32);

#[derive(Component)]
pub struct Client;

#[derive(Component)]
pub struct ConnectionState(pub voidmc_protocol::State);

#[derive(Component)]
pub struct ProtocolVersion(pub i32);

#[derive(Component)]
pub struct PlayerName(pub String);

#[derive(Component)]
pub struct PlayerUuid(pub Uuid);

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Component)]
pub struct Rotation {
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Component)]
pub struct PreviousPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Component)]
pub struct MinecraftEntityId(pub i32);

#[derive(Component)]
pub struct TeleportState {
    pub next_id: i32,
    pub pending_id: Option<i32>,
}

#[derive(Component)]
pub struct KeepAliveState {
    pub last_sent_id: i64,
    pub awaiting_response: bool,
}

#[derive(Component)]
pub struct ClientSettings {
    pub locale: String,
    pub view_distance: u8,
}

#[derive(Component)]
pub struct PlayerReady;

/// The effective (capped) view distance last used for chunk streaming.
#[derive(Component)]
pub struct EffectiveViewDistance(pub i32);

/// Chunks currently loaded (sent) for this player.
#[derive(Component)]
pub struct LoadedChunks(pub HashSet<ChunkPos>);

/// The chunk column the player is currently standing in.
#[derive(Component)]
pub struct CurrentChunkPos(pub ChunkPos);

/// Which dimension the player is in.
#[derive(Component)]
pub struct PlayerDimension(pub DimensionId);

/// Marker component for operator (admin) players.
#[derive(Component)]
pub struct Operator;

/// The currently selected hotbar slot (0..9). Updated whenever the client
/// sends a `SetHeldItem` packet.
#[derive(Component, Clone, Copy)]
pub struct HotbarSlot(pub i16);

/// Per-player container-sync counter. Incremented before each container packet
/// so the vanilla client can reconcile its predicted inventory against the
/// server's authoritative state.
#[derive(Component, Default)]
pub struct ContainerSync {
    pub state_id: i32,
}

impl ContainerSync {
    /// Advances and returns the next state id.
    pub fn advance(&mut self) -> i32 {
        self.state_id = self.state_id.wrapping_add(1);
        self.state_id
    }
}

/// Numeric entity type ID from the `minecraft:entity_type` registry.
#[derive(Component)]
pub struct EntityType(pub i32);

/// Marker component for non-player summoned entities.
#[derive(Component)]
pub struct SpawnedEntity;

/// Which dimension a non-player entity belongs to.
#[derive(Component, Clone, Copy, Debug)]
pub struct EntityDimension(pub DimensionId);

/// UUID for a non-player summoned entity, matching the UUID sent in SpawnEntity.
#[derive(Component)]
pub struct EntityUuid(pub uuid::Uuid);

/// Entity velocity in blocks/tick, encoded directly as protocol LP Vec3.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Velocity {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Movement feature flags for a server-owned entity.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct MovementConfig {
    pub wander: bool,
    pub gravity_enabled: bool,
    pub block_collision_enabled: bool,
}

/// Collision box used by the lightweight server-side entity physics.
///
/// Positions are at the entity's feet, matching the Minecraft protocol. The
/// box is centered on X/Z and extends upward by `height` blocks.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct EntityCollider {
    pub half_width: f64,
    pub height: f64,
    pub step_height: f64,
}

impl EntityCollider {
    pub const fn new(width: f64, height: f64, step_height: f64) -> Self {
        Self {
            half_width: width / 2.0,
            height,
            step_height,
        }
    }

    /// Returns the vanilla-sized collision box for entities whose dimensions
    /// matter to the built-in movement demo, with a safe humanoid fallback.
    pub fn for_entity_name(entity_name: &str) -> Self {
        match entity_name {
            "minecraft:pig" => Self::new(0.9, 0.9, 1.0),
            "minecraft:item" => Self::new(0.25, 0.25, 0.0),
            _ => Self::default(),
        }
    }
}

impl Default for EntityCollider {
    fn default() -> Self {
        Self::new(0.6, 1.8, 1.0)
    }
}

/// Vertical physics velocity for server-controlled entities, in blocks per tick.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct VerticalVelocity(pub f64);

/// Whether the entity is resting on a solid surface.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Grounded(pub bool);

/// Simple wander behavior state for random walking AI example.
#[derive(Component, Clone)]
pub struct Wander {
    /// Ticks remaining until picking a new walk direction.
    pub ticks: i32,
    /// Movement speed in blocks per tick.
    pub speed: f64,
    /// Current yaw direction in degrees (0-360).
    pub yaw: f32,
}

/// A dropped item floating in the world. The entity also carries the standard
/// `SpawnedEntity` / `EntityType(item)` / physics components.
#[derive(Component, Clone)]
pub struct ItemEntity {
    pub stack: ItemStack,
}

/// Ticks remaining before a dropped item can be picked up (prevents instantly
/// re-collecting an item you just threw).
#[derive(Component)]
pub struct PickupDelay(pub u8);

#[derive(Resource)]
pub struct EntityIdCounter(pub i32);

#[derive(Component)]
pub struct RecentlySpawned(pub u8);
