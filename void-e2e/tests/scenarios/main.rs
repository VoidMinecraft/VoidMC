mod commands;
mod connection;
mod inventory;
mod players;
mod world;

use voidmc_e2e::Bot;
use voidmc_e2e::core::position::BlockPos;

fn feet(bot: &Bot) -> BlockPos {
    BlockPos::from(bot.position())
}

fn ground(bot: &Bot) -> BlockPos {
    let feet = feet(bot);
    bot.view(|view| view.ground_below(feet))
        .unwrap_or_else(|| panic!("{} holds no chunk under its feet", bot.name()))
}
