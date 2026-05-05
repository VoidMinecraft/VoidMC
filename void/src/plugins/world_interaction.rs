//! World-interaction plugin.
//!
//! Translates the player-action events emitted by `InteractionPlugin` into
//! authoritative world mutations: breaking, placing, and acknowledging block
//! changes. The plugin is responsible for the round-trip the vanilla client
//! expects (ack the prediction sequence, broadcast `BlockUpdate` to nearby
//! observers) and emits semantic `BlockBreakEvent` / `BlockPlaceEvent` /
//! `BlockChangeEvent` for downstream developer code.

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::Resource;
use bevy_ecs::{
    entity::Entity,
    observer::On,
    query::With,
    system::{Commands, Query, Res},
};
use voidmc_protocol::{
    clientbound,
    types::{BlockFace, BlockPosition},
};

use crate::{
    components::{ClientId, HotbarSlot, LoadedChunks, PlayerDimension, PlayerReady},
    events::{
        BlockBreakEvent, BlockChangeEvent, BlockPlaceEvent, PlayerChangeSlotEvent,
        PlayerFinishDiggingEvent, PlayerUseItemOnBlockEvent,
    },
    network::{NetworkChannels, OutgoingPacket},
    world::{ChunkData, ChunkDirty, ChunkIndex, ChunkPos, DimensionId},
};

/// The block-state id placed when a player triggers `UseItemOn` and the
/// hotbar/`HotbarBlocks` resource has no entry for the selected slot. Defaults
/// to `minecraft:stone` (1).
#[derive(Resource, Clone, Copy)]
pub struct DefaultPlaceBlock(pub i32);

impl Default for DefaultPlaceBlock {
    fn default() -> Self {
        Self(1)
    }
}

/// Block-state id assigned to each of the player's nine hotbar slots.
///
/// Acts as a stand-in for a real inventory: when the player triggers
/// `UseItemOn`, the world-interaction plugin places `slots[hotbar_slot]`. A
/// `0` entry falls back to [`DefaultPlaceBlock`].
///
/// The default cycles through the four block-state ids exposed by
/// [`voidmc_protocol::clientbound::chunk::blocks`] so the example server
/// shows visual variety across hotbar keys 1–9 without any developer setup.
/// Replace this resource to expose richer palettes — every entry is a raw
/// block-state id from the global block-state palette of the targeted
/// protocol version.
#[derive(Resource, Clone, Copy)]
pub struct HotbarBlocks(pub [i32; 9]);

impl Default for HotbarBlocks {
    fn default() -> Self {
        use voidmc_data::v26_1_2::blocks::{
            BRICKS, COBBLESTONE, DIAMOND_BLOCK, GLASS, GOLD_BLOCK, OAK_LOG, OAK_PLANKS,
            REDSTONE_LAMP, STONE,
        };
        Self([
            STONE,
            OAK_PLANKS,
            COBBLESTONE,
            GLASS,
            OAK_LOG,
            BRICKS,
            DIAMOND_BLOCK,
            GOLD_BLOCK,
            REDSTONE_LAMP,
        ])
    }
}

pub struct WorldInteractionPlugin;

impl Plugin for WorldInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DefaultPlaceBlock>()
            .init_resource::<HotbarBlocks>()
            .add_observer(handle_finish_digging)
            .add_observer(handle_use_item_on)
            .add_observer(track_hotbar_slot);
    }
}

fn track_hotbar_slot(event: On<PlayerChangeSlotEvent>, mut slots: Query<&mut HotbarSlot>) {
    if let Ok(mut slot) = slots.get_mut(event.entity) {
        slot.0 = event.slot;
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_finish_digging(
    event: On<PlayerFinishDiggingEvent>,
    channels: Res<NetworkChannels>,
    chunk_index: Res<ChunkIndex>,
    mut chunks: Query<&mut ChunkData>,
    player_dim: Query<&PlayerDimension>,
    client_ids: Query<&ClientId>,
    observers: Query<(&ClientId, &PlayerDimension, &LoadedChunks), With<PlayerReady>>,
    mut commands: Commands,
) {
    let Ok(dim) = player_dim.get(event.entity) else {
        return;
    };
    mutate_block(
        event.entity,
        dim.0,
        event.position,
        0,
        event.sequence,
        &channels,
        &chunk_index,
        &mut chunks,
        &client_ids,
        &observers,
        &mut commands,
        BlockMutation::Break,
        BlockFace::Top,
    );
}

#[allow(clippy::too_many_arguments)]
fn handle_use_item_on(
    event: On<PlayerUseItemOnBlockEvent>,
    channels: Res<NetworkChannels>,
    chunk_index: Res<ChunkIndex>,
    mut chunks: Query<&mut ChunkData>,
    actor_state: Query<(&PlayerDimension, &HotbarSlot)>,
    client_ids: Query<&ClientId>,
    observers: Query<(&ClientId, &PlayerDimension, &LoadedChunks), With<PlayerReady>>,
    place_block: Res<DefaultPlaceBlock>,
    hotbar: Res<HotbarBlocks>,
    mut commands: Commands,
) {
    let Ok((dim, slot)) = actor_state.get(event.entity) else {
        return;
    };

    let block_id = hotbar
        .0
        .get(slot.0.clamp(0, 8) as usize)
        .copied()
        .filter(|&id| id != 0)
        .unwrap_or(place_block.0);

    let target = offset_position(event.position, event.face);
    mutate_block(
        event.entity,
        dim.0,
        target,
        block_id,
        event.sequence,
        &channels,
        &chunk_index,
        &mut chunks,
        &client_ids,
        &observers,
        &mut commands,
        BlockMutation::Place,
        event.face,
    );
}

#[derive(Clone, Copy)]
enum BlockMutation {
    Break,
    Place,
}

#[allow(clippy::too_many_arguments)]
fn mutate_block(
    actor: Entity,
    dimension: DimensionId,
    position: BlockPosition,
    new_state: i32,
    sequence: i32,
    channels: &NetworkChannels,
    chunk_index: &ChunkIndex,
    chunks: &mut Query<&mut ChunkData>,
    client_ids: &Query<&ClientId>,
    observers: &Query<(&ClientId, &PlayerDimension, &LoadedChunks), With<PlayerReady>>,
    commands: &mut Commands,
    mutation: BlockMutation,
    face: BlockFace,
) {
    let chunk_pos = ChunkPos::new(position.x >> 4, position.z >> 4);
    let Some(&chunk_entity) = chunk_index.0.get(&(dimension, chunk_pos)) else {
        send_ack(actor, sequence, channels, client_ids);
        return;
    };

    let Ok(mut chunk_data) = chunks.get_mut(chunk_entity) else {
        send_ack(actor, sequence, channels, client_ids);
        return;
    };

    let local_x = position.x.rem_euclid(16) as u8;
    let local_z = position.z.rem_euclid(16) as u8;
    let world_y = position.y as i32;

    let old_state = match chunk_data.set_block(local_x, world_y, local_z, new_state) {
        Some(prev) => prev,
        None => {
            send_ack(actor, sequence, channels, client_ids);
            return;
        }
    };

    if old_state == new_state {
        send_ack(actor, sequence, channels, client_ids);
        return;
    }

    commands.entity(chunk_entity).insert(ChunkDirty);

    let update_packet = clientbound::ClientboundPacket::Play(clientbound::PlayPacket::BlockUpdate(
        clientbound::BlockUpdate {
            position,
            block_state_id: new_state,
        },
    ));

    for (client_id, dim, loaded) in observers.iter() {
        if dim.0 != dimension || !loaded.0.contains(&chunk_pos) {
            continue;
        }
        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: client_id.0,
            packet: update_packet.clone(),
        });
    }

    send_ack(actor, sequence, channels, client_ids);

    commands.trigger(BlockChangeEvent {
        dimension,
        position,
        old_state,
        new_state,
        source: Some(actor),
    });

    match mutation {
        BlockMutation::Break => {
            commands.trigger(BlockBreakEvent {
                entity: actor,
                dimension,
                position,
                broken_state: old_state,
            });
        }
        BlockMutation::Place => {
            commands.trigger(BlockPlaceEvent {
                entity: actor,
                dimension,
                position,
                face,
                placed_state: new_state,
            });
        }
    }
}

fn send_ack(
    actor: Entity,
    sequence: i32,
    channels: &NetworkChannels,
    client_ids: &Query<&ClientId>,
) {
    if sequence == 0 {
        return;
    }
    let Ok(client_id) = client_ids.get(actor) else {
        return;
    };
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: client_id.0,
        packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::BlockChangedAck(
            clientbound::BlockChangedAck { sequence },
        )),
    });
}

fn offset_position(pos: BlockPosition, face: BlockFace) -> BlockPosition {
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
