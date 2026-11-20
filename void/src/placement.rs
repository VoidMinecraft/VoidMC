//! Vanilla's rules for a block a player places by hand: which block a click
//! fills, whether vanilla refuses it, and the state the block takes from the
//! clicked face, the cursor and the player's rotation. Also the block
//! interaction range that bounds every click on a block.
//!
//! The rules follow Minecraft 26.1.2 (`BlockItem`, `BlockPlaceContext` and
//! each block's `getStateForPlacement`). Only the property families `axis`,
//! `facing`, `face`, `half` and slab `type` are computed, for the block kinds
//! listed in [`state_for_placement`]; every other block, and every other
//! property (waterlogging, shapes that depend on the neighbours, redstone
//! power), keeps its default value.

use bevy_ecs::prelude::*;
use voidmc_data::{BlockType, Version, block_type_of_state};
use voidmc_protocol::types::{BlockFace, BlockPosition};

use crate::components::{Position, Sneaking};
use crate::config::ServerConfigResource;
use crate::plugins::abilities::PlayerAbilities;
use crate::world::offset_position;

const VERSION: Version = Version::V26_1_2;

/// The configured game mode of a creative server.
const CREATIVE: u8 = 1;

/// A player's eye height standing, and crouching (sneaking, not flying).
const STANDING_EYE_HEIGHT: f64 = 1.62;
const CROUCHING_EYE_HEIGHT: f64 = 1.27;

/// The default `block_interaction_range` attribute, and the `+0.5` vanilla
/// adds to it in Creative.
const BLOCK_INTERACTION_RANGE: f64 = 4.5;
const CREATIVE_BLOCK_INTERACTION_RANGE: f64 = 5.0;

/// The margin a server allows beyond the interaction range
/// (`isWithinBlockInteractionRange(pos, 1.0)`).
const RANGE_BUFFER: f64 = 1.0;

/// Whether the server-wide game mode is Creative.
pub(crate) fn creative(world: &World) -> bool {
    world
        .get_resource::<ServerConfigResource>()
        .is_some_and(|config| config.game_mode == CREATIVE)
}

/// Whether `player` can reach the block at `position`: the distance from its
/// eyes to the nearest point of the block is below its block interaction
/// range (4.5, 5 in Creative) plus 1, as a vanilla server checks for
/// placing, using and breaking. A player without a position yet is not
/// limited.
pub(crate) fn within_reach(world: &World, player: Entity, position: BlockPosition) -> bool {
    let Some(feet) = world.get::<Position>(player) else {
        return true;
    };
    let sneaking = world.get::<Sneaking>(player).is_some_and(|s| s.0);
    let flying = world
        .get::<PlayerAbilities>(player)
        .is_some_and(|abilities| abilities.flying);
    let eye_height = if sneaking && !flying {
        CROUCHING_EYE_HEIGHT
    } else {
        STANDING_EYE_HEIGHT
    };
    let range = if creative(world) {
        CREATIVE_BLOCK_INTERACTION_RANGE
    } else {
        BLOCK_INTERACTION_RANGE
    };
    within_range((feet.x, feet.y + eye_height, feet.z), position, range)
}

/// Vanilla's `new AABB(pos).distanceToSqr(eye) < (range + 1)²`.
fn within_range(eye: (f64, f64, f64), position: BlockPosition, range: f64) -> bool {
    let gap = |eye: f64, min: i32| {
        let min = f64::from(min);
        (min - eye).max(eye - (min + 1.0)).max(0.0)
    };
    let dx = gap(eye.0, position.x);
    let dy = gap(eye.1, i32::from(position.y));
    let dz = gap(eye.2, position.z);
    let reach = range + RANGE_BUFFER;
    dx * dx + dy * dy + dz * dz < reach * reach
}

/// A right-click on a block with a block item.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Click {
    pub clicked: BlockPosition,
    pub face: BlockFace,
    /// The cursor on the clicked face, relative to the clicked block.
    pub cursor: (f32, f32, f32),
    pub yaw: f32,
    pub pitch: f32,
}

impl Click {
    /// Vanilla's `getClickLocation().y - pos.getY() > 0.5`: the click is in
    /// the upper half of the block at `position`.
    fn in_upper_half(&self, position: BlockPosition) -> bool {
        f64::from(self.clicked.y) + f64::from(self.cursor.1) - f64::from(position.y) > 0.5
    }
}

/// What the default placement of a click does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    /// Puts `state` at `position`, unless an entity is in the way.
    Place { position: BlockPosition, state: i32 },
    /// Vanilla refuses the click; `target` is the block it tried to fill.
    Refused { target: BlockPosition },
}

/// Where and as what `item_block` (the block of the used item) is placed for
/// `click`, with `block_at` reading the world (`None` outside the loaded
/// world). Like vanilla, the clicked block itself is filled when it is
/// replaceable, else the block across the clicked face, and only if that
/// one is replaceable.
pub(crate) fn place(
    item_block: &'static BlockType,
    click: &Click,
    block_at: impl Fn(BlockPosition) -> Option<i32>,
) -> Placement {
    let across = offset_position(click.clicked, click.face);
    let Some(clicked_state) = block_at(click.clicked) else {
        return Placement::Refused { target: across };
    };
    let replace_clicked = can_be_replaced(clicked_state, item_block, click, true);
    let position = if replace_clicked {
        click.clicked
    } else {
        match block_at(across) {
            Some(state) if can_be_replaced(state, item_block, click, false) => across,
            _ => return Placement::Refused { target: across },
        }
    };
    match state_for_placement(item_block, click, position, replace_clicked, &block_at) {
        Some(state) => Placement::Place { position, state },
        None => Placement::Refused { target: position },
    }
}

/// Vanilla's `BlockState.canBeReplaced(BlockPlaceContext)` for a placement of
/// `item_block`. `replacing_clicked` is whether `state` is the clicked block
/// (as `BlockPlaceContext.replacingClickedOnBlock()` reads while it is being
/// decided). Merging a second item into the same block is only done for
/// slabs: for snow layers, vines and multiface blocks it is refused.
fn can_be_replaced(
    state: i32,
    item_block: &BlockType,
    click: &Click,
    replacing_clicked: bool,
) -> bool {
    let Some(block) = block_type_of_state(VERSION, state) else {
        return false;
    };
    let same_item = block.name == item_block.name;
    match block.kind {
        "minecraft:slab" | "minecraft:weathering_copper_slab" => {
            let kind = block.property(state, "type");
            if !same_item || kind == Some("double") {
                return false;
            }
            if !replacing_clicked {
                return true;
            }
            let upper = click.in_upper_half(click.clicked);
            let horizontal = is_horizontal(click.face);
            if kind == Some("bottom") {
                click.face == BlockFace::Top || upper && horizontal
            } else {
                click.face == BlockFace::Bottom || !upper && horizontal
            }
        }
        "minecraft:snow_layer" => !same_item && block.property(state, "layers") == Some("1"),
        "minecraft:vine" => {
            let faces = ["up", "north", "east", "south", "west"]
                .iter()
                .filter(|side| block.property(state, side) == Some("true"))
                .count();
            !same_item && faces < 5
        }
        "minecraft:glow_lichen" | "minecraft:sculk_vein" | "minecraft:multiface" => !same_item,
        _ => block.replaceable && !same_item,
    }
}

fn opposite(face: BlockFace) -> BlockFace {
    match face {
        BlockFace::Bottom => BlockFace::Top,
        BlockFace::Top => BlockFace::Bottom,
        BlockFace::North => BlockFace::South,
        BlockFace::South => BlockFace::North,
        BlockFace::West => BlockFace::East,
        BlockFace::East => BlockFace::West,
    }
}

fn is_horizontal(face: BlockFace) -> bool {
    !matches!(face, BlockFace::Bottom | BlockFace::Top)
}

/// The next horizontal direction clockwise seen from above.
fn clockwise(face: BlockFace) -> BlockFace {
    match face {
        BlockFace::North => BlockFace::East,
        BlockFace::East => BlockFace::South,
        BlockFace::South => BlockFace::West,
        BlockFace::West => BlockFace::North,
        vertical => vertical,
    }
}

/// The value of a `facing` property.
fn direction_name(face: BlockFace) -> &'static str {
    match face {
        BlockFace::Bottom => "down",
        BlockFace::Top => "up",
        BlockFace::North => "north",
        BlockFace::South => "south",
        BlockFace::West => "west",
        BlockFace::East => "east",
    }
}

/// The value of an `axis` property.
fn axis_name(face: BlockFace) -> &'static str {
    match face {
        BlockFace::Bottom | BlockFace::Top => "y",
        BlockFace::North | BlockFace::South => "z",
        BlockFace::West | BlockFace::East => "x",
    }
}

/// The horizontal direction a player with this yaw faces
/// (`Direction.fromYRot`).
fn horizontal_direction(yaw: f32) -> BlockFace {
    match ((f64::from(yaw) / 90.0 + 0.5).floor() as i64) & 3 {
        0 => BlockFace::South,
        1 => BlockFace::West,
        2 => BlockFace::North,
        _ => BlockFace::East,
    }
}

/// `Mth.sin` and `Mth.cos`: a sine read from vanilla's 65536-entry table.
fn table_sin(radians: f64) -> f32 {
    table_entry((radians * 10430.378350470453) as i64)
}

fn table_cos(radians: f64) -> f32 {
    table_entry((radians * 10430.378350470453 + 16384.0) as i64)
}

fn table_entry(index: i64) -> f32 {
    ((index & 65535) as f64 * std::f64::consts::TAU / 65536.0).sin() as f32
}

/// The six directions ordered from the one the player looks along most to
/// its opposite (`Direction.orderedByNearest`).
fn ordered_by_nearest(yaw: f32, pitch: f32) -> [BlockFace; 6] {
    let pitch = pitch * 0.017_453_292;
    let yaw = -yaw * 0.017_453_292;
    let sin_pitch = table_sin(f64::from(pitch));
    let cos_pitch = table_cos(f64::from(pitch));
    let sin_yaw = table_sin(f64::from(yaw));
    let cos_yaw = table_cos(f64::from(yaw));
    let east = sin_yaw > 0.0;
    let up = sin_pitch < 0.0;
    let south = cos_yaw > 0.0;
    let x = if east { sin_yaw } else { -sin_yaw };
    let y = if up { -sin_pitch } else { sin_pitch };
    let z = if south { cos_yaw } else { -cos_yaw };
    let x_flat = x * cos_pitch;
    let z_flat = z * cos_pitch;
    let x_dir = if east {
        BlockFace::East
    } else {
        BlockFace::West
    };
    let y_dir = if up {
        BlockFace::Top
    } else {
        BlockFace::Bottom
    };
    let z_dir = if south {
        BlockFace::South
    } else {
        BlockFace::North
    };
    let order =
        |a: BlockFace, b: BlockFace, c: BlockFace| [a, b, c, opposite(c), opposite(b), opposite(a)];
    if x > z {
        if y > x_flat {
            order(y_dir, x_dir, z_dir)
        } else if z_flat > y {
            order(x_dir, z_dir, y_dir)
        } else {
            order(x_dir, y_dir, z_dir)
        }
    } else if y > z_flat {
        order(y_dir, z_dir, x_dir)
    } else if x_flat > y {
        order(z_dir, x_dir, y_dir)
    } else {
        order(z_dir, y_dir, x_dir)
    }
}

/// `BlockPlaceContext.getNearestLookingDirections()`: the look order, with
/// the side of the clicked block moved first unless the clicked block is
/// replaced.
fn nearest_looking_directions(click: &Click, replace_clicked: bool) -> [BlockFace; 6] {
    let mut directions = ordered_by_nearest(click.yaw, click.pitch);
    if !replace_clicked {
        let toward_clicked = opposite(click.face);
        let index = directions
            .iter()
            .position(|direction| *direction == toward_clicked)
            .unwrap_or(0);
        directions[..=index].rotate_right(1);
    }
    directions
}

/// The state vanilla gives `block` placed at `position`, or `None` when it
/// refuses the placement (an attached block with nothing to attach to).
///
/// Covered, by vanilla block kind:
/// - `axis` from the clicked face: logs and other pillars, hay, chains, the
///   infested pillar and the creaking heart;
/// - `facing` toward the player: furnaces, smokers, blast furnaces, carved
///   pumpkins, glazed terracotta, looms, stonecutters, beehives, chiseled
///   bookshelves, lecterns, end portal frames, ender chests, shelves, copper
///   golem statues, dried ghasts, repeaters and comparators; along the
///   player's look: campfires, decorated pots, calibrated sculk sensors,
///   fence gates; clockwise of it: anvils;
/// - six-way `facing`: back along the look for dispensers, droppers,
///   pistons and barrels, along it for observers, the clicked face for
///   shulker boxes, lightning rods, end rods and amethyst buds, hoppers;
/// - wall-attached `facing`: ladders and tripwire hooks;
/// - `face` and `facing`: levers, buttons and grindstones;
/// - `half`: stairs and trapdoors; slab `type`, a second slab making it
///   double.
fn state_for_placement(
    block: &BlockType,
    click: &Click,
    position: BlockPosition,
    replace_clicked: bool,
    block_at: &impl Fn(BlockPosition) -> Option<i32>,
) -> Option<i32> {
    let default = block.default_state_id;
    let with = |state: i32, property: &str, value: &str| {
        block.with_property(state, property, value).unwrap_or(state)
    };
    let facing = |face: BlockFace| with(default, "facing", direction_name(face));
    let horizontal = horizontal_direction(click.yaw);
    let sturdy = |direction: BlockFace| {
        // The face of the block across `direction` that faces `position`.
        block_at(offset_position(position, direction))
            .is_some_and(|state| face_sturdy(state, opposite(direction)))
    };
    let state = match block.kind {
        "minecraft:rotated_pillar"
        | "minecraft:hay"
        | "minecraft:infested_rotated_pillar"
        | "minecraft:chain"
        | "minecraft:weathering_copper_chain"
        | "minecraft:creaking_heart" => with(default, "axis", axis_name(click.face)),
        "minecraft:furnace"
        | "minecraft:blast_furnace"
        | "minecraft:smoker"
        | "minecraft:jack_o_lantern"
        | "minecraft:glazed_terracotta"
        | "minecraft:loom"
        | "minecraft:stonecutter"
        | "minecraft:beehive"
        | "minecraft:chiseled_book_shelf"
        | "minecraft:lectern"
        | "minecraft:end_portal_frame"
        | "minecraft:ender_chest"
        | "minecraft:shelf"
        | "minecraft:copper_golem_statue"
        | "minecraft:weathering_copper_golem_statue"
        | "minecraft:dried_ghast"
        | "minecraft:repeater"
        | "minecraft:comparator" => facing(opposite(horizontal)),
        "minecraft:campfire"
        | "minecraft:decorated_pot"
        | "minecraft:calibrated_sculk_sensor"
        | "minecraft:fence_gate" => facing(horizontal),
        "minecraft:anvil" => facing(clockwise(horizontal)),
        "minecraft:dispenser"
        | "minecraft:dropper"
        | "minecraft:piston_base"
        | "minecraft:barrel" => facing(opposite(ordered_by_nearest(click.yaw, click.pitch)[0])),
        "minecraft:observer" => facing(ordered_by_nearest(click.yaw, click.pitch)[0]),
        "minecraft:shulker_box"
        | "minecraft:lightning_rod"
        | "minecraft:weathering_lightning_rod" => facing(click.face),
        "minecraft:amethyst_cluster" => {
            if !sturdy(opposite(click.face)) {
                return None;
            }
            facing(click.face)
        }
        "minecraft:end_rod" => {
            // Against an end rod pointing this way, it points back at it.
            let behind = block_at(offset_position(position, opposite(click.face)));
            let rod_this_way = behind.is_some_and(|state| {
                block.contains(state)
                    && block.property(state, "facing") == Some(direction_name(click.face))
            });
            facing(if rod_this_way {
                opposite(click.face)
            } else {
                click.face
            })
        }
        "minecraft:hopper" => {
            let toward = opposite(click.face);
            facing(if is_horizontal(toward) {
                toward
            } else {
                BlockFace::Bottom
            })
        }
        "minecraft:ladder" => {
            if !replace_clicked {
                let behind = block_at(offset_position(position, opposite(click.face)));
                if behind.is_some_and(|state| {
                    block.contains(state)
                        && block.property(state, "facing") == Some(direction_name(click.face))
                }) {
                    return None;
                }
            }
            let direction = nearest_looking_directions(click, replace_clicked)
                .into_iter()
                .find(|direction| is_horizontal(*direction) && sturdy(*direction))?;
            facing(opposite(direction))
        }
        "minecraft:trip_wire_hook" => {
            let direction = nearest_looking_directions(click, replace_clicked)
                .into_iter()
                .find(|direction| is_horizontal(*direction) && sturdy(*direction))?;
            facing(opposite(direction))
        }
        "minecraft:lever" | "minecraft:button" | "minecraft:grindstone" => {
            let floats = block.kind == "minecraft:grindstone";
            let direction = nearest_looking_directions(click, replace_clicked)
                .into_iter()
                .find(|direction| floats || sturdy(*direction))?;
            let (face, facing_value) = match direction {
                BlockFace::Top => ("ceiling", horizontal),
                BlockFace::Bottom => ("floor", horizontal),
                wall => ("wall", opposite(wall)),
            };
            with(facing(facing_value), "face", face)
        }
        "minecraft:stair" | "minecraft:weathering_copper_stair" => {
            let half = if click.face != BlockFace::Bottom
                && (click.face == BlockFace::Top || !click.in_upper_half(position))
            {
                "bottom"
            } else {
                "top"
            };
            with(facing(horizontal), "half", half)
        }
        "minecraft:trapdoor" | "minecraft:weathering_copper_trap_door" => {
            if !replace_clicked && is_horizontal(click.face) {
                let half = if click.in_upper_half(position) {
                    "top"
                } else {
                    "bottom"
                };
                with(facing(click.face), "half", half)
            } else {
                let half = if click.face == BlockFace::Top {
                    "bottom"
                } else {
                    "top"
                };
                with(facing(opposite(horizontal)), "half", half)
            }
        }
        "minecraft:slab" | "minecraft:weathering_copper_slab" => {
            if let Some(existing) = block_at(position).filter(|state| block.contains(*state)) {
                let double = with(existing, "type", "double");
                return Some(with(double, "waterlogged", "false"));
            }
            let kind = if click.face != BlockFace::Bottom
                && (click.face == BlockFace::Top || !click.in_upper_half(position))
            {
                "bottom"
            } else {
                "top"
            };
            with(default, "type", kind)
        }
        _ => default,
    };
    Some(state)
}

/// Vanilla's `isFaceSturdy(.., SupportType.FULL)`: the block's support shape
/// covers the whole of `face`. The support shape is the collision shape,
/// except for leaves (none), mud and soul sand (a full block) and snow
/// layers (their height, one layer more than they collide with).
pub(crate) fn face_sturdy(state: i32, face: BlockFace) -> bool {
    let Some(block) = block_type_of_state(VERSION, state) else {
        return false;
    };
    match block.kind {
        "minecraft:tinted_particle_leaves"
        | "minecraft:untinted_particle_leaves"
        | "minecraft:mangrove_leaves" => false,
        "minecraft:mud" | "minecraft:soul_sand" => true,
        "minecraft:snow_layer" => {
            face == BlockFace::Bottom || block.property(state, "layers") == Some("8")
        }
        _ => covers_face(voidmc_data::v26_1_2::shapes::for_state(state), face),
    }
}

/// Whether the boxes reaching `face` of the unit cube cover all of it.
fn covers_face(boxes: &[voidmc_data::v26_1_2::shapes::Aabb], face: BlockFace) -> bool {
    const EPSILON: f32 = 1.0e-7;
    // Each box touching the face, as a rectangle (u0, v0, u1, v1) on it.
    let rectangles: Vec<[f32; 4]> = boxes
        .iter()
        .filter(|b| match face {
            BlockFace::Bottom => b.y0 <= EPSILON,
            BlockFace::Top => b.y1 >= 1.0 - EPSILON,
            BlockFace::North => b.z0 <= EPSILON,
            BlockFace::South => b.z1 >= 1.0 - EPSILON,
            BlockFace::West => b.x0 <= EPSILON,
            BlockFace::East => b.x1 >= 1.0 - EPSILON,
        })
        .map(|b| match face {
            BlockFace::Bottom | BlockFace::Top => [b.x0, b.z0, b.x1, b.z1],
            BlockFace::North | BlockFace::South => [b.x0, b.y0, b.x1, b.y1],
            BlockFace::West | BlockFace::East => [b.z0, b.y0, b.z1, b.y1],
        })
        .collect();
    let cuts = |lo: usize, hi: usize| {
        let mut cuts: Vec<f32> = rectangles
            .iter()
            .flat_map(|r| [r[lo], r[hi]])
            .map(|cut| cut.clamp(0.0, 1.0))
            .chain([0.0, 1.0])
            .collect();
        cuts.sort_by(f32::total_cmp);
        cuts.dedup();
        cuts
    };
    let (us, vs) = (cuts(0, 2), cuts(1, 3));
    us.windows(2).all(|u| {
        let mid_u = (u[0] + u[1]) / 2.0;
        vs.windows(2).all(|v| {
            let mid_v = (v[0] + v[1]) / 2.0;
            rectangles
                .iter()
                .any(|r| r[0] <= mid_u && mid_u <= r[2] && r[1] <= mid_v && mid_v <= r[3])
        })
    })
}

/// Whether entities of this kind keep blocks from being placed into them
/// (vanilla's `Entity.blocksBuilding`): living entities, primed TNT,
/// falling blocks, minecarts, boats and end crystals. Items, experience
/// orbs and projectiles do not.
pub(crate) fn blocks_building(kind: crate::entity::EntityKind) -> bool {
    let name = kind.name();
    kind.is_living()
        || matches!(
            name,
            "minecraft:tnt" | "minecraft:falling_block" | "minecraft:end_crystal"
        )
        || name.ends_with("minecart")
        || name.ends_with("_boat")
        || name.ends_with("_raft")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const AIR: i32 = 0;

    fn block(name: &str) -> &'static BlockType {
        voidmc_data::block_type(VERSION, &format!("minecraft:{name}"))
            .unwrap_or_else(|| panic!("no block {name}"))
    }

    /// The state of `name` with the listed properties.
    fn state(name: &str, properties: &[(&str, &str)]) -> i32 {
        let block = block(name);
        properties
            .iter()
            .fold(block.default_state_id, |state, (property, value)| {
                block
                    .with_property(state, property, value)
                    .unwrap_or_else(|| panic!("{name} has no {property}={value}"))
            })
    }

    fn describe(state: i32) -> String {
        let block = block_type_of_state(VERSION, state).unwrap();
        let properties: Vec<String> = block
            .property_values(state)
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        format!("{}[{}]", block.name, properties.join(","))
    }

    const ORIGIN: BlockPosition = BlockPosition { x: 0, y: 64, z: 0 };

    fn at(dx: i32, dy: i16, dz: i32) -> BlockPosition {
        BlockPosition {
            x: dx,
            y: 64 + dy,
            z: dz,
        }
    }

    /// A world of air with the listed blocks.
    struct World(HashMap<(i32, i16, i32), i32>);

    impl World {
        fn with(blocks: &[(BlockPosition, i32)]) -> Self {
            Self(
                blocks
                    .iter()
                    .map(|(pos, state)| ((pos.x, pos.y, pos.z), *state))
                    .collect(),
            )
        }

        fn get(&self, pos: BlockPosition) -> Option<i32> {
            Some(self.0.get(&(pos.x, pos.y, pos.z)).copied().unwrap_or(AIR))
        }
    }

    /// The face's centre, or `cursor_y` up the face for a side face.
    fn click(face: BlockFace, cursor_y: f32, yaw: f32, pitch: f32) -> Click {
        let cursor = match face {
            BlockFace::Top => (0.5, 1.0, 0.5),
            BlockFace::Bottom => (0.5, 0.0, 0.5),
            BlockFace::North => (0.5, cursor_y, 0.0),
            BlockFace::South => (0.5, cursor_y, 1.0),
            BlockFace::West => (0.0, cursor_y, 0.5),
            BlockFace::East => (1.0, cursor_y, 0.5),
        };
        Click {
            clicked: ORIGIN,
            face,
            cursor,
            yaw,
            pitch,
        }
    }

    /// Places `item` with `click` on a stone at the origin; panics when
    /// refused.
    fn placed(world: &World, item: &str, click: Click) -> String {
        match place(block(item), &click, |pos| world.get(pos)) {
            Placement::Place { state, .. } => describe(state),
            Placement::Refused { target } => panic!("{item} refused at {target:?}"),
        }
    }

    fn on_stone() -> World {
        World::with(&[(ORIGIN, state("stone", &[]))])
    }

    const SOUTH: f32 = 0.0;
    const WEST: f32 = 90.0;
    const NORTH: f32 = 180.0;
    const EAST: f32 = -90.0;

    #[test]
    fn the_block_across_the_clicked_face_is_filled_only_if_replaceable() {
        let stone = state("stone", &[]);
        let glass = block("glass");
        let top = click(BlockFace::Top, 0.0, SOUTH, 0.0);
        for (above, expected) in [
            (AIR, Some(at(0, 1, 0))),
            (state("water", &[]), Some(at(0, 1, 0))),
            (state("short_grass", &[]), Some(at(0, 1, 0))),
            (state("snow", &[("layers", "1")]), Some(at(0, 1, 0))),
            (state("snow", &[("layers", "2")]), None),
            (state("glow_lichen", &[]), Some(at(0, 1, 0))),
            (state("sculk_vein", &[]), Some(at(0, 1, 0))),
            (stone, None),
            (state("dandelion", &[]), None),
            (state("glass", &[]), None),
        ] {
            let world = World::with(&[(ORIGIN, stone), (at(0, 1, 0), above)]);
            let outcome = place(glass, &top, |pos| world.get(pos));
            match expected {
                Some(position) => assert!(
                    matches!(outcome, Placement::Place { position: p, .. } if p == position),
                    "on {}: {outcome:?}",
                    describe(above)
                ),
                None => assert_eq!(
                    outcome,
                    Placement::Refused {
                        target: at(0, 1, 0)
                    },
                    "on {}",
                    describe(above)
                ),
            }
        }
    }

    #[test]
    fn a_replaceable_clicked_block_is_filled_itself() {
        let world = World::with(&[(ORIGIN, state("short_grass", &[]))]);
        let outcome = place(
            block("stone"),
            &click(BlockFace::Top, 0.0, SOUTH, 0.0),
            |pos| world.get(pos),
        );
        assert_eq!(
            outcome,
            Placement::Place {
                position: ORIGIN,
                state: state("stone", &[])
            }
        );
        // Short grass does not replace short grass: it goes above.
        let outcome = place(
            block("short_grass"),
            &click(BlockFace::Top, 0.0, SOUTH, 0.0),
            |pos| world.get(pos),
        );
        assert!(matches!(outcome, Placement::Place { position, .. } if position == at(0, 1, 0)));
    }

    #[test]
    fn unloaded_blocks_refuse_the_placement() {
        let outcome = place(
            block("stone"),
            &click(BlockFace::Top, 0.0, SOUTH, 0.0),
            |_| None,
        );
        assert_eq!(
            outcome,
            Placement::Refused {
                target: at(0, 1, 0)
            }
        );
    }

    #[test]
    fn pillars_take_the_axis_of_the_clicked_face() {
        let world = on_stone();
        for (face, axis) in [
            (BlockFace::Top, "y"),
            (BlockFace::Bottom, "y"),
            (BlockFace::North, "z"),
            (BlockFace::South, "z"),
            (BlockFace::East, "x"),
            (BlockFace::West, "x"),
        ] {
            for item in [
                "oak_log",
                "basalt",
                "hay_block",
                "iron_chain",
                "infested_deepslate",
            ] {
                let placed = placed(&world, item, click(face, 0.5, SOUTH, 0.0));
                assert!(
                    placed.contains(&format!("axis={axis}")),
                    "{item} on {face:?}: {placed}"
                );
            }
        }
    }

    #[test]
    fn horizontal_facing_follows_the_player_yaw() {
        let world = on_stone();
        let top = |yaw| click(BlockFace::Top, 0.0, yaw, 30.0);
        for (yaw, toward, along, clockwise) in [
            (SOUTH, "north", "south", "west"),
            (WEST, "east", "west", "north"),
            (NORTH, "south", "north", "east"),
            (EAST, "west", "east", "south"),
            (44.0, "north", "south", "west"),
            (46.0, "east", "west", "north"),
            (-450.0, "west", "east", "south"),
        ] {
            assert_eq!(
                placed(&world, "furnace", top(yaw)),
                format!("minecraft:furnace[facing={toward},lit=false]")
            );
            assert!(
                placed(&world, "carved_pumpkin", top(yaw)).contains(&format!("facing={toward}"))
            );
            assert!(placed(&world, "repeater", top(yaw)).contains(&format!("facing={toward}")));
            assert!(
                placed(&world, "oak_fence_gate", top(yaw)).contains(&format!("facing={along}"))
            );
            assert!(placed(&world, "anvil", top(yaw)).contains(&format!("facing={clockwise}")));
        }
    }

    #[test]
    fn six_way_facing_follows_the_look() {
        let world = on_stone();
        let top = |yaw, pitch| click(BlockFace::Top, 0.0, yaw, pitch);
        for (yaw, pitch, look, back) in [
            (SOUTH, 0.0, "south", "north"),
            (EAST, 10.0, "east", "west"),
            (NORTH, 90.0, "down", "up"),
            (WEST, -90.0, "up", "down"),
            (SOUTH, 50.0, "down", "up"),
            (SOUTH, 40.0, "south", "north"),
        ] {
            for item in ["dispenser", "dropper", "piston", "barrel"] {
                let placed = placed(&world, item, top(yaw, pitch));
                assert!(
                    placed.contains(&format!("facing={back}")),
                    "{item} looking {look}: {placed}"
                );
            }
            let observer = placed(&world, "observer", top(yaw, pitch));
            assert!(observer.contains(&format!("facing={look}")), "{observer}");
        }
    }

    #[test]
    fn clicked_face_blocks_point_away_from_the_clicked_block() {
        let world = on_stone();
        for face in [
            BlockFace::Top,
            BlockFace::Bottom,
            BlockFace::North,
            BlockFace::East,
        ] {
            let name = direction_name(face);
            for item in [
                "shulker_box",
                "lightning_rod",
                "end_rod",
                "amethyst_cluster",
            ] {
                let placed = placed(&world, item, click(face, 0.5, SOUTH, 0.0));
                assert!(
                    placed.contains(&format!("facing={name}")),
                    "{item}: {placed}"
                );
            }
        }
        // A hopper points into the clicked block, or down from a top or
        // bottom face.
        for (face, expected) in [
            (BlockFace::Top, "down"),
            (BlockFace::Bottom, "down"),
            (BlockFace::North, "south"),
            (BlockFace::West, "east"),
        ] {
            let placed = placed(&world, "hopper", click(face, 0.5, SOUTH, 0.0));
            assert!(placed.contains(&format!("facing={expected}")), "{placed}");
        }
    }

    #[test]
    fn an_end_rod_against_an_end_rod_pointing_at_it_points_back() {
        let rod_up = state("end_rod", &[("facing", "up")]);
        let world = World::with(&[(ORIGIN, rod_up)]);
        let placed = placed(&world, "end_rod", click(BlockFace::Top, 0.0, SOUTH, 0.0));
        assert_eq!(placed, "minecraft:end_rod[facing=down]");
    }

    #[test]
    fn amethyst_needs_a_sturdy_face_to_grow_on() {
        let world = World::with(&[(ORIGIN, state("glass_pane", &[]))]);
        let outcome = place(
            block("amethyst_cluster"),
            &click(BlockFace::Top, 0.0, SOUTH, 0.0),
            |pos| world.get(pos),
        );
        assert_eq!(
            outcome,
            Placement::Refused {
                target: at(0, 1, 0)
            }
        );
    }

    #[test]
    fn levers_and_buttons_attach_to_the_clicked_block() {
        let world = on_stone();
        for (face, yaw, expected) in [
            (BlockFace::Top, EAST, "face=floor,facing=east"),
            (BlockFace::Top, NORTH, "face=floor,facing=north"),
            (BlockFace::Bottom, WEST, "face=ceiling,facing=west"),
            (BlockFace::North, EAST, "face=wall,facing=north"),
            (BlockFace::East, SOUTH, "face=wall,facing=east"),
        ] {
            for item in ["lever", "stone_button", "oak_button"] {
                let placed = placed(&world, item, click(face, 0.5, yaw, 0.0));
                assert!(placed.contains(expected), "{item} on {face:?}: {placed}");
                assert!(placed.contains("powered=false"));
            }
        }
    }

    #[test]
    fn a_lever_on_a_block_without_a_sturdy_face_tries_the_look_directions() {
        // The clicked glass pane's top is not sturdy; looking north and a
        // little down, the next candidate is the stone north of the target.
        let pane = state("glass_pane", &[]);
        let stone = state("stone", &[]);
        let world = World::with(&[(ORIGIN, pane), (at(0, 1, -1), stone)]);
        let lever = placed(&world, "lever", click(BlockFace::Top, 0.0, NORTH, 20.0));
        assert!(lever.contains("face=wall,facing=south"), "{lever}");
        // With nothing sturdy around, the lever is refused; a grindstone
        // needs no support.
        let world = World::with(&[(ORIGIN, pane)]);
        let outcome = place(
            block("lever"),
            &click(BlockFace::Top, 0.0, NORTH, 20.0),
            |pos| world.get(pos),
        );
        assert_eq!(
            outcome,
            Placement::Refused {
                target: at(0, 1, 0)
            }
        );
        let grindstone = placed(
            &world,
            "grindstone",
            click(BlockFace::Top, 0.0, NORTH, 20.0),
        );
        assert!(
            grindstone.contains("face=floor,facing=north"),
            "{grindstone}"
        );
    }

    #[test]
    fn ladders_and_tripwire_hooks_hang_on_a_wall() {
        let stone = state("stone", &[]);
        let world = on_stone();
        for item in ["ladder", "tripwire_hook"] {
            let placed = placed(&world, item, click(BlockFace::West, 0.5, EAST, 0.0));
            assert!(placed.contains("facing=west"), "{item}: {placed}");
        }
        // On the top of a block, the wall the player looks at holds it.
        let world = World::with(&[(ORIGIN, stone), (at(1, 1, 0), stone)]);
        for item in ["ladder", "tripwire_hook"] {
            let placed = placed(&world, item, click(BlockFace::Top, 0.0, EAST, 30.0));
            assert!(placed.contains("facing=west"), "{item}: {placed}");
        }
        // With no wall, it is refused.
        let world = on_stone();
        for item in ["ladder", "tripwire_hook"] {
            let outcome = place(
                block(item),
                &click(BlockFace::Top, 0.0, EAST, 30.0),
                |pos| world.get(pos),
            );
            assert_eq!(
                outcome,
                Placement::Refused {
                    target: at(0, 1, 0)
                },
                "{item}"
            );
        }
    }

    #[test]
    fn stairs_face_the_player_and_take_the_clicked_half() {
        let world = on_stone();
        for (face, cursor_y, yaw, expected) in [
            (BlockFace::Top, 0.0, SOUTH, "facing=south,half=bottom"),
            (BlockFace::Bottom, 0.0, NORTH, "facing=north,half=top"),
            (BlockFace::North, 0.75, EAST, "facing=east,half=top"),
            (BlockFace::North, 0.25, WEST, "facing=west,half=bottom"),
            (BlockFace::East, 0.5, SOUTH, "facing=south,half=bottom"),
        ] {
            let placed = placed(&world, "oak_stairs", click(face, cursor_y, yaw, 0.0));
            assert!(placed.contains(expected), "{face:?} {cursor_y}: {placed}");
            assert!(placed.contains("shape=straight,waterlogged=false"));
        }
        let copper = placed(
            &world,
            "cut_copper_stairs",
            click(BlockFace::Bottom, 0.0, EAST, 0.0),
        );
        assert!(copper.contains("facing=east,half=top"), "{copper}");
    }

    #[test]
    fn trapdoors_hinge_on_the_clicked_side_or_face_the_player() {
        let world = on_stone();
        for (face, cursor_y, yaw, expected) in [
            (BlockFace::North, 0.75, EAST, "facing=north,half=top"),
            (BlockFace::West, 0.25, EAST, "facing=west,half=bottom"),
            (BlockFace::Top, 0.0, EAST, "facing=west,half=bottom"),
            (BlockFace::Bottom, 0.0, SOUTH, "facing=north,half=top"),
        ] {
            let placed = placed(&world, "oak_trapdoor", click(face, cursor_y, yaw, 0.0));
            assert!(placed.contains(expected), "{face:?}: {placed}");
            assert!(placed.contains("open=false"));
        }
    }

    #[test]
    fn slabs_take_the_clicked_half_and_merge_into_a_double_slab() {
        let world = on_stone();
        for (face, cursor_y, expected) in [
            (BlockFace::Top, 0.0, "bottom"),
            (BlockFace::Bottom, 0.0, "top"),
            (BlockFace::North, 0.75, "top"),
            (BlockFace::North, 0.25, "bottom"),
        ] {
            let placed = placed(&world, "oak_slab", click(face, cursor_y, SOUTH, 0.0));
            assert_eq!(
                placed,
                format!("minecraft:oak_slab[type={expected},waterlogged=false]")
            );
        }

        // The top of a bottom slab, the bottom of a top slab, and the side
        // half that is empty fill the clicked slab.
        let bottom = state("oak_slab", &[("type", "bottom")]);
        let top = state("oak_slab", &[("type", "top")]);
        let double = state("oak_slab", &[("type", "double")]);
        for (clicked, face, cursor_y) in [
            (bottom, BlockFace::Top, 0.5),
            (top, BlockFace::Bottom, 0.5),
            (bottom, BlockFace::North, 0.75),
            (top, BlockFace::North, 0.25),
        ] {
            let world = World::with(&[(ORIGIN, clicked)]);
            let mut click = click(face, cursor_y, SOUTH, 0.0);
            if face == BlockFace::Top {
                click.cursor.1 = cursor_y;
            }
            let outcome = place(block("oak_slab"), &click, |pos| world.get(pos));
            assert_eq!(
                outcome,
                Placement::Place {
                    position: ORIGIN,
                    state: double
                },
                "{} on {face:?}",
                describe(clicked)
            );
        }

        // Clicking the filled half places next to it; another slab or a
        // double slab is not merged.
        let world = World::with(&[(ORIGIN, bottom)]);
        let outcome = place(
            block("oak_slab"),
            &click(BlockFace::North, 0.25, SOUTH, 0.0),
            |pos| world.get(pos),
        );
        assert!(matches!(outcome, Placement::Place { position, .. } if position == at(0, 0, -1)));
        let world = World::with(&[(ORIGIN, bottom)]);
        let outcome = place(
            block("spruce_slab"),
            &click(BlockFace::Top, 0.0, SOUTH, 0.0),
            |pos| world.get(pos),
        );
        assert!(matches!(outcome, Placement::Place { position, .. } if position == at(0, 1, 0)));

        // A slab against a block whose neighbour is a matching slab fills it.
        let world = World::with(&[(ORIGIN, state("stone", &[])), (at(0, 1, 0), top)]);
        let outcome = place(
            block("oak_slab"),
            &click(BlockFace::Top, 0.0, SOUTH, 0.0),
            |pos| world.get(pos),
        );
        assert_eq!(
            outcome,
            Placement::Place {
                position: at(0, 1, 0),
                state: double
            }
        );
    }

    #[test]
    fn blocks_outside_the_families_keep_their_default_state() {
        let world = on_stone();
        for item in [
            "oak_fence",
            "oak_door",
            "torch",
            "redstone_lamp",
            "chest",
            "oak_sign",
        ] {
            let placed = placed(&world, item, click(BlockFace::North, 0.5, EAST, 0.0));
            assert_eq!(placed, describe(block(item).default_state_id), "{item}");
        }
    }

    #[test]
    fn sturdy_faces_are_full_support_faces() {
        let stone = state("stone", &[]);
        let bottom_slab = state("oak_slab", &[("type", "bottom")]);
        for face in [BlockFace::Top, BlockFace::North, BlockFace::Bottom] {
            assert!(face_sturdy(stone, face));
            assert!(face_sturdy(state("glass", &[]), face));
            assert!(face_sturdy(state("soul_sand", &[]), face));
            assert!(!face_sturdy(state("oak_leaves", &[]), face));
            assert!(!face_sturdy(AIR, face));
        }
        assert!(face_sturdy(bottom_slab, BlockFace::Bottom));
        assert!(!face_sturdy(bottom_slab, BlockFace::Top));
        assert!(!face_sturdy(bottom_slab, BlockFace::North));
        assert!(!face_sturdy(state("oak_fence", &[]), BlockFace::Top));
        assert!(face_sturdy(
            state("snow", &[("layers", "8")]),
            BlockFace::Top
        ));
        assert!(!face_sturdy(
            state("snow", &[("layers", "7")]),
            BlockFace::Top
        ));
    }

    #[test]
    fn reach_is_the_interaction_range_plus_one_from_the_eyes() {
        let block = BlockPosition { x: 7, y: 1, z: 0 };
        // Eyes level with the block: only the X gap counts.
        assert!(!within_range((0.95, 1.62, 0.5), block, 5.0));
        assert!(within_range((1.05, 1.62, 0.5), block, 5.0));
        assert!(!within_range((1.45, 1.62, 0.5), block, 4.5));
        assert!(within_range((1.55, 1.62, 0.5), block, 4.5));
        // Inside the block, or along a diagonal.
        assert!(within_range((7.5, 1.5, 0.5), block, 4.5));
        let corner = BlockPosition { x: 3, y: 3, z: 3 };
        assert!(within_range((0.0, 1.0, 0.0), corner, 5.0));
        assert!(!within_range((-1.5, -0.5, -1.5), corner, 5.0));
    }

    #[test]
    fn only_some_entities_block_building() {
        use crate::entity::EntityKind;
        for kind in [
            EntityKind::Player,
            EntityKind::Pig,
            EntityKind::ArmorStand,
            EntityKind::Tnt,
            EntityKind::FallingBlock,
            EntityKind::Minecart,
            EntityKind::HopperMinecart,
            EntityKind::OakBoat,
            EntityKind::BambooChestRaft,
            EntityKind::EndCrystal,
        ] {
            assert!(blocks_building(kind), "{kind:?}");
        }
        for kind in [
            EntityKind::Item,
            EntityKind::ExperienceOrb,
            EntityKind::Arrow,
            EntityKind::Snowball,
            EntityKind::Marker,
            EntityKind::LightningBolt,
        ] {
            assert!(!blocks_building(kind), "{kind:?}");
        }
    }
}
