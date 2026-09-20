//! Runs before `ChunkStreaming`: a player receiving the chunk this tick gets
//! its block entities inside the chunk packet, not as a second packet.

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;
use ussr_nbt::owned::{Compound, Nbt};
use voidmc_protocol::clientbound::BlockEntityData;

use crate::players::Players;
use crate::schedule::VoidSystems;
use crate::world::{ChunkData, ChunkDimension, ChunkDirty, ChunkPosition};

pub struct BlockEntityPlugin;

impl Plugin for BlockEntityPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PostUpdate,
            VoidSystems::BlockEntitySync.before(VoidSystems::ChunkStreaming),
        )
        .add_systems(
            PostUpdate,
            sync_block_entities.in_set(VoidSystems::BlockEntitySync),
        );
    }
}

fn sync_block_entities(
    players: Players,
    mut chunks: Query<(Entity, &ChunkPosition, &ChunkDimension, &mut ChunkData)>,
    mut commands: Commands,
) {
    for (entity, position, dimension, mut data) in chunks.iter_mut() {
        if !data.has_pending_block_entities() {
            continue;
        }
        commands.entity(entity).insert(ChunkDirty);
        let recipients = players.ready().seeing_chunk(dimension.0, position.0);
        for (block_position, kind) in data.take_pending_block_entities(position.0) {
            let packet = match data.block_entity(block_position) {
                Some(block_entity) => block_entity.packet(block_position),
                None => BlockEntityData {
                    position: block_position,
                    kind,
                    data: Nbt {
                        name: "".into(),
                        compound: Compound { tags: Vec::new() },
                    },
                },
            };
            recipients.send(packet);
        }
    }
}
