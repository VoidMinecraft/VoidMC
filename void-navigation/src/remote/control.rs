use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc::components::{
    EntityCollider, EntityDimension, MinecraftEntityId, PlayerDimension, Position, Rotation,
};
use voidmc::events::PlayerInteractEntityEvent;
use voidmc::item_behavior::{ItemBehavior, ItemUseContext, UseResult};
use voidmc::world::{ChunkData, ChunkIndex, ChunkPos, DimensionId, is_solid_block_state};
use voidmc::{
    Command, CommandBuilder, CommandContext, EnumArg, Inventory, ItemStack, TextColor, Vec3Arg,
    WorldMessages,
};

use super::{NavRemote, RemoteSettings, select};
use crate::adapter::{Goal, NavigationStats, NavigationWorld, Navigator};
use crate::pathing::{BlockPos, Vec3};

const EYE_HEIGHT: f64 = 1.62;
const PICK_RANGE: f64 = 64.0;
const CLICK_RANGE: f64 = 128.0;
const GROUND_SCAN: i32 = 24;
const FOLLOW_DISTANCE: f64 = 2.5;

struct Ray {
    origin: Vec3,
    direction: Vec3,
}

fn view_ray(world: &World, player: Entity) -> Option<(Ray, DimensionId)> {
    let position = world.get::<Position>(player)?;
    let rotation = world.get::<Rotation>(player)?;
    let dimension = world
        .get::<PlayerDimension>(player)
        .map_or(DimensionId::Overworld, |d| d.0);
    let (yaw, pitch) = (
        (rotation.yaw as f64).to_radians(),
        (rotation.pitch as f64).to_radians(),
    );
    let direction = Vec3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    );
    let origin = Vec3::new(position.x, position.y + EYE_HEIGHT, position.z);
    Some((Ray { origin, direction }, dimension))
}

fn solid_at(world: &World, dimension: DimensionId, pos: BlockPos) -> Option<bool> {
    let chunk = ChunkPos::new(pos.x.div_euclid(16), pos.z.div_euclid(16));
    let entity = *world.resource::<ChunkIndex>().0.get(&(dimension, chunk))?;
    let data = world.get::<ChunkData>(entity)?;
    let state = data
        .get_block(
            pos.x.rem_euclid(16) as u8,
            pos.y,
            pos.z.rem_euclid(16) as u8,
        )
        .unwrap_or(0);
    Some(is_solid_block_state(state))
}

enum Hit {
    Block { distance: f64, before: BlockPos },
    Unloaded,
    Miss,
}

/// The first solid block along the ray (voxel traversal), with the empty cell
/// the ray entered it from.
fn block_hit(world: &World, dimension: DimensionId, ray: &Ray, range: f64) -> Hit {
    let mut cell = BlockPos::containing(ray.origin);
    let step = |d: f64| if d > 0.0 { 1 } else { -1 };
    let (sx, sy, sz) = (
        step(ray.direction.x),
        step(ray.direction.y),
        step(ray.direction.z),
    );
    let boundary = |o: f64, c: i32, s: i32| {
        if s > 0 {
            c as f64 + 1.0 - o
        } else {
            o - c as f64
        }
    };
    let delta = |d: f64| {
        if d == 0.0 {
            f64::INFINITY
        } else {
            1.0 / d.abs()
        }
    };
    let (dx, dy, dz) = (
        delta(ray.direction.x),
        delta(ray.direction.y),
        delta(ray.direction.z),
    );
    let mut tx = boundary(ray.origin.x, cell.x, sx) * dx;
    let mut ty = boundary(ray.origin.y, cell.y, sy) * dy;
    let mut tz = boundary(ray.origin.z, cell.z, sz) * dz;
    let mut t = 0.0;
    while t <= range {
        let previous = cell;
        if tx <= ty && tx <= tz {
            cell.x += sx;
            t = tx;
            tx += dx;
        } else if ty <= tz {
            cell.y += sy;
            t = ty;
            ty += dy;
        } else {
            cell.z += sz;
            t = tz;
            tz += dz;
        }
        match solid_at(world, dimension, cell) {
            Some(true) => {
                return Hit::Block {
                    distance: t,
                    before: previous,
                };
            }
            Some(false) => {}
            None => return Hit::Unloaded,
        }
    }
    Hit::Miss
}

fn aabb_hit(ray: &Ray, min: Vec3, max: Vec3) -> Option<f64> {
    let mut near = 0.0f64;
    let mut far = f64::INFINITY;
    for (o, d, lo, hi) in [
        (ray.origin.x, ray.direction.x, min.x, max.x),
        (ray.origin.y, ray.direction.y, min.y, max.y),
        (ray.origin.z, ray.direction.z, min.z, max.z),
    ] {
        if d.abs() < 1.0e-12 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo - o) / d, (hi - o) / d);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if near > far {
            return None;
        }
    }
    Some(near)
}

/// The navigating mob `player` is looking at, if no block is in the way.
pub fn select_looked_at(world: &mut World, player: Entity) -> Option<Entity> {
    let (ray, dimension) = view_ray(world, player)?;
    let wall = match block_hit(world, dimension, &ray, PICK_RANGE) {
        Hit::Block { distance, .. } => distance,
        Hit::Unloaded | Hit::Miss => PICK_RANGE,
    };
    let mut query = world.query_filtered::<(
        Entity,
        &Position,
        Option<&EntityCollider>,
        Option<&EntityDimension>,
    ), With<Navigator>>();
    query
        .iter(world)
        .filter(|(_, _, _, d)| d.map_or(DimensionId::Overworld, |d| d.0) == dimension)
        .filter_map(|(entity, position, collider, _)| {
            let collider = collider.copied().unwrap_or_default();
            let half = collider.half_width.max(0.3);
            let min = Vec3::new(position.x - half, position.y, position.z - half);
            let max = Vec3::new(
                position.x + half,
                position.y + collider.height,
                position.z + half,
            );
            aabb_hit(&ray, min, max)
                .filter(|&t| t <= wall)
                .map(|t| (entity, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(entity, _)| entity)
}

fn standing_spot(world: &World, dimension: DimensionId, cell: BlockPos) -> Vec3 {
    let mut pos = cell;
    for _ in 0..GROUND_SCAN {
        if solid_at(world, dimension, pos.offset(0, -1, 0)) != Some(false) {
            break;
        }
        pos = pos.offset(0, -1, 0);
    }
    pos.bottom_center()
}

enum Clicked {
    Spot(Vec3),
    Unloaded,
    Sky,
}

fn clicked_destination(world: &World, player: Entity) -> Clicked {
    let Some((ray, dimension)) = view_ray(world, player) else {
        return Clicked::Sky;
    };
    match block_hit(world, dimension, &ray, CLICK_RANGE) {
        Hit::Block { before, .. } => Clicked::Spot(standing_spot(world, dimension, before)),
        Hit::Unloaded => Clicked::Unloaded,
        Hit::Miss => Clicked::Sky,
    }
}

fn claim_use(world: &mut World, player: Entity) -> bool {
    world
        .get_mut::<NavRemote>(player)
        .is_none_or(|mut remote| remote.claim_use())
}

fn selected(world: &World, player: Entity) -> Option<Entity> {
    world.get::<NavRemote>(player)?.selected()
}

fn allowed(world: &World, player: Entity) -> bool {
    world
        .get_resource::<RemoteSettings>()
        .is_some_and(|settings| settings.allows(world, player))
}

fn drive(world: &mut World, player: Entity, target: Vec3) -> Option<Entity> {
    let mob = selected(world, player)?;
    world.get_mut::<Navigator>(mob)?.move_to(target);
    Some(mob)
}

fn describe(target: Vec3) -> String {
    format!("{:.1} {:.1} {:.1}", target.x, target.y, target.z)
}

fn pick(world: &mut World, player: Entity) -> Option<Entity> {
    let mob = select_looked_at(world, player)?;
    select(world, player, Some(mob));
    Some(mob)
}

/// The remote's control item: right-click a mob to select it, a block (or a
/// distant block in view) to send it there, the sky to release it.
pub(crate) struct RemoteWand;

impl ItemBehavior for RemoteWand {
    fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
        let player = ctx.player;
        if !ctx.with_world(|world| allowed(world, player)) {
            return UseResult::Pass;
        }
        let Some(block) = ctx.block else {
            return UseResult::Pass;
        };
        let reply = ctx.with_world_mut(|world| {
            if !claim_use(world, player) {
                return None;
            }
            if selected(world, player).is_none() {
                let reply = match pick(world, player) {
                    Some(_) => "Selected the mob you are looking at.",
                    None => "Right-click a mob with the remote to select it.",
                };
                claim_use(world, player);
                return Some(reply.to_string());
            }
            let dimension = world
                .get::<PlayerDimension>(player)
                .map_or(DimensionId::Overworld, |d| d.0);
            let face = voidmc::world::offset_position(block.position, block.face);
            let target = standing_spot(
                world,
                dimension,
                BlockPos::new(face.x, face.y as i32, face.z),
            );
            Some(match drive(world, player, target) {
                Some(_) => format!("Moving to {}.", describe(target)),
                None => "The selected mob is gone.".to_string(),
            })
        });
        if let Some(reply) = reply {
            ctx.reply(&reply);
        }
        UseResult::Handled
    }

    fn on_use(&self, ctx: &mut ItemUseContext) -> UseResult {
        let player = ctx.player;
        if !ctx.with_world(|world| allowed(world, player)) {
            return UseResult::Pass;
        }
        let reply = ctx.with_world_mut(|world| {
            if !claim_use(world, player) {
                return None;
            }
            let current = selected(world, player);
            if let Some(mob) = select_looked_at(world, player) {
                if current == Some(mob) {
                    return None;
                }
                select(world, player, Some(mob));
                return Some("Selected. Right-click a block to send it there.".to_string());
            }
            let mob = current?;
            match clicked_destination(world, player) {
                Clicked::Spot(target) => {
                    let already = world
                        .get::<Navigator>(mob)
                        .and_then(Navigator::destination)
                        .is_some_and(|goal| goal.distance(target) < 1.0e-6);
                    if already {
                        return None;
                    }
                    drive(world, player, target);
                    Some(format!("Moving to {}.", describe(target)))
                }
                Clicked::Unloaded => Some("That spot is not loaded.".to_string()),
                Clicked::Sky => {
                    select(world, player, None);
                    Some("Released the mob.".to_string())
                }
            }
        });
        if let Some(reply) = reply {
            ctx.reply(&reply);
        }
        UseResult::Handled
    }
}

pub(crate) fn select_on_interact(
    event: On<PlayerInteractEntityEvent>,
    settings: Res<RemoteSettings>,
    inventories: Query<&Inventory>,
    mobs: Query<(Entity, &MinecraftEntityId), With<Navigator>>,
    mut commands: Commands,
) {
    if event.attack {
        return;
    }
    let player = event.entity;
    let holding = inventories.get(player).is_ok_and(|inventory| {
        inventory
            .get(Inventory::hotbar_slot_index(inventory.selected_hotbar()))
            .item
            == settings.wand
    });
    if !holding {
        return;
    }
    let Some((mob, _)) = mobs.iter().find(|(_, id)| id.0 == event.target_id) else {
        return;
    };
    commands.queue(move |world: &mut World| {
        if allowed(world, player) && selected(world, player) != Some(mob) {
            select(world, player, Some(mob));
            WorldMessages::new(world)
                .message(player, "Selected. Right-click a block to send it there.")
                .color(TextColor::Green)
                .send();
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Wand,
    Select,
    Deselect,
    Goto,
    Here,
    Follow,
    Stop,
    Pause,
    Resume,
    Info,
}

/// `/nav <action> [x y z]`: give the remote, select/release a mob, drive it
/// (`goto`, `here`, `follow`, `stop`, `pause`, `resume`) or inspect it.
pub fn nav_command() -> Command {
    CommandBuilder::new("nav")
        .description("Select, drive and inspect navigating mobs")
        .usage("/nav <wand|select|deselect|goto|here|follow|stop|pause|resume|info> [x y z]")
        .arg(
            "action",
            EnumArg::new([
                ("wand", Action::Wand),
                ("select", Action::Select),
                ("deselect", Action::Deselect),
                ("goto", Action::Goto),
                ("here", Action::Here),
                ("follow", Action::Follow),
                ("stop", Action::Stop),
                ("pause", Action::Pause),
                ("resume", Action::Resume),
                ("info", Action::Info),
            ]),
        )
        .arg_optional("position", Arc::new(Vec3Arg))
        .handler(handle_nav)
        .build()
}

fn handle_nav(ctx: &mut CommandContext) {
    let player = ctx.entity;
    if !ctx.with_world(|world| allowed(world, player)) {
        ctx.reply_error("The navigation remote is reserved to operators.");
        return;
    }
    let Some(&action) = ctx.get::<Action>("action") else {
        return;
    };
    let position = ctx.get::<[f64; 3]>("position").copied();
    let result = ctx.with_world_mut(|world| run(world, player, action, position));
    match result {
        Ok(message) => ctx.reply(&message),
        Err(message) => ctx.reply_error(&message),
    }
}

fn run(
    world: &mut World,
    player: Entity,
    action: Action,
    position: Option<[f64; 3]>,
) -> Result<String, String> {
    match action {
        Action::Wand => {
            let wand = world.resource::<RemoteSettings>().wand;
            let mut inventory = world
                .get_mut::<Inventory>(player)
                .ok_or("You have no inventory.")?;
            inventory.give(ItemStack::new(wand, 1));
            return Ok("Right-click a mob to select it, then a block to send it there.".into());
        }
        Action::Select => {
            let mob = pick(world, player)
                .or_else(|| nearest_navigator(world, player))
                .ok_or("No navigating mob in sight or within 16 blocks.")?;
            return Ok(format!("Selected {mob}."));
        }
        Action::Deselect => {
            select(world, player, None);
            return Ok("Released the mob.".into());
        }
        Action::Info => return Ok(info(world, player)),
        _ => {}
    }
    let mob = selected(world, player).ok_or("Select a mob first: /nav select.")?;
    let here = world
        .get::<Position>(player)
        .map(|p| Vec3::new(p.x, p.y, p.z))
        .ok_or("You have no position.")?;
    let mut navigator = world
        .get_mut::<Navigator>(mob)
        .ok_or("The selected mob is gone.")?;
    Ok(match action {
        Action::Goto => {
            let target: Vec3 = position.ok_or("Usage: /nav goto <x y z>")?.into();
            navigator.move_to(target);
            format!("Moving to {}.", describe(target))
        }
        Action::Here => {
            navigator.move_to(here);
            "Coming to you.".into()
        }
        Action::Follow => {
            navigator.set_goal(Goal::follow(player, FOLLOW_DISTANCE));
            "Following you.".into()
        }
        Action::Stop => {
            navigator.stop();
            "Stopped.".into()
        }
        Action::Pause => {
            navigator.pause();
            "Paused.".into()
        }
        Action::Resume => {
            navigator.resume();
            "Resumed.".into()
        }
        _ => unreachable!(),
    })
}

fn nearest_navigator(world: &mut World, player: Entity) -> Option<Entity> {
    let here = *world.get::<Position>(player)?;
    let mut query = world.query_filtered::<(Entity, &Position), With<Navigator>>();
    let mob = query
        .iter(world)
        .map(|(entity, p)| {
            let d = (p.x - here.x).powi(2) + (p.y - here.y).powi(2) + (p.z - here.z).powi(2);
            (entity, d)
        })
        .filter(|&(_, d)| d <= 16.0 * 16.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))?
        .0;
    select(world, player, Some(mob));
    Some(mob)
}

fn info(world: &World, player: Entity) -> String {
    let stats = *world.resource::<NavigationStats>();
    let nav_world = world.resource::<NavigationWorld>();
    let mut lines = vec![format!(
        "searches {} (complete {}, partial {}, unreachable {}), queue {}, moving {}, last tick {} nodes / {} µs, cache {} sections ({} KiB)",
        stats.searches,
        stats.complete,
        stats.partial,
        stats.unreachable,
        stats.queued,
        stats.moving,
        stats.expanded_last_tick,
        stats.planning_micros_last_tick,
        nav_world.cached_sections(),
        nav_world.memory_bytes() / 1024,
    )];
    if let Some(mob) = selected(world, player)
        && let Some(navigator) = world.get::<Navigator>(mob)
    {
        lines.push(format!(
            "{mob}: goal {:?}, {} waypoints (at {}), {}{}",
            navigator.goal(),
            navigator.path().len(),
            navigator.waypoint(),
            if navigator.path().is_complete() {
                "complete"
            } else {
                "partial"
            },
            if navigator.is_paused() {
                ", paused"
            } else {
                ""
            },
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use voidmc::item::ItemId;

    use super::*;
    use crate::remote::RemoteAccess;

    fn world_with_mob() -> (World, Entity, Entity) {
        let mut world = World::new();
        world.insert_resource(RemoteSettings {
            access: RemoteAccess::Operators,
            wand: ItemId::from_name("minecraft:blaze_rod").expect("item"),
        });
        world.init_resource::<ChunkIndex>();
        let mob = world
            .spawn((
                Position {
                    x: 3.0,
                    y: 64.0,
                    z: 0.0,
                },
                Navigator::default(),
            ))
            .id();
        let player = world
            .spawn((
                Position {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
                Rotation {
                    yaw: 270.0,
                    pitch: 0.0,
                },
            ))
            .id();
        (world, mob, player)
    }

    #[test]
    fn commands_drive_the_selected_mob() {
        let (mut world, mob, player) = world_with_mob();
        assert!(!allowed(&world, player));
        world
            .entity_mut(player)
            .insert(voidmc::components::Operator);
        assert!(allowed(&world, player));
        assert!(run(&mut world, player, Action::Stop, None).is_err());
        assert_eq!(select_looked_at(&mut world, player), Some(mob));
        run(&mut world, player, Action::Select, None).expect("select");
        assert_eq!(selected(&world, player), Some(mob));
        run(&mut world, player, Action::Goto, Some([10.5, 64.0, 2.5])).expect("goto");
        assert_eq!(
            world.get::<Navigator>(mob).and_then(Navigator::destination),
            Some(Vec3::new(10.5, 64.0, 2.5))
        );
        run(&mut world, player, Action::Follow, None).expect("follow");
        assert_eq!(
            world.get::<Navigator>(mob).and_then(Navigator::goal),
            Some(&Goal::follow(player, FOLLOW_DISTANCE))
        );
        run(&mut world, player, Action::Deselect, None).expect("deselect");
        assert_eq!(selected(&world, player), None);
    }

    #[test]
    fn ray_hits_boxes_in_front_only() {
        let ray = Ray {
            origin: Vec3::new(0.0, 1.0, 0.0),
            direction: Vec3::new(0.0, 0.0, 1.0),
        };
        let hit = aabb_hit(&ray, Vec3::new(-0.5, 0.0, 4.0), Vec3::new(0.5, 2.0, 5.0));
        assert_eq!(hit, Some(4.0));
        assert_eq!(
            aabb_hit(&ray, Vec3::new(-0.5, 0.0, -5.0), Vec3::new(0.5, 2.0, -4.0)),
            None
        );
        assert_eq!(
            aabb_hit(&ray, Vec3::new(2.0, 0.0, 4.0), Vec3::new(3.0, 2.0, 5.0)),
            None
        );
    }
}
