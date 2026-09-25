use bevy_ecs::prelude::*;
use tracing::instrument;

use crate::components::{
    EntityDimension, Grounded, MovementConfig, Position, PreviousPosition, RecentlySpawned,
    SpawnedEntity, VerticalVelocity,
};
use crate::world::{
    ChunkData, ChunkIndex, ChunkPosition, block_state_at_world, is_solid_block_state,
};

/// Settle newly spawned gravity-enabled entities by scanning downward and snapping them
/// onto the first solid block found within `MAX_SCAN` blocks.
#[instrument(
    name = "entity_spawn_settling",
    level = "info",
    skip(chunk_index, chunks, query)
)]
pub fn settle_recent_spawns(
    chunk_index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    mut query: Query<
        (
            &mut Position,
            &PreviousPosition,
            &MovementConfig,
            &EntityDimension,
            &mut VerticalVelocity,
            &mut Grounded,
            &mut RecentlySpawned,
        ),
        With<SpawnedEntity>,
    >,
) {
    const MAX_SCAN: i32 = 64;

    for (mut pos, prev_pos, movement, dimension, mut velocity, mut grounded, mut marker) in
        query.iter_mut()
    {
        if marker.0 == 0 {
            continue;
        }

        marker.0 -= 1;

        if !movement.gravity_enabled {
            marker.0 = 0;
            continue;
        }

        // Only stationary summons are eligible for the initial ground snap.
        // Thrown item entities have an intentional launch velocity and must
        // follow their normal arc instead of teleporting to the ground.
        if velocity.0.abs() > f64::EPSILON {
            continue;
        }

        let start_y = pos.y.floor() as i32 - 1;
        let min_y = start_y - MAX_SCAN;
        let tx = pos.x.floor() as i32;
        let tz = pos.z.floor() as i32;

        for y in (min_y..=start_y).rev() {
            if let Some(block_state) =
                block_state_at_world(&chunk_index, &chunks, dimension.0, tx, y, tz)
            {
                if is_solid_block_state(block_state) {
                    let ground_y = (y as f64) + 1.0;
                    let fall_distance = prev_pos.y - ground_y;

                    if fall_distance > 0.1 {
                        pos.y = ground_y;
                        velocity.0 = 0.0;
                        grounded.0 = true;
                        marker.0 = 0;
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_app::{App, PostUpdate, Update};
    use bevy_ecs::schedule::IntoScheduleConfigs;
    use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData, blocks};
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, EntityViewers, PlayerReady};
    use crate::entity::{EntityBuilder, EntityKind};
    use crate::network::{NetworkChannels, OutgoingPacket};
    use crate::systems::entities::{broadcast_entity_movement, update_previous_entity_positions};
    use crate::world::{ChunkPos, DimensionId};

    #[test]
    fn ground_snap_reaches_existing_viewers() {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_non_send_resource(incoming_tx)
        .insert_resource(ChunkIndex::default())
        .add_systems(Update, settle_recent_spawns)
        .add_systems(
            PostUpdate,
            (broadcast_entity_movement, update_previous_entity_positions).chain(),
        );

        let viewer = app.world_mut().spawn((ClientId(1), PlayerReady)).id();
        let entity = EntityBuilder::new(EntityKind::Zombie)
            .at(0.5, 70.0, 0.5)
            .gravity(true)
            .spawn_in(app.world_mut())
            .insert(EntityViewers {
                players: HashSet::from([viewer]),
                chunk: None,
            })
            .id();
        app.update();
        assert!(outgoing_rx.try_iter().next().is_none());

        let mut chunk = ChunkData::new(
            (0..24).map(|_| ChunkSection::empty()).collect(),
            ChunkHeightmaps::empty(),
            LightData::empty(),
        );
        chunk.set_block(0, 63, 0, blocks::STONE);
        let chunk_pos = ChunkPos::new(0, 0);
        let chunk_entity = app
            .world_mut()
            .spawn((ChunkPosition(chunk_pos), chunk))
            .id();
        app.world_mut()
            .resource_mut::<ChunkIndex>()
            .0
            .insert((DimensionId::Overworld, chunk_pos), chunk_entity);
        app.update();

        assert_eq!(app.world().get::<Position>(entity).unwrap().y, 64.0);
        let out = outgoing_rx.recv().unwrap();
        assert_eq!(out.client_id, 1);
        let ClientboundPacket::Play(PlayPacket::UpdateEntityPosition(moved)) = out.packet else {
            panic!(
                "expected the snap to reach the viewer, got {:?}",
                out.packet
            );
        };
        assert_eq!(
            (moved.delta_x, moved.delta_y, moved.delta_z),
            (0, -24576, 0)
        );
        assert_eq!(app.world().get::<PreviousPosition>(entity).unwrap().y, 64.0);
    }
}
