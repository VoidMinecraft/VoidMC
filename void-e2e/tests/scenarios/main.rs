mod atp_002_server_status;
mod atp_003_login_handshake;
mod atp_004_initial_world;
mod atp_005_two_player_sync;
mod atp_006_shared_blocks;
mod atp_007_chat;
mod atp_008_discover_commands;
mod atp_009_world_restart;
mod atp_010_gamemode;
mod atp_011_teleport;
mod atp_012_manage_players;
mod atp_013_give_and_clear;
mod atp_014_inventory_clicks;
mod atp_015_item_drop;
mod atp_016_summon;
mod atp_017_wander;
mod atp_018_physics;
mod atp_019_chunk_streaming;
mod atp_020_world_loader;
mod atp_021_custom_command;
mod atp_022_item_behavior;
mod atp_023_plugin_hook;
mod atp_024_server_config;
mod atp_025_tps_diagnostics;
mod atp_026_graceful_stop;
mod atp_027_world_generation;
mod atp_028_parallel_regions;
mod atp_029_biomes_weather;
mod atp_030_note_block_redstone;
mod atp_031_load;
mod atp_032_observability;
mod atp_033_command_completion;
mod atp_034_lifecycle_plugin;
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
use voidmc_e2e::inventory::ItemStack;
use voidmc_e2e::inventory::components::EquipmentSlot;
use voidmc_e2e::registry::builtin::ItemKind;

fn feet(bot: &Bot) -> BlockPos {
    BlockPos::from(bot.position())
}

fn ground(bot: &Bot) -> BlockPos {
    let feet = feet(bot);
    bot.view(|view| view.ground_below(feet))
        .unwrap_or_else(|| panic!("{} holds no chunk under its feet", bot.name()))
}

/// The six slots a player is shown with, in Set Equipment order.
const PLAYER_EQUIPMENT: [EquipmentSlot; 6] = [
    EquipmentSlot::Mainhand,
    EquipmentSlot::Offhand,
    EquipmentSlot::Feet,
    EquipmentSlot::Legs,
    EquipmentSlot::Chest,
    EquipmentSlot::Head,
];

/// What `observer` is shown `actor` holding and wearing: `(kind, count)` per
/// slot of [`PLAYER_EQUIPMENT`], `None` for an empty slot.
fn shown_equipment(observer: &Bot, actor: &Bot) -> [Option<(ItemKind, i32)>; 6] {
    let id = actor.entity_id();
    observer.view(|view| {
        PLAYER_EQUIPMENT.map(|slot| match view.equipment_of(id, slot) {
            ItemStack::Empty => None,
            ItemStack::Present(data) => Some((data.kind, data.count)),
        })
    })
}

/// Waits until `observer` is shown `actor` with exactly `expected` (every
/// slot not listed empty).
async fn wait_for_equipment(
    observer: &Bot,
    actor: &Bot,
    expected: &[(EquipmentSlot, ItemKind, i32)],
) {
    let wanted = PLAYER_EQUIPMENT.map(|slot| {
        expected
            .iter()
            .find(|(listed, ..)| *listed == slot)
            .map(|&(_, kind, count)| (kind, count))
    });
    let id = actor.entity_id();
    let description = format!("{} with {wanted:?}", actor.name());
    let reached = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        observer
            .wait_until(&description, |view| {
                PLAYER_EQUIPMENT.iter().zip(&wanted).all(|(slot, wanted)| {
                    match (view.equipment_of(id, *slot), wanted) {
                        (ItemStack::Empty, None) => true,
                        (ItemStack::Present(data), Some((kind, count))) => {
                            data.kind == *kind && data.count == *count
                        }
                        _ => false,
                    }
                })
            })
            .await
    })
    .await;
    if reached.is_err() {
        panic!(
            "{} was never shown {description}; it is shown {:?}",
            observer.name(),
            shown_equipment(observer, actor)
        );
    }
}
