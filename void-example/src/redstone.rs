//! Basic redstone demo (VMC-ATP-030): `RedstonePlugin` is enabled in `main.rs`
//! and `/circuit` builds a test circuit next to the player.

use voidmc::components::{PlayerDimension, Position};
use voidmc::world::set_block_state;
use voidmc::{BlockPosition, Command, CommandBuilder, CommandContext, DimensionId};
use voidmc_data::v26_1_2::blocks;
use voidmc_data::v26_1_2::props::{East3Up, Face, Facing4, Instrument, West3Up};
use voidmc_data::v26_1_2::state::{Lever, NoteBlock, RedstoneWire, StoneButton};

pub(super) fn circuit_command() -> Command {
    CommandBuilder::new("circuit")
        .description(
            "Example command: build a lever, three redstone dust and a note block, \
             and a stone button next to a note block, east of you",
        )
        .handler(handle_circuit)
        .build()
}

fn handle_circuit(ctx: &mut CommandContext) {
    let player = ctx.entity;
    let Some((dimension, feet)) = ctx.with_world(|world| {
        let position = world.get::<Position>(player)?;
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
        ))
    }) else {
        ctx.reply_error("You have no position.");
        return;
    };

    let origin = BlockPosition {
        x: feet.x + 2,
        ..feet
    };
    let blocks = circuit_blocks(origin);
    ctx.with_world_mut(|world| {
        for (position, state) in blocks {
            set_block_state(world, dimension, position, state, None);
        }
    });
    ctx.reply(&format!(
        "Circuit built at {} {} {}: right-click the lever (west end of the dust) \
         or the stone button two blocks south of it.",
        origin.x, origin.y, origin.z
    ));
}

/// Lever, three dust and a note block along +x at `origin`; two blocks
/// south, a stone button next to a second note block. Everything stands on
/// stone with air above.
fn circuit_blocks(origin: BlockPosition) -> Vec<(BlockPosition, i32)> {
    let at = |dx: i32, dy: i16, dz: i32| BlockPosition {
        x: origin.x + dx,
        y: origin.y + dy,
        z: origin.z + dz,
    };
    let lever = Lever {
        face: Face::Floor,
        facing: Facing4::East,
        powered: false,
    };
    let dust = RedstoneWire {
        east: East3Up::Side,
        west: West3Up::Side,
        ..RedstoneWire::DEFAULT
    };
    let note = |note| NoteBlock {
        instrument: Instrument::Harp,
        note,
        powered: false,
    };
    let button = StoneButton {
        face: Face::Floor,
        facing: Facing4::East,
        powered: false,
    };

    let mut blocks = Vec::new();
    for dz in 0..=2 {
        for dx in -1..=5 {
            blocks.push((at(dx, -1, dz), blocks::STONE));
            blocks.push((at(dx, 0, dz), blocks::AIR));
            blocks.push((at(dx, 1, dz), blocks::AIR));
        }
    }
    blocks.push((at(0, 0, 0), lever.to_state_id()));
    for dx in 1..=3 {
        blocks.push((at(dx, 0, 0), dust.to_state_id()));
    }
    blocks.push((at(4, 0, 0), note(12).to_state_id()));
    blocks.push((at(0, 0, 2), button.to_state_id()));
    blocks.push((at(1, 0, 2), note(18).to_state_id()));
    blocks
}
