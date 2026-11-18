use bevy_ecs::prelude::*;
use voidmc::components::{MinecraftEntityId, PlayerDimension};
use voidmc::{DimensionId, Players};
use voidmc_protocol::clientbound::entity_metadata::{display_index, entity_flag, entity_index};
use voidmc_protocol::clientbound::{
    DisplayTransform, EntityMetadataEntry, EntityMetadataValue as Value, RemoveEntities,
    SetEntityData, SpawnEntity, TeleportEntity, TeleportFlags, pack_brightness,
};
use voidmc_protocol::types::LpVec3;

use super::WorldEditConfig;
use super::session::EditSession;
use crate::block::{BlockState, VERSION};
use crate::math::BlockPos;
use crate::region::Cuboid;

const EDGE: f32 = 1.0 / 16.0;
const MARKER: f32 = 1.02;
const INTERPOLATION_TICKS: u16 = 3;
const VIEW_RANGE: f32 = 8.0;
const PARTS: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Part {
    block: BlockState,
    glow: i32,
    position: [f64; 3],
    scale: [f32; 3],
}

#[derive(Clone, Copy, Debug)]
struct Shown {
    id: i32,
    part: Option<Part>,
}

/// The display entities drawing one player's selection, sent to that player
/// only. They exist purely client-side: the server never tracks them.
#[derive(Component, Debug)]
pub(crate) struct Outline {
    revision: Option<u64>,
    dimension: Option<DimensionId>,
    parts: [Shown; PARTS],
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            revision: None,
            dimension: None,
            parts: std::array::from_fn(|_| Shown {
                id: MinecraftEntityId::allocate().0,
                part: None,
            }),
        }
    }
}

fn state(name: &str) -> BlockState {
    BlockState::parse(name).unwrap_or(BlockState::AIR)
}

fn marker(pos: BlockPos, block: BlockState, glow: i32) -> Part {
    let inset = f64::from(MARKER - 1.0) / 2.0;
    Part {
        block,
        glow,
        position: [
            f64::from(pos.x) - inset,
            f64::from(pos.y) - inset,
            f64::from(pos.z) - inset,
        ],
        scale: [MARKER; 3],
    }
}

fn edges(bounds: Cuboid, block: BlockState, glow: i32) -> impl Iterator<Item = Part> {
    let lo = [bounds.min.x, bounds.min.y, bounds.min.z].map(f64::from);
    let hi = [bounds.max.x + 1, bounds.max.y + 1, bounds.max.z + 1].map(f64::from);
    let half = f64::from(EDGE) / 2.0;
    (0..3).flat_map(move |axis| {
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        [lo[a], hi[a]].into_iter().flat_map(move |u| {
            [lo[b], hi[b]].into_iter().map(move |v| {
                let mut position = [0.0; 3];
                let mut scale = [EDGE; 3];
                position[axis] = lo[axis];
                position[a] = u - half;
                position[b] = v - half;
                scale[axis] = (hi[axis] - lo[axis]) as f32;
                Part {
                    block,
                    glow,
                    position,
                    scale,
                }
            })
        })
    })
}

fn desired(session: &EditSession) -> [Option<Part>; PARTS] {
    let selection = &session.selection;
    let mut parts = [None; PARTS];
    if let Some(pos) = selection.pos1() {
        parts[0] = Some(marker(pos, state("red_stained_glass"), 0xFF_5555));
    }
    if let Some(pos) = selection.pos2() {
        parts[1] = Some(marker(pos, state("light_blue_stained_glass"), 0x55_55FF));
    }
    if let Ok(bounds) = selection.bounds() {
        for (slot, edge) in
            parts[2..]
                .iter_mut()
                .zip(edges(bounds, state("white_concrete"), 0xFF_AA00))
        {
            *slot = Some(edge);
        }
    }
    parts
}

fn metadata(part: &Part, interpolation: u16) -> Vec<EntityMetadataEntry> {
    let transform = DisplayTransform::default().scale(part.scale[0], part.scale[1], part.scale[2]);
    let mut entries = transform.entries(interpolation);
    let longest = part.scale.iter().fold(0.0f32, |a, b| a.max(*b));
    entries.extend([
        EntityMetadataEntry {
            index: display_index::WIDTH,
            value: Value::Float(longest * 2.0 + 1.0),
        },
        EntityMetadataEntry {
            index: display_index::HEIGHT,
            value: Value::Float(part.scale[1] + 1.0),
        },
    ]);
    entries
}

fn spawn_metadata(part: &Part) -> Vec<EntityMetadataEntry> {
    let mut entries = vec![
        EntityMetadataEntry {
            index: entity_index::FLAGS,
            value: Value::Byte(entity_flag::GLOWING as i8),
        },
        EntityMetadataEntry {
            index: display_index::POS_ROT_DURATION,
            value: Value::Int(i32::from(INTERPOLATION_TICKS)),
        },
        EntityMetadataEntry {
            index: display_index::BRIGHTNESS,
            value: Value::Int(pack_brightness(Some((15, 15)))),
        },
        EntityMetadataEntry {
            index: display_index::VIEW_RANGE,
            value: Value::Float(VIEW_RANGE),
        },
        EntityMetadataEntry {
            index: display_index::GLOW_COLOR,
            value: Value::Int(part.glow),
        },
        EntityMetadataEntry {
            index: display_index::BLOCK_STATE,
            value: Value::BlockState(part.block.0 as i32),
        },
    ];
    entries.extend(metadata(part, 0));
    entries
}

fn block_display_type() -> i32 {
    voidmc_data::entity_type_id(VERSION, "minecraft:block_display").unwrap_or_default()
}

pub(crate) fn sync_outlines(
    config: Res<WorldEditConfig>,
    players: Players,
    mut commands: Commands,
    mut sessions: Query<(
        Entity,
        Ref<EditSession>,
        &PlayerDimension,
        Option<&mut Outline>,
    )>,
) {
    if !config.show_selection {
        return;
    }
    for (player, session, dimension, outline) in sessions.iter_mut() {
        let Some(mut outline) = outline else {
            commands.entity(player).try_insert(Outline::default());
            continue;
        };
        let revision = session.selection.revision();
        if outline.revision == Some(revision) && outline.dimension == Some(dimension.0) {
            continue;
        }
        if outline.dimension != Some(dimension.0) {
            let stale: Vec<i32> = outline
                .parts
                .iter_mut()
                .filter_map(|shown| shown.part.take().map(|_| shown.id))
                .collect();
            if !stale.is_empty() {
                players.send(player, RemoveEntities { entity_ids: stale });
            }
        }
        outline.revision = Some(revision);
        outline.dimension = Some(dimension.0);

        let mut removed = Vec::new();
        for (shown, wanted) in outline.parts.iter_mut().zip(desired(&session)) {
            match (shown.part, wanted) {
                (None, Some(part)) => {
                    players.send(
                        player,
                        SpawnEntity {
                            entity_id: shown.id,
                            entity_uuid: uuid::Uuid::new_v4(),
                            entity_type: block_display_type(),
                            x: part.position[0],
                            y: part.position[1],
                            z: part.position[2],
                            velocity: LpVec3::ZERO,
                            pitch: 0,
                            yaw: 0,
                            head_yaw: 0,
                            data: 0,
                        },
                    );
                    players.send(
                        player,
                        SetEntityData {
                            entity_id: shown.id,
                            entries: spawn_metadata(&part),
                        },
                    );
                }
                (Some(old), Some(part)) if old != part => {
                    if old.position != part.position {
                        players.send(
                            player,
                            TeleportEntity {
                                entity_id: shown.id,
                                x: part.position[0],
                                y: part.position[1],
                                z: part.position[2],
                                vx: 0.0,
                                vy: 0.0,
                                vz: 0.0,
                                yaw: 0.0,
                                pitch: 0.0,
                                relatives: TeleportFlags::empty(),
                                on_ground: false,
                            },
                        );
                    }
                    if old.scale != part.scale {
                        players.send(
                            player,
                            SetEntityData {
                                entity_id: shown.id,
                                entries: metadata(&part, INTERPOLATION_TICKS),
                            },
                        );
                    }
                }
                (Some(_), None) => removed.push(shown.id),
                _ => {}
            }
            shown.part = wanted;
        }
        if !removed.is_empty() {
            players.send(
                player,
                RemoveEntities {
                    entity_ids: removed,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_edges_frame_the_selection_bounds() {
        let bounds = Cuboid::new(BlockPos::new(0, 64, 0), BlockPos::new(3, 65, 9));
        let edges: Vec<Part> = edges(bounds, BlockState::AIR, 0).collect();
        assert_eq!(edges.len(), 12);
        let along_x: Vec<_> = edges.iter().filter(|e| e.scale[0] > EDGE).collect();
        assert_eq!(along_x.len(), 4);
        assert!(
            along_x
                .iter()
                .all(|e| e.scale[0] == 4.0 && e.position[0] == 0.0)
        );
        let along_y: Vec<_> = edges.iter().filter(|e| e.scale[1] > EDGE).collect();
        assert!(
            along_y
                .iter()
                .all(|e| e.scale[1] == 2.0 && e.position[1] == 64.0)
        );
        let along_z: Vec<_> = edges.iter().filter(|e| e.scale[2] > EDGE).collect();
        assert!(along_z.iter().all(|e| e.scale[2] == 10.0));
        let corner_x: Vec<f64> = along_y.iter().map(|e| e.position[0]).collect();
        assert!(corner_x.contains(&(-f64::from(EDGE) / 2.0)));
        assert!(corner_x.contains(&(4.0 - f64::from(EDGE) / 2.0)));
    }

    #[test]
    fn markers_wrap_their_block() {
        let part = marker(BlockPos::new(1, 2, 3), BlockState::AIR, 0);
        assert!(part.position[0] < 1.0 && part.position[0] > 0.98);
        assert_eq!(part.scale, [MARKER; 3]);
    }
}
