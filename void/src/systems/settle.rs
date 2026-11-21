use bevy_ecs::prelude::*;
use tracing::instrument;

use crate::components::{
    EntityCollider, EntityDimension, Grounded, MovementConfig, Position, PreviousPosition,
    RecentlySpawned, SpawnedEntity, VerticalVelocity,
};
use crate::config::ExecutionMode;
use crate::world::{
    ChunkData, ChunkIndex, ChunkPosition, RegionPos, block_state_at_world, for_each_region,
    is_solid_block_state,
};

/// Keeps a footprint edge that only touches a block face from counting that
/// block, like the physics step's collision checks.
const FOOTPRINT_EPSILON: f64 = 1.0e-7;

/// Settle newly spawned gravity-enabled entities: scan down under the whole
/// collision box and snap them onto the highest solid block top found within
/// `MAX_SCAN` blocks. An entity already standing on that top (within 0.1
/// blocks) is settled where it is.
///
/// Like the physics step, entities are processed per region with
/// [`for_each_region`]: each one only reads chunk data and writes its own
/// components.
#[instrument(
    name = "entity_spawn_settling",
    level = "info",
    skip(mode, chunk_index, chunks, query)
)]
pub fn settle_recent_spawns(
    mode: Option<Res<ExecutionMode>>,
    chunk_index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    mut query: Query<
        (
            &mut Position,
            &PreviousPosition,
            &MovementConfig,
            &EntityDimension,
            Option<&EntityCollider>,
            &mut VerticalVelocity,
            &mut Grounded,
            &mut RecentlySpawned,
        ),
        With<SpawnedEntity>,
    >,
) {
    let mode = mode.as_deref().copied().unwrap_or_default();
    let entities: Vec<_> = query
        .iter_mut()
        .filter(|(.., marker)| marker.0 != 0)
        .collect();
    for_each_region(
        mode,
        entities,
        |(pos, _, _, dimension, ..)| RegionPos::of_block(dimension.0, pos.x, pos.z),
        |_, entities| {
            for (pos, prev_pos, movement, dimension, collider, velocity, grounded, marker) in
                entities
            {
                settle_entity(
                    &chunk_index,
                    &chunks,
                    pos,
                    prev_pos,
                    movement,
                    dimension,
                    collider.copied().unwrap_or_default(),
                    velocity,
                    grounded,
                    marker,
                );
            }
        },
    );
}

fn settle_entity(
    chunk_index: &ChunkIndex,
    chunks: &Query<(&ChunkPosition, &ChunkData)>,
    pos: &mut Mut<Position>,
    prev_pos: &PreviousPosition,
    movement: &MovementConfig,
    dimension: &EntityDimension,
    collider: EntityCollider,
    velocity: &mut Mut<VerticalVelocity>,
    grounded: &mut Mut<Grounded>,
    marker: &mut Mut<RecentlySpawned>,
) {
    const MAX_SCAN: i32 = 64;

    marker.0 -= 1;

    if !movement.gravity_enabled {
        marker.0 = 0;
        return;
    }

    // Only stationary summons are eligible for the initial ground snap.
    // Thrown item entities have an intentional launch velocity and must
    // follow their normal arc instead of teleporting to the ground.
    if velocity.0.abs() > f64::EPSILON {
        return;
    }

    let start_y = pos.y.floor() as i32 - 1;
    let min_y = start_y - MAX_SCAN;
    let min_x = (pos.x - collider.half_width + FOOTPRINT_EPSILON).floor() as i32;
    let max_x = (pos.x + collider.half_width - FOOTPRINT_EPSILON).floor() as i32;
    let min_z = (pos.z - collider.half_width + FOOTPRINT_EPSILON).floor() as i32;
    let max_z = (pos.z + collider.half_width - FOOTPRINT_EPSILON).floor() as i32;
    let solid_under_footprint = |y: i32| {
        (min_x..=max_x).any(|x| {
            (min_z..=max_z).any(|z| {
                block_state_at_world(chunk_index, chunks, dimension.0, x, y, z)
                    .is_some_and(is_solid_block_state)
            })
        })
    };

    // The first solid layer going down holds the highest block top under
    // any part of the box.
    let Some(ground_y) = (min_y..=start_y)
        .rev()
        .find(|&y| solid_under_footprint(y))
        .map(|y| f64::from(y) + 1.0)
    else {
        return;
    };

    if prev_pos.y - ground_y > 0.1 {
        pos.y = ground_y;
        velocity.0 = 0.0;
        grounded.0 = true;
    }
    // Snapped, or already standing on the top: settled either way.
    marker.0 = 0;
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
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::systems::entities::{broadcast_entity_movement, update_previous_entity_positions};
    use crate::world::{ChunkPos, DimensionId};

    #[test]
    fn ground_snap_reaches_existing_viewers() {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx))
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

    /// A summoned `--gravity --block-checks` pig over a chunk holding `blocks`,
    /// run through spawn settling and physics in their `Update` order; returns
    /// its height at the end of each tick, which is what viewers are sent.
    fn simulate_pig(
        blocks: &[(u8, i32, u8)],
        spawn: (f64, f64, f64),
        ticks: usize,
    ) -> (Vec<f64>, bool) {
        let mut app = App::new();
        app.insert_resource(ChunkIndex::default()).add_systems(
            Update,
            (
                settle_recent_spawns,
                crate::systems::physics::apply_spawned_entity_physics,
                update_previous_entity_positions,
            )
                .chain(),
        );
        let mut chunk = ChunkData::new(
            (0..24).map(|_| ChunkSection::empty()).collect(),
            ChunkHeightmaps::empty(),
            LightData::empty(),
        );
        for &(x, y, z) in blocks {
            chunk.set_block(x, y, z, blocks::STONE);
        }
        let chunk_pos = ChunkPos::new(0, 0);
        let chunk_entity = app
            .world_mut()
            .spawn((ChunkPosition(chunk_pos), chunk))
            .id();
        app.world_mut()
            .resource_mut::<ChunkIndex>()
            .0
            .insert((DimensionId::Overworld, chunk_pos), chunk_entity);
        let pig = EntityBuilder::new(EntityKind::Pig)
            .at(spawn.0, spawn.1, spawn.2)
            .gravity(true)
            .block_collision(true)
            .spawn_in(app.world_mut())
            .id();

        let mut heights = Vec::new();
        for _ in 0..ticks {
            app.update();
            heights.push(app.world().get::<Position>(pig).unwrap().y);
        }
        (heights, app.world().get::<Grounded>(pig).unwrap().0)
    }

    #[test]
    fn pig_summoned_on_thick_ground_stays_on_top_of_it() {
        // Three layers of ground under the summon point, which is exactly on
        // the top face or a hair above it.
        let ground: Vec<_> = (61..=63).map(|y| (5, y, 5)).collect();
        for y in [64.0, 64.05] {
            let (heights, grounded) = simulate_pig(&ground, (5.5, y, 5.5), 20);
            assert!(
                heights.iter().all(|&height| height == 64.0),
                "summoned at y = {y}: {heights:?}"
            );
            assert!(grounded);
        }
    }

    #[test]
    fn pig_summoned_beside_a_rise_comes_to_rest_on_it() {
        // The pig's centre is above the floor column, but its 0.9-wide box
        // also reaches over the one-block rise next to it.
        let blocks = [(5, 63, 5), (6, 63, 5), (6, 64, 5)];
        let (heights, grounded) = simulate_pig(&blocks, (5.9, 70.0, 5.5), 20);
        assert!(
            heights.iter().all(|&height| height == 65.0),
            "lands on the rise: {heights:?}"
        );
        assert!(grounded);
    }
}
