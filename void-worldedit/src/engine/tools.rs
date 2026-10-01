use std::collections::HashSet;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc::components::{PlayerDimension, Position, Rotation};
use voidmc::item_behavior::{BlockBreakContext, ItemBehavior, ItemUseContext, UseResult};
use voidmc::{Hand, Inventory, ItemId, Particle, ParticleColor, TextColor, WorldParticles};

use super::WorldEditConfig;
use super::extent::ChunkExtent;
use super::queue::{Edit, EditQueue, reply};
use super::session::{EditSession, session_mut};
use crate::brush::raycast;
use crate::math::{BlockPos, Vec3};
use crate::region::Region;

const EYE_HEIGHT: f64 = 1.62;
const PREVIEW_INTERVAL: u32 = 4;

/// Items this plugin registered a brush behaviour for, so binding never
/// replaces another plugin's behaviour.
#[derive(Resource, Default)]
pub(crate) struct BrushItems(pub HashSet<ItemId>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Corner {
    First,
    Second,
}

pub(crate) fn set_corner(world: &mut World, player: Entity, pos: BlockPos, corner: Corner) -> bool {
    if !world.resource::<WorldEditConfig>().allows(world, player) {
        return false;
    }
    let Some(mut session) = session_mut(world, player) else {
        return false;
    };
    let previous = match corner {
        Corner::First => session.selection.pos1(),
        Corner::Second => session.selection.pos2(),
    };
    match corner {
        Corner::First => session.selection.set_pos1(pos),
        Corner::Second => session.selection.set_pos2(pos),
    }
    let current = match corner {
        Corner::First => session.selection.pos1(),
        Corner::Second => session.selection.pos2(),
    };
    if previous == current {
        return true;
    }
    let region = session.selection.region().ok();
    let pos = current.unwrap_or(pos);
    let name = match corner {
        Corner::First => "First",
        Corner::Second => "Second",
    };
    let message = match region {
        Some(region) => format!(
            "{name} position set to {pos} ({}).",
            describe_volume(world, region.as_ref())
        ),
        None => format!("{name} position set to {pos}."),
    };
    reply(world, player, &message, TextColor::LightPurple);
    true
}

/// The exact block count when the bounds are within `max_volume`, else the
/// bounding box count: an exact count walks every row of the shape.
pub(crate) fn describe_volume(world: &World, region: &dyn Region) -> String {
    let bounds = region.bounds().cuboid_volume();
    if bounds <= world.resource::<WorldEditConfig>().max_volume {
        format!("{} blocks", region.volume())
    } else {
        format!("{bounds} blocks in its bounds, over the edit limit")
    }
}

/// Left click sets the first corner (the block is kept), right click the second.
pub(crate) struct Wand;

impl ItemBehavior for Wand {
    fn before_break_block(&self, ctx: &mut BlockBreakContext) -> UseResult {
        let (player, position) = (ctx.player, ctx.position);
        let pos = BlockPos::new(position.x, i32::from(position.y), position.z);
        handled(ctx.with_world_mut(|world| set_corner(world, player, pos, Corner::First)))
    }

    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        let Some(target) = ctx.block else {
            return UseResult::Pass;
        };
        if ctx.hand != Hand::MainHand {
            return UseResult::Handled;
        }
        let player = ctx.player;
        let pos = BlockPos::new(
            target.position.x,
            i32::from(target.position.y),
            target.position.z,
        );
        handled(ctx.with_world_mut(|world| set_corner(world, player, pos, Corner::Second)))
    }
}

fn handled(done: bool) -> UseResult {
    if done {
        UseResult::Handled
    } else {
        UseResult::Pass
    }
}

pub(crate) struct BrushTool;

impl ItemBehavior for BrushTool {
    fn on_use(&self, ctx: &mut ItemUseContext) -> UseResult {
        use_brush(ctx)
    }

    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        use_brush(ctx)
    }
}

fn use_brush(ctx: &mut ItemUseContext) -> UseResult {
    let (player, item, hand) = (ctx.player, ctx.held.item, ctx.hand);
    ctx.with_world_mut(|world| {
        let has_brush = world
            .get::<EditSession>(player)
            .is_some_and(|session| session.brush(item).is_some());
        if !has_brush || !world.resource::<WorldEditConfig>().allows(world, player) {
            return UseResult::Pass;
        }
        if hand == Hand::MainHand {
            fire_brush(world, player, item);
        }
        UseResult::Handled
    })
}

pub(crate) struct Eye {
    pub dimension: voidmc::DimensionId,
    pub origin: Vec3,
    pub look: Vec3,
}

pub(crate) fn eye(world: &World, player: Entity) -> Option<Eye> {
    let position = world.get::<Position>(player)?;
    let rotation = world.get::<Rotation>(player)?;
    Some(Eye {
        dimension: world.get::<PlayerDimension>(player)?.0,
        origin: Vec3::new(position.x, position.y + EYE_HEIGHT, position.z),
        look: Vec3::from_rotation(rotation.yaw, rotation.pitch),
    })
}

pub(crate) fn target_block(world: &mut World, player: Entity, range: f64) -> Option<BlockPos> {
    let eye = eye(world, player)?;
    let extent = ChunkExtent::new(world, eye.dimension);
    raycast(&extent, eye.origin, eye.look, range)
}

fn fire_brush(world: &mut World, player: Entity, item: ItemId) {
    if world.resource::<EditQueue>().is_busy(player) {
        return;
    }
    let Some(eye) = eye(world, player) else {
        return;
    };
    let Some(mut session) = session_mut(world, player) else {
        return;
    };
    let Some(brush) = session.brush(item).cloned() else {
        return;
    };
    let seed = session.next_seed();
    let extent = ChunkExtent::new(world, eye.dimension);
    let Some(target) = raycast(&extent, eye.origin, eye.look, f64::from(brush.range)) else {
        drop(extent);
        reply(world, player, "No block in sight.", TextColor::Red);
        return;
    };
    let operation: Arc<dyn crate::operation::Operation> =
        Arc::from(brush.operation(&extent, target, seed));
    drop(extent);
    world.resource_mut::<EditQueue>().submit(
        Edit::from_arc(eye.dimension, operation)
            .owner(player)
            .label("Brush"),
    );
    spawn_ring(
        world,
        player,
        eye.dimension,
        target,
        brush.radius(),
        ParticleColor::rgb(255, 170, 0),
    );
}

fn spawn_ring(
    world: &World,
    player: Entity,
    dimension: voidmc::DimensionId,
    target: BlockPos,
    radius: f64,
    color: ParticleColor,
) {
    let center = target.center();
    let points = ((radius * 6.0) as usize).clamp(8, 32);
    let particles = WorldParticles::new(world);
    for index in 0..points {
        let angle = index as f64 / points as f64 * std::f64::consts::TAU;
        let r = radius.max(0.5);
        particles
            .spawn(Particle::Dust { color, scale: 0.8 })
            .at([
                center.x + r * angle.cos(),
                center.y + 0.6,
                center.z + r * angle.sin(),
            ])
            .dimension(dimension)
            .viewers([player])
            .send();
    }
}

/// Every few ticks, outlines where each player's held brush would land.
pub(crate) fn brush_preview(world: &mut World, mut tick: Local<u32>) {
    *tick = tick.wrapping_add(1);
    if !tick.is_multiple_of(PREVIEW_INTERVAL) || !world.resource::<WorldEditConfig>().brush_preview
    {
        return;
    }
    let mut holders = Vec::new();
    let mut query = world.query::<(Entity, &EditSession, &Inventory)>();
    for (player, session, inventory) in query.iter(world) {
        if !session.has_brushes() {
            continue;
        }
        if let Some(brush) = session.brush(inventory.held().item) {
            holders.push((player, brush.range, brush.radius()));
        }
    }
    for (player, range, radius) in holders {
        let Some(dimension) = world.get::<PlayerDimension>(player).map(|d| d.0) else {
            continue;
        };
        if let Some(target) = target_block(world, player, f64::from(range)) {
            spawn_ring(
                world,
                player,
                dimension,
                target,
                radius,
                ParticleColor::rgb(255, 85, 255),
            );
        }
    }
}
