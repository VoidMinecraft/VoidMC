use std::collections::HashSet;
use std::sync::atomic::{AtomicI32, Ordering};

use bevy_ecs::prelude::*;
use uuid::Uuid;

use crate::entity::EntityMetadata;
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

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Rotation {
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct PreviousPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Network entity id. `Default` allocates a fresh one, so every entity that
/// requires this component gets a unique id without touching a resource.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MinecraftEntityId(pub i32);

impl MinecraftEntityId {
    pub fn allocate() -> Self {
        static NEXT: AtomicI32 = AtomicI32::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for MinecraftEntityId {
    fn default() -> Self {
        Self::allocate()
    }
}

#[derive(Component)]
pub struct TeleportState {
    pub next_id: i32,
    pub pending_id: Option<i32>,
}

#[derive(Component, Default, Clone, Copy)]
pub struct KeepAliveState {
    pub last_sent_id: i64,
    pub awaiting_response: bool,
    pub latency: i32,
}

#[derive(Component)]
pub struct ClientSettings {
    pub locale: String,
    pub view_distance: u8,
}

#[derive(Component)]
#[require(crate::entity::visibility_index::IndexedChunks)]
pub struct PlayerReady;

/// The effective (capped) view distance last used for chunk streaming.
#[derive(Component)]
pub struct EffectiveViewDistance(pub i32);

/// Chunks currently loaded (sent) for this player.
#[derive(Component)]
pub struct LoadedChunks(pub HashSet<ChunkPos>);

/// Set by chunk streaming when a pass could not send every chunk in range
/// (send budget or generation cap); the player is revisited next tick even
/// while stationary. Carries the unsent chunks nearest-first so a stationary
/// player drains them without recomputing the range. Cleared once the range
/// is fully streamed; an empty backlog forces a full recompute.
#[derive(Component, Default)]
pub struct ChunkStreamBacklog {
    pub(crate) pending: Vec<ChunkPos>,
}

/// Caps how many chunk packets stream to this player per tick, cached chunks
/// included; at least one is always sent. Absent means unlimited, which is the
/// default for every player.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkSendBudget(pub usize);

/// While present the server owns the player's position: client movement
/// packets update `Rotation` only and never `Position`. Whoever inserts it is
/// responsible for keeping the client in sync (see [`crate::Teleport`]).
#[derive(Component)]
pub struct ServerControlledPosition;

/// The chunk column the player is currently standing in.
#[derive(Component)]
pub struct CurrentChunkPos(pub ChunkPos);

/// Which dimension the player is in.
#[derive(Component)]
pub struct PlayerDimension(pub DimensionId);

/// Marker component for operator (admin) players.
#[derive(Component)]
pub struct Operator;

/// Numeric entity type ID from the `minecraft:entity_type` registry.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntityType(pub i32);

/// Marker for non-player, server-owned entities. Only
/// [`crate::entity::EntityBuilder`] can construct it (private field), and
/// requiring it pulls in every component the replication and simulation
/// systems read, so a half-spawned entity cannot exist. Downstream code uses
/// it as a query filter.
#[derive(Component)]
#[require(
    MinecraftEntityId,
    EntityUuid,
    EntityType,
    Position,
    PreviousPosition,
    Rotation,
    Velocity,
    EntityDimension,
    EntityCollider,
    MovementConfig,
    VerticalVelocity,
    Grounded,
    RecentlySpawned,
    EntityViewers,
    EntityMetadata
)]
pub struct SpawnedEntity(pub(crate) ());

/// Which dimension a non-player entity belongs to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityDimension(pub DimensionId);

impl Default for EntityDimension {
    fn default() -> Self {
        Self(DimensionId::Overworld)
    }
}

/// UUID for a non-player summoned entity, matching the UUID sent in SpawnEntity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityUuid(pub uuid::Uuid);

impl Default for EntityUuid {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Entity velocity in blocks/tick, encoded directly as protocol LP Vec3.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Velocity {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Players currently receiving packets for this entity; maintained by the
/// visibility tracker in `PostUpdate`.
#[derive(Component, Debug, Default)]
pub struct EntityViewers {
    pub(crate) players: HashSet<Entity>,
    pub(crate) chunk: Option<(DimensionId, ChunkPos)>,
}

impl EntityViewers {
    pub fn contains(&self, player: Entity) -> bool {
        self.players.contains(&player)
    }

    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.players.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }
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

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RecentlySpawned(pub u8);
