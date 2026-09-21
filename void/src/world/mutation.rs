//! World-based block mutation shared by the break and item-use paths.
//!
//! Unlike the observer handlers (which take Bevy system params), this operates
//! directly on `&mut World` so it can be called from the exclusive item-use
//! drain system, where item behaviours run with full world access.

use bevy_ecs::prelude::*;
use voidmc_protocol::{
    clientbound,
    types::{BlockFace, BlockPosition},
};

use crate::components::{ClientId, PlayerName};
use crate::events::{BlockBreakEvent, BlockChangeEvent, BlockPlaceEvent};
use crate::players::WorldPlayers;
use crate::world::{ChunkData, ChunkDirty, ChunkIndex, ChunkPos, DimensionId};

/// Whether a mutation breaks or places a block (selects the semantic event).
#[derive(Clone, Copy, Debug)]
pub enum BlockMutation {
    Break,
    Place,
}

/// Applies a block change to the world: updates chunk data, marks the chunk
/// dirty, broadcasts `BlockUpdate` to observers, and fires `BlockChangeEvent`
/// plus `BlockBreakEvent`/`BlockPlaceEvent`.
///
/// Returns the previous block-state id if the block actually changed, else
/// `None`. Does **not** acknowledge the client prediction sequence — the caller
/// owns that (so an action acks exactly once even when it mutates several blocks
/// or none); use [`send_ack`].
pub fn mutate_block(
    world: &mut World,
    actor: Entity,
    dimension: DimensionId,
    position: BlockPosition,
    new_state: i32,
    face: BlockFace,
    mutation: BlockMutation,
) -> Option<i32> {
    let chunk_pos = ChunkPos::new(position.x >> 4, position.z >> 4);

    let chunk_entity = world
        .resource::<ChunkIndex>()
        .0
        .get(&(dimension, chunk_pos))
        .copied()?;

    let local_x = position.x.rem_euclid(16) as u8;
    let local_z = position.z.rem_euclid(16) as u8;
    let world_y = position.y as i32;

    let old_state = world
        .get_mut::<ChunkData>(chunk_entity)?
        .set_block(local_x, world_y, local_z, new_state)?;

    if old_state == new_state {
        return None;
    }

    world.entity_mut(chunk_entity).insert(ChunkDirty);

    // Broadcast the change to every ready player observing the chunk.
    WorldPlayers::new(world).broadcast_chunk(
        dimension,
        chunk_pos,
        clientbound::BlockUpdate {
            position,
            block_state_id: new_state,
        },
    );

    world.trigger(BlockChangeEvent {
        dimension,
        position,
        old_state,
        new_state,
        source: Some(actor),
    });
    match mutation {
        BlockMutation::Break => world.trigger(BlockBreakEvent {
            entity: actor,
            dimension,
            position,
            broken_state: old_state,
        }),
        BlockMutation::Place => {
            let player_name = world
                .get::<PlayerName>(actor)
                .map(|name| name.0.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            let client_id = world.get::<ClientId>(actor).map(|id| id.0);
            tracing::info!(
                player_name = %player_name,
                client_id,
                dimension = dimension.name(),
                x = position.x,
                y = position.y,
                z = position.z,
                block_state_id = new_state,
                "Block placed"
            );
            world.trigger(BlockPlaceEvent {
                entity: actor,
                dimension,
                position,
                face,
                placed_state: new_state,
            });
        }
    }

    Some(old_state)
}

/// Sends a `BlockChangedAck` for the given prediction sequence to `actor`.
pub fn send_ack(world: &World, actor: Entity, sequence: i32) {
    WorldPlayers::new(world).send(actor, clientbound::BlockChangedAck { sequence });
}

/// The block position adjacent to `pos` across `face` (where a placed block lands).
pub fn offset_position(pos: BlockPosition, face: BlockFace) -> BlockPosition {
    let (dx, dy, dz) = match face {
        BlockFace::Bottom => (0, -1, 0),
        BlockFace::Top => (0, 1, 0),
        BlockFace::North => (0, 0, -1),
        BlockFace::South => (0, 0, 1),
        BlockFace::West => (-1, 0, 0),
        BlockFace::East => (1, 0, 0),
    };
    BlockPosition {
        x: pos.x + dx,
        y: pos.y + dy as i16,
        z: pos.z + dz,
    }
}
