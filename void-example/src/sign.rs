use std::sync::Arc;

use voidmc::components::{PlayerDimension, Position, Rotation};
use voidmc::world::{BlockMutation, mutate_block};
use voidmc::{
    BlockFace, BlockPosition, Command, CommandBuilder, CommandContext, DimensionId, DyeColor,
    GreedyStringArg, Sign, SignSide, set_block_entity,
};
use voidmc_data::v26_1_2::state::OakSign;

pub(super) fn sign_command() -> Command {
    CommandBuilder::new("sign")
        .description("Example command: place a sign at your feet reading the given text")
        .arg_variadic_required("text", Arc::new(GreedyStringArg))
        .flag("glow", Some('g'), "Glowing red text")
        .handler(handle_sign)
        .build()
}

fn handle_sign(ctx: &mut CommandContext) {
    let text = ctx.get::<String>("text").cloned().unwrap_or_default();
    let glow = ctx.flag("glow");
    let player = ctx.entity;

    let Some((dimension, position, yaw)) = ctx.with_world(|world| {
        let position = world.get::<Position>(player)?;
        let yaw = world.get::<Rotation>(player).map(|r| r.yaw).unwrap_or(0.0);
        let dimension = world
            .get::<PlayerDimension>(player)
            .map(|d| d.0)
            .unwrap_or(DimensionId::Overworld);
        Some((
            dimension,
            BlockPosition {
                x: position.x.floor() as i32,
                y: position.y.floor() as i16,
                z: position.z.floor() as i32,
            },
            yaw,
        ))
    }) else {
        ctx.reply_error("You have no position.");
        return;
    };

    let mut lines = [String::new(), String::new(), String::new(), String::new()];
    for (line, words) in lines.iter_mut().zip(wrap_words(&text, 15)) {
        *line = words;
    }
    let mut side = SignSide::lines(lines);
    if glow {
        side = side.color(DyeColor::Red).glowing();
    }
    let sign = Sign::new().front(side).waxed();

    let state = OakSign {
        rotation: ((yaw + 180.0) / 22.5 + 0.5).floor() as u8 & 15,
        waterlogged: false,
    }
    .to_state_id();

    let result = ctx.with_world_mut(|world| {
        mutate_block(
            world,
            player,
            dimension,
            position,
            state,
            BlockFace::Top,
            BlockMutation::Place,
        );
        set_block_entity(world, dimension, position, sign)
    });

    match result {
        Ok(_) => ctx.reply(&format!(
            "Sign placed at {} {} {}.",
            position.x, position.y, position.z
        )),
        Err(err) => ctx.reply_error(&format!("Could not place the sign: {err}")),
    }
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.len() + 1 + word.len() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}
