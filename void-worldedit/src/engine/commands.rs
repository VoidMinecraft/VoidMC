use std::path::PathBuf;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc::components::{PlayerDimension, Position, Rotation};
use voidmc::{
    BlockPosArg, Command, CommandBuilder, CommandContext, DimensionId, EnumArg, IntegerArg,
    Inventory, ItemBehaviorRegistry, StringArg, TextColor,
};

use super::WorldEditConfig;
use super::args::{BlocksArg, DirectionArg, DirectionChoice};
use super::queue::{AfterCopy, Edit, EditQueue, Record, reply};
use super::session::{EditSession, session_mut};
use super::tools::{BrushItems, BrushTool, Corner, describe_volume, set_corner, target_block};
use crate::brush::{Brush, BrushShape, MAX_BRUSH_RADIUS};
use crate::mask::Mask;
use crate::math::{Axis, BlockPos, Direction, WORLD_LIMIT};
use crate::operation::{Fill, Operation, Paste, Restore};
use crate::pattern::Pattern;
use crate::region::{Cuboid, Faces, Region, Walls};
use crate::schematic::Schematic;
use crate::selection::SelectionShape;

const RAYCAST_RANGE: f64 = 300.0;

type Outcome = Result<String, String>;

struct Actor {
    player: Entity,
    dimension: DimensionId,
    feet: BlockPos,
    facing: Direction,
}

fn actor(world: &World, player: Entity) -> Option<Actor> {
    let position = world.get::<Position>(player)?;
    let rotation = world.get::<Rotation>(player)?;
    Some(Actor {
        player,
        dimension: world.get::<PlayerDimension>(player)?.0,
        feet: BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        ),
        facing: Direction::facing(rotation.yaw, rotation.pitch),
    })
}

fn run(ctx: &mut CommandContext, f: impl FnOnce(&mut World, &Actor) -> Outcome) {
    let player = ctx.entity;
    let outcome = ctx.with_world_mut(|world| {
        if !world.resource::<WorldEditConfig>().allows(world, player) {
            return Err("You are not allowed to use WorldEdit.".to_string());
        }
        let actor = actor(world, player).ok_or("Only players can use this command.")?;
        f(world, &actor)
    });
    match outcome {
        Ok(message) if !message.is_empty() => {
            ctx.with_world(|world| reply(world, player, &message, TextColor::LightPurple))
        }
        Ok(_) => {}
        Err(error) => ctx.reply_error(&error),
    }
}

fn session<'w>(world: &'w mut World, actor: &Actor) -> Result<Mut<'w, EditSession>, String> {
    session_mut(world, actor.player).ok_or_else(|| "No session.".to_string())
}

fn selection_region(world: &mut World, actor: &Actor) -> Result<Arc<dyn Region>, String> {
    session(world, actor)?
        .selection
        .region()
        .map_err(|error| error.to_string())
}

fn selection_cuboid(world: &mut World, actor: &Actor) -> Result<Cuboid, String> {
    session(world, actor)?
        .selection
        .cuboid()
        .map_err(|error| error.to_string())
}

fn check_volume(world: &World, volume: u64) -> Result<(), String> {
    let limit = world.resource::<WorldEditConfig>().max_volume;
    if volume > limit {
        return Err(format!(
            "That touches {volume} blocks; the limit is {limit}."
        ));
    }
    Ok(())
}

fn not_busy(world: &World, actor: &Actor) -> Result<(), String> {
    if world.resource::<EditQueue>().is_busy(actor.player) {
        return Err("Your previous edit is still running.".to_string());
    }
    Ok(())
}

fn submit(
    world: &mut World,
    actor: &Actor,
    label: &str,
    operation: impl Operation + 'static,
) -> Outcome {
    world.resource_mut::<EditQueue>().submit(
        Edit::new(actor.dimension, operation)
            .owner(actor.player)
            .label(label),
    );
    Ok(String::new())
}

fn pattern(input: &str) -> Result<Pattern, String> {
    Pattern::parse(input).map_err(|error| format!("Invalid pattern: {error}."))
}

fn mask(input: &str) -> Result<Mask, String> {
    Mask::parse(input).map_err(|error| format!("Invalid mask: {error}."))
}

fn text(ctx: &CommandContext, name: &str) -> String {
    ctx.get::<String>(name).cloned().unwrap_or_default()
}

fn int(ctx: &CommandContext, name: &str, default: i32) -> i32 {
    ctx.get::<i32>(name).copied().unwrap_or(default)
}

fn resolve(choice: Option<DirectionChoice>, actor: &Actor) -> Direction {
    choice.unwrap_or(DirectionChoice::Me).resolve(actor.facing)
}

fn edit_command(name: &str, description: &str) -> CommandBuilder {
    CommandBuilder::new(name).description(description)
}

pub(crate) fn commands() -> Vec<Command> {
    vec![
        wand(),
        position("/pos1", Corner::First),
        position("/pos2", Corner::Second),
        look_position("/hpos1", Corner::First),
        look_position("/hpos2", Corner::Second),
        sel(),
        desel(),
        size(),
        expand(),
        contract(),
        shift(),
        set(),
        replace(),
        walls(),
        outline(),
        move_selection(),
        stack(),
        copy("/copy", AfterCopyKind::Store),
        copy("/cut", AfterCopyKind::Cut),
        paste(),
        rotate(),
        flip(),
        undo(),
        redo(),
        clear_history(),
        schematic(),
        brush(),
    ]
}

fn wand() -> Command {
    edit_command("/wand", "Get the selection wand")
        .handler(|ctx| {
            run(ctx, |world, actor| {
                let stack = world
                    .resource::<WorldEditConfig>()
                    .wand()
                    .ok_or("The configured wand item does not exist.")?;
                world
                    .get_mut::<Inventory>(actor.player)
                    .ok_or("You have no inventory.")?
                    .give(stack);
                Ok("Left click: first position. Right click: second position.".to_string())
            })
        })
        .build()
}

fn position(name: &str, corner: Corner) -> Command {
    edit_command(
        name,
        "Set a selection corner to your position or the given block",
    )
    .arg_optional("position", Arc::new(BlockPosArg))
    .handler(move |ctx| {
        let given = ctx.get::<[i32; 3]>("position").copied();
        run(ctx, |world, actor| {
            let pos = given.map_or(actor.feet, |[x, y, z]| BlockPos::new(x, y, z));
            set_corner(world, actor.player, pos, corner);
            Ok(String::new())
        })
    })
    .build()
}

fn look_position(name: &str, corner: Corner) -> Command {
    edit_command(name, "Set a selection corner to the block you look at")
        .handler(move |ctx| {
            run(ctx, |world, actor| {
                let pos =
                    target_block(world, actor.player, RAYCAST_RANGE).ok_or("No block in sight.")?;
                set_corner(world, actor.player, pos, corner);
                Ok(String::new())
            })
        })
        .build()
}

fn sel() -> Command {
    edit_command("/sel", "Choose the selection shape, or clear the selection")
        .arg_optional(
            "shape",
            EnumArg::new([
                ("cuboid", SelectionShape::Cuboid),
                ("sphere", SelectionShape::Sphere),
                ("cyl", SelectionShape::Cylinder),
            ]),
        )
        .handler(|ctx| {
            let shape = ctx.get::<SelectionShape>("shape").copied();
            run(ctx, |world, actor| {
                let mut session = session(world, actor)?;
                match shape {
                    Some(shape) => {
                        session.selection.set_shape(shape);
                        Ok(format!("Selection shape: {}.", shape.name()))
                    }
                    None => {
                        session.selection.clear();
                        Ok("Selection cleared.".to_string())
                    }
                }
            })
        })
        .build()
}

fn desel() -> Command {
    edit_command("/desel", "Clear the selection")
        .handler(|ctx| {
            run(ctx, |world, actor| {
                session(world, actor)?.selection.clear();
                Ok("Selection cleared.".to_string())
            })
        })
        .build()
}

fn size() -> Command {
    edit_command("/size", "Show the size of the selection")
        .handler(|ctx| {
            run(ctx, |world, actor| {
                let region = selection_region(world, actor)?;
                let bounds = region.bounds();
                let size = bounds.size();
                Ok(format!(
                    "{} × {} × {} ({}), from {} to {}.",
                    size.x,
                    size.y,
                    size.z,
                    describe_volume(world, region.as_ref()),
                    bounds.min,
                    bounds.max
                ))
            })
        })
        .build()
}

fn expand() -> Command {
    edit_command(
        "/expand",
        "Grow the selection towards a direction, or to full height with 'vert'",
    )
    .usage("//expand <amount|vert> [direction]")
    .arg("amount", StringArg::single_word())
    .arg_optional("direction", DirectionArg::new())
    .handler(|ctx| {
        let amount = text(ctx, "amount");
        let choice = direction_of(ctx);
        run(ctx, |world, actor| {
            let mut session = session(world, actor)?;
            if amount.eq_ignore_ascii_case("vert") {
                session
                    .selection
                    .expand_vertical(super::queue::HEIGHT)
                    .map_err(|e| e.to_string())?;
                return Ok("Selection expanded to the full world height.".to_string());
            }
            let amount: i32 = amount
                .parse()
                .ok()
                .filter(|a| *a >= 0)
                .ok_or_else(|| format!("'{amount}' is not a positive amount or 'vert'."))?;
            let direction = resolve(choice, actor);
            session
                .selection
                .expand(direction, amount)
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "Selection expanded {amount} blocks {}.",
                direction.name()
            ))
        })
    })
    .build()
}

fn contract() -> Command {
    edit_command("/contract", "Pull the face opposite a direction towards it")
        .arg("amount", IntegerArg::new(0, 1_000_000))
        .arg_optional("direction", DirectionArg::new())
        .handler(|ctx| {
            let amount = int(ctx, "amount", 1);
            let choice = direction_of(ctx);
            run(ctx, |world, actor| {
                let direction = resolve(choice, actor);
                session(world, actor)?
                    .selection
                    .contract(direction, amount)
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "Selection contracted {amount} blocks {}.",
                    direction.name()
                ))
            })
        })
        .build()
}

fn shift() -> Command {
    edit_command("/shift", "Move the selection (not the blocks)")
        .arg("amount", IntegerArg::new(0, 1_000_000))
        .arg_optional("direction", DirectionArg::new())
        .handler(|ctx| {
            let amount = int(ctx, "amount", 1);
            let choice = direction_of(ctx);
            run(ctx, |world, actor| {
                let direction = resolve(choice, actor);
                session(world, actor)?
                    .selection
                    .shift(direction.vector() * amount)
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "Selection shifted {amount} blocks {}.",
                    direction.name()
                ))
            })
        })
        .build()
}

fn fill_selection(
    world: &mut World,
    actor: &Actor,
    label: &str,
    region: Arc<dyn Region>,
    pattern: Pattern,
    mask: Mask,
) -> Outcome {
    not_busy(world, actor)?;
    check_volume(world, region.bounds().cuboid_volume())?;
    let seed = session(world, actor)?.next_seed();
    submit(
        world,
        actor,
        label,
        Fill {
            region,
            pattern: pattern.with_seed(seed),
            mask,
        },
    )
}

fn set() -> Command {
    edit_command("/set", "Fill the selection with a pattern")
        .arg("pattern", BlocksArg::new())
        .handler(|ctx| {
            let input = text(ctx, "pattern");
            run(ctx, |world, actor| {
                let pattern = pattern(&input)?;
                let region = selection_region(world, actor)?;
                fill_selection(world, actor, "Set", region, pattern, Mask::Any)
            })
        })
        .build()
}

fn replace() -> Command {
    edit_command(
        "/replace",
        "Replace blocks matching a mask (default: any non-air) with a pattern",
    )
    .usage("//replace [from] <to>")
    .arg_variadic_required("blocks", BlocksArg::new())
    .handler(|ctx| {
        let blocks = text(ctx, "blocks");
        run(ctx, |world, actor| {
            let tokens: Vec<&str> = blocks.split_whitespace().collect();
            let (mask, pattern) = match tokens.as_slice() {
                [to] => (Mask::Existing, self::pattern(to)?),
                [from, to] => (mask(from)?, self::pattern(to)?),
                _ => return Err("Usage: //replace [from] <to>".to_string()),
            };
            let region = selection_region(world, actor)?;
            fill_selection(world, actor, "Replace", region, pattern, mask)
        })
    })
    .build()
}

fn walls() -> Command {
    edit_command("/walls", "Build the four walls of the selection")
        .arg("pattern", BlocksArg::new())
        .handler(|ctx| {
            let input = text(ctx, "pattern");
            run(ctx, |world, actor| {
                let pattern = pattern(&input)?;
                let cuboid = selection_cuboid(world, actor)?;
                fill_selection(
                    world,
                    actor,
                    "Walls",
                    Arc::new(Walls(cuboid)),
                    pattern,
                    Mask::Any,
                )
            })
        })
        .build()
}

fn outline() -> Command {
    edit_command("/outline", "Cover every face of the selection")
        .alias("/faces")
        .arg("pattern", BlocksArg::new())
        .handler(|ctx| {
            let input = text(ctx, "pattern");
            run(ctx, |world, actor| {
                let pattern = pattern(&input)?;
                let cuboid = selection_cuboid(world, actor)?;
                fill_selection(
                    world,
                    actor,
                    "Outline",
                    Arc::new(Faces(cuboid)),
                    pattern,
                    Mask::Any,
                )
            })
        })
        .build()
}

fn move_selection() -> Command {
    edit_command("/move", "Move the selected blocks")
        .arg_optional("amount", IntegerArg::new(1, 1_000_000))
        .arg_optional("direction", DirectionArg::new())
        .flag("shift", Some('s'), "Move the selection along")
        .flag("skip-air", Some('a'), "Do not paste air")
        .handler(|ctx| {
            let amount = int(ctx, "amount", 1);
            let (select, skip_air) = (ctx.flag("shift"), ctx.flag("skip-air"));
            let choice = direction_of(ctx);
            run(ctx, |world, actor| {
                not_busy(world, actor)?;
                let direction = resolve(choice, actor);
                let region = selection_region(world, actor)?;
                check_volume(world, region.bounds().cuboid_volume().saturating_mul(2))?;
                let edit = Edit::copy(
                    actor.dimension,
                    region,
                    BlockPos::ZERO,
                    AfterCopy::Move {
                        offset: direction.vector() * amount,
                        skip_air,
                        select,
                    },
                );
                world
                    .resource_mut::<EditQueue>()
                    .submit(edit.owner(actor.player).label("Move"));
                Ok(String::new())
            })
        })
        .build()
}

fn direction_of(ctx: &CommandContext) -> Option<DirectionChoice> {
    ctx.get::<DirectionChoice>("direction").copied()
}

fn stack() -> Command {
    edit_command("/stack", "Repeat the selection next to itself")
        .arg_optional("count", IntegerArg::new(1, 10_000))
        .arg_optional("direction", DirectionArg::new())
        .flag("shift", Some('s'), "Move the selection to the last copy")
        .flag("skip-air", Some('a'), "Do not paste air")
        .handler(|ctx| {
            let count = int(ctx, "count", 1);
            let (select, skip_air) = (ctx.flag("shift"), ctx.flag("skip-air"));
            let choice = direction_of(ctx);
            run(ctx, |world, actor| {
                not_busy(world, actor)?;
                let direction = resolve(choice, actor);
                let region = selection_region(world, actor)?;
                let size = region.bounds().size();
                let step = match direction.axis() {
                    Axis::X => size.x,
                    Axis::Y => size.y,
                    Axis::Z => size.z,
                };
                check_volume(
                    world,
                    region.bounds().cuboid_volume().saturating_mul(count as u64),
                )?;
                if step
                    .checked_mul(count)
                    .is_none_or(|reach| reach > 2 * WORLD_LIMIT)
                {
                    return Err("That stack would leave the world.".to_string());
                }
                let offsets = (1..=count)
                    .map(|i| direction.vector() * (step * i))
                    .collect();
                let edit = Edit::copy(
                    actor.dimension,
                    region,
                    BlockPos::ZERO,
                    AfterCopy::Stack {
                        offsets,
                        skip_air,
                        select,
                    },
                );
                world
                    .resource_mut::<EditQueue>()
                    .submit(edit.owner(actor.player).label("Stack"));
                Ok(String::new())
            })
        })
        .build()
}

#[derive(Clone, Copy)]
enum AfterCopyKind {
    Store,
    Cut,
}

fn copy(name: &str, kind: AfterCopyKind) -> Command {
    let description = match kind {
        AfterCopyKind::Store => "Copy the selection to your clipboard",
        AfterCopyKind::Cut => "Copy the selection to your clipboard and clear it",
    };
    edit_command(name, description)
        .handler(move |ctx| {
            run(ctx, |world, actor| {
                not_busy(world, actor)?;
                let region = selection_region(world, actor)?;
                check_volume(world, region.bounds().cuboid_volume())?;
                let then = match kind {
                    AfterCopyKind::Store => AfterCopy::Store,
                    AfterCopyKind::Cut => AfterCopy::Cut,
                };
                let edit = Edit::copy(actor.dimension, region, actor.feet, then);
                world
                    .resource_mut::<EditQueue>()
                    .submit(edit.owner(actor.player));
                Ok(String::new())
            })
        })
        .build()
}

fn paste() -> Command {
    edit_command("/paste", "Paste your clipboard where you stand")
        .flag("skip-air", Some('a'), "Do not paste air")
        .flag("original", Some('o'), "Paste where it was copied")
        .flag("select", Some('s'), "Select the pasted region")
        .handler(|ctx| {
            let (skip_air, original, select) = (
                ctx.flag("skip-air"),
                ctx.flag("original"),
                ctx.flag("select"),
            );
            run(ctx, |world, actor| {
                not_busy(world, actor)?;
                let (clipboard, bounds, origin) = {
                    let mut session = session(world, actor)?;
                    let clipboard = session
                        .clipboard()
                        .cloned()
                        .ok_or("Your clipboard is empty: //copy something first.")?;
                    let origin = match original {
                        true => session.clipboard_origin().unwrap_or(actor.feet),
                        false => actor.feet,
                    };
                    let bounds = clipboard.bounds_at(origin);
                    if select {
                        session.selection.set_shape(SelectionShape::Cuboid);
                        session.selection.set_pos1(bounds.min);
                        session.selection.set_pos2(bounds.max);
                    }
                    (clipboard, bounds, origin)
                };
                check_volume(world, bounds.cuboid_volume())?;
                submit(
                    world,
                    actor,
                    "Paste",
                    Paste::new(clipboard, origin).skip_air(skip_air),
                )
            })
        })
        .build()
}

fn rotate() -> Command {
    edit_command(
        "/rotate",
        "Rotate your clipboard clockwise around the vertical axis",
    )
    .arg("degrees", IntegerArg::new(0, 360))
    .handler(|ctx| {
        let degrees = int(ctx, "degrees", 90);
        run(ctx, |world, actor| {
            not_busy(world, actor)?;
            if degrees % 90 != 0 {
                return Err("Rotation must be a multiple of 90 degrees.".to_string());
            }
            let mut session = session(world, actor)?;
            let clipboard = session
                .clipboard()
                .cloned()
                .ok_or("Your clipboard is empty.")?;
            let origin = session.clipboard_origin().unwrap_or(actor.feet);
            session.set_clipboard(clipboard.rotated((degrees / 90) as u8), origin);
            Ok(format!("Clipboard rotated {degrees}°."))
        })
    })
    .build()
}

fn flip() -> Command {
    edit_command("/flip", "Mirror your clipboard along a direction")
        .arg_optional("direction", DirectionArg::new())
        .handler(|ctx| {
            let choice = direction_of(ctx);
            run(ctx, |world, actor| {
                not_busy(world, actor)?;
                let direction = resolve(choice, actor);
                let mut session = session(world, actor)?;
                let clipboard = session
                    .clipboard()
                    .cloned()
                    .ok_or("Your clipboard is empty.")?;
                let origin = session.clipboard_origin().unwrap_or(actor.feet);
                session.set_clipboard(clipboard.flipped(direction.axis()), origin);
                Ok(format!("Clipboard flipped {}.", direction.name()))
            })
        })
        .build()
}

fn undo() -> Command {
    edit_command("/undo", "Undo your last edits")
        .arg_optional("times", IntegerArg::new(1, 100))
        .handler(|ctx| {
            let times = int(ctx, "times", 1);
            run(ctx, |world, actor| {
                not_busy(world, actor)?;
                let mut queued = 0;
                for _ in 0..times {
                    let Some((changes, dimension)) = session(world, actor)?.history.pop_undo()
                    else {
                        break;
                    };
                    let edit = Edit::new(dimension, Restore::undo(changes.clone()))
                        .owner(actor.player)
                        .label("Undo")
                        .recording(Record::Undo(changes));
                    world.resource_mut::<EditQueue>().submit(edit);
                    queued += 1;
                }
                if queued == 0 {
                    return Err("Nothing left to undo.".to_string());
                }
                Ok(String::new())
            })
        })
        .build()
}

fn redo() -> Command {
    edit_command("/redo", "Redo edits you undid")
        .arg_optional("times", IntegerArg::new(1, 100))
        .handler(|ctx| {
            let times = int(ctx, "times", 1);
            run(ctx, |world, actor| {
                not_busy(world, actor)?;
                let mut queued = 0;
                for _ in 0..times {
                    let Some((changes, dimension)) = session(world, actor)?.history.pop_redo()
                    else {
                        break;
                    };
                    let edit = Edit::new(dimension, Restore::redo(changes.clone()))
                        .owner(actor.player)
                        .label("Redo")
                        .recording(Record::Redo(changes));
                    world.resource_mut::<EditQueue>().submit(edit);
                    queued += 1;
                }
                if queued == 0 {
                    return Err("Nothing left to redo.".to_string());
                }
                Ok(String::new())
            })
        })
        .build()
}

fn clear_history() -> Command {
    edit_command("/clearhistory", "Forget your undo history")
        .handler(|ctx| {
            run(ctx, |world, actor| {
                session(world, actor)?.history.clear();
                Ok("History cleared.".to_string())
            })
        })
        .build()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SchematicAction {
    Save,
    Load,
    List,
    Delete,
}

fn schematic_path(world: &World, name: &str) -> Result<PathBuf, String> {
    let name = name.strip_suffix(".schem").unwrap_or(name);
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return Err("Schematic names use letters, digits, '_' and '-'.".to_string());
    }
    Ok(world
        .resource::<WorldEditConfig>()
        .schematic_dir
        .join(format!("{name}.schem")))
}

fn schematic() -> Command {
    edit_command("schem", "Save, load, list or delete Sponge schematics")
        .alias("schematic")
        .usage("/schem <save|load|list|delete> [name]")
        .arg(
            "action",
            EnumArg::new([
                ("save", SchematicAction::Save),
                ("load", SchematicAction::Load),
                ("list", SchematicAction::List),
                ("delete", SchematicAction::Delete),
            ]),
        )
        .arg_optional("name", StringArg::single_word())
        .handler(|ctx| {
            let action = ctx.get::<SchematicAction>("action").copied();
            let name = ctx.get::<String>("name").cloned();
            run(ctx, |world, actor| {
                let action = action.ok_or("Unknown action.")?;
                if action == SchematicAction::List {
                    return list_schematics(world);
                }
                not_busy(world, actor)?;
                let name = name.ok_or("Give the schematic a name.")?;
                let path = schematic_path(world, &name)?;
                match action {
                    SchematicAction::Save => {
                        let clipboard = session(world, actor)?
                            .clipboard()
                            .cloned()
                            .ok_or("Your clipboard is empty.")?;
                        if let Some(dir) = path.parent() {
                            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                        }
                        Schematic::save(&clipboard, &path).map_err(|e| e.to_string())?;
                        Ok(format!(
                            "Saved {} blocks to {name}.schem.",
                            clipboard.block_count()
                        ))
                    }
                    SchematicAction::Load => {
                        let limit = world.resource::<WorldEditConfig>().max_volume;
                        let file = std::fs::File::open(&path)
                            .map_err(|_| format!("No schematic named '{name}'."))?;
                        let loaded =
                            Schematic::read_with_limit(file, limit).map_err(|e| e.to_string())?;
                        let count = loaded.clipboard.block_count();
                        session(world, actor)?.set_clipboard(loaded.clipboard, actor.feet);
                        let mut message =
                            format!("Loaded {name}.schem ({count} blocks) into your clipboard.");
                        if !loaded.unknown_blocks.is_empty() {
                            message.push_str(&format!(
                                " Unknown blocks became air: {}.",
                                loaded.unknown_blocks.join(", ")
                            ));
                        }
                        Ok(message)
                    }
                    SchematicAction::Delete => {
                        std::fs::remove_file(&path)
                            .map_err(|_| format!("No schematic named '{name}'."))?;
                        Ok(format!("Deleted {name}.schem."))
                    }
                    SchematicAction::List => unreachable!(),
                }
            })
        })
        .build()
}

fn list_schematics(world: &World) -> Outcome {
    let dir = &world.resource::<WorldEditConfig>().schematic_dir;
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .and_then(|name| name.strip_suffix(".schem"))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    if names.is_empty() {
        return Ok("No schematics saved yet.".to_string());
    }
    Ok(format!("Schematics: {}.", names.join(", ")))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrushKind {
    Sphere,
    Cylinder,
    Smooth,
    Gravity,
    Mask,
    Range,
    None,
}

fn brush() -> Command {
    edit_command("brush", "Bind a brush to the item in your hand")
        .alias("br")
        .usage(
            "/brush sphere [-h] <pattern> [radius] | cyl [-h] <pattern> [radius] [height] | smooth [radius] [iterations] | gravity [radius] | mask <mask> | range <blocks> | none",
        )
        .arg(
            "kind",
            EnumArg::new([
                ("sphere", BrushKind::Sphere),
                ("cyl", BrushKind::Cylinder),
                ("smooth", BrushKind::Smooth),
                ("gravity", BrushKind::Gravity),
                ("mask", BrushKind::Mask),
                ("range", BrushKind::Range),
                ("none", BrushKind::None),
            ]),
        )
        .arg_variadic("options", BlocksArg::new())
        .flag("hollow", Some('h'), "Only the shell of the shape")
        .handler(|ctx| {
            let kind = ctx.get::<BrushKind>("kind").copied();
            let options: Vec<String> = text(ctx, "options")
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let hollow = ctx.flag("hollow");
            run(ctx, |world, actor| {
                configure_brush(world, actor, kind.ok_or("Unknown brush.")?, &options, hollow)
            })
        })
        .build()
}

fn number<T: std::str::FromStr>(options: &[String], index: usize, default: T) -> Result<T, String> {
    match options.get(index) {
        Some(value) => value
            .parse()
            .map_err(|_| format!("'{value}' is not a number.")),
        None => Ok(default),
    }
}

fn radius(options: &[String], index: usize) -> Result<f64, String> {
    let radius: f64 = number(options, index, 3.0)?;
    if !(0.0..=MAX_BRUSH_RADIUS).contains(&radius) {
        return Err(format!(
            "The radius must be between 0 and {MAX_BRUSH_RADIUS}."
        ));
    }
    Ok(radius)
}

fn configure_brush(
    world: &mut World,
    actor: &Actor,
    kind: BrushKind,
    options: &[String],
    hollow: bool,
) -> Outcome {
    let item = world
        .get::<Inventory>(actor.player)
        .map(|inventory| inventory.held().item)
        .filter(|item| item.0 != 0)
        .ok_or("Hold the item to bind the brush to.")?;
    let wand = world
        .resource::<WorldEditConfig>()
        .wand()
        .map(|stack| stack.item);
    if Some(item) == wand {
        return Err("The wand cannot hold a brush.".to_string());
    }
    let shape = match kind {
        BrushKind::None => {
            session(world, actor)?.unbind_brush(item);
            return Ok("Brush unbound.".to_string());
        }
        BrushKind::Range | BrushKind::Mask => {
            let mut session = session(world, actor)?;
            let brush = session
                .brush_mut(item)
                .ok_or("No brush is bound to this item.")?;
            let value = options.first().ok_or("Missing value.")?;
            return if kind == BrushKind::Range {
                brush.range = value
                    .parse::<u32>()
                    .ok()
                    .filter(|r| (1..=1000).contains(r))
                    .ok_or("The range must be between 1 and 1000.")?;
                Ok(format!("Brush range set to {}.", brush.range))
            } else {
                brush.mask = mask(value)?;
                Ok(format!("Brush mask set to {value}."))
            };
        }
        BrushKind::Sphere => BrushShape::Sphere {
            pattern: pattern(options.first().ok_or("Give the brush a pattern.")?)?,
            radius: radius(options, 1)?,
            hollow,
        },
        BrushKind::Cylinder => BrushShape::Cylinder {
            pattern: pattern(options.first().ok_or("Give the brush a pattern.")?)?,
            radius: radius(options, 1)?,
            height: number(options, 2, 1)?.clamp(1, 2 * MAX_BRUSH_RADIUS as i32),
            hollow,
        },
        BrushKind::Smooth => BrushShape::Smooth {
            radius: radius(options, 0)?,
            iterations: number(options, 1, 4u32)?.clamp(1, 32),
        },
        BrushKind::Gravity => BrushShape::Gravity {
            radius: radius(options, 0)?,
        },
    };
    let ours = world.resource::<BrushItems>().0.contains(&item);
    if !ours {
        if world.resource::<ItemBehaviorRegistry>().contains(item) {
            return Err("That item already has a behaviour; pick another one.".to_string());
        }
        world
            .resource_mut::<ItemBehaviorRegistry>()
            .register(item, BrushTool);
        world.resource_mut::<BrushItems>().0.insert(item);
    }
    let description = shape.to_string();
    session(world, actor)?.bind_brush(item, Brush::new(shape));
    Ok(format!(
        "Brush bound: {description}. Right click to use it."
    ))
}
