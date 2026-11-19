mod atp_002_server_status;
mod atp_003_login_handshake;
mod atp_004_initial_world;
mod atp_005_two_player_sync;
mod atp_006_shared_blocks;
mod atp_008_discover_commands;
mod atp_009_world_restart;
mod atp_010_gamemode;
mod atp_013_give_and_clear;
mod atp_016_summon;
mod atp_017_wander;
mod atp_018_physics;
mod atp_020_world_loader;
mod atp_021_custom_command;
mod atp_022_item_behavior;
mod atp_023_plugin_hook;
mod atp_026_graceful_stop;
mod atp_027_world_generation;
mod atp_029_biomes_weather;
mod atp_030_note_block_redstone;
mod atp_031_load;
mod atp_035_gameplay_events;
mod atp_036_custom_world_generator;
mod atp_037_custom_biome;
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
