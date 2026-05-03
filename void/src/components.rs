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

#[derive(Resource)]
pub struct EntityIdCounter(pub i32);
