use std::collections::HashSet;

use bevy_ecs::prelude::*;
use uuid::Uuid;

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

#[derive(Component, Clone)]
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

/// Numeric entity type ID from the `minecraft:entity_type` registry.
#[derive(Component)]
pub struct EntityType(pub i32);

/// Which dimension a server-owned entity is in.
#[derive(Component, Clone, Copy, Debug)]
pub struct EntityDimension(pub DimensionId);

/// Marker component for non-player summoned entities.
#[derive(Component)]
pub struct SpawnedEntity;

/// UUID for a non-player summoned entity, matching the UUID sent in SpawnEntity.
#[derive(Component)]
pub struct EntityUuid(pub uuid::Uuid);

/// Entity velocity in blocks/tick, matching the SpawnEntity packet LpVec3 format.
#[derive(Component)]
pub struct Velocity {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

/// Movement feature flags for a server-owned entity.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct MovementConfig {
    pub wander: bool,
    pub gravity_enabled: bool,
    pub block_collision_enabled: bool,
}

/// Vertical physics velocity for server-controlled entities, in blocks per tick.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct VerticalVelocity(pub f64);

/// Whether the entity is resting on a solid surface.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Grounded(pub bool);

/// Per-entity movement update cooldown (ticks until next position packet).
#[derive(Component)]
pub struct MovementUpdateCooldown(pub u8);

/// Marker tag for pigs spawned by the /circle command.
#[derive(Component)]
pub struct CirclePig;

/// Orbital state for a circle pig.
#[derive(Component)]
pub struct CirclePigState {
    pub angle: f32,
    /// The player who spawned this circle (used for ownership / cleanup).
    pub owner: Entity,
    /// The entity being orbited (can differ from owner when a player targets another).
    pub target: Entity,
}

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

#[derive(Resource)]
pub struct EntityIdCounter(pub i32);

#[derive(Component)]
pub struct RecentlySpawned(pub u8);
