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

use voidmc::components::PlayerReady;
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, Vec3};
use voidmc_e2e::inventory::ItemStack;
use voidmc_e2e::inventory::components::EquipmentSlot;
use voidmc_e2e::protocol::packets::game::s_command_suggestion::ServerboundCommandSuggestion;
use voidmc_e2e::protocol::packets::game::s_interact::InteractionHand;
use voidmc_e2e::protocol::packets::game::s_use_item_on::{BlockHit, ServerboundUseItemOn};
use voidmc_e2e::protocol::packets::{Packet, ProtocolPacket};
use voidmc_e2e::registry::builtin::ItemKind;
use voidmc_e2e::{Bot, TestServer};

/// `packet` as a client writes it, its id then its body, for a test to alter.
fn raw<P: ProtocolPacket + std::fmt::Debug>(packet: impl Packet<P>) -> Vec<u8> {
    voidmc_e2e::protocol::write::serialize_packet(&packet.into_variant())
        .expect("Azalea encodes the packet")
        .into_vec()
}

/// `value` as a protocol VarInt.
fn var_int(mut value: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            bytes.push(byte);
            return bytes;
        }
        bytes.push(byte | 0x80);
    }
}

/// A Command Suggestions Request for `text`, built by hand: Azalea refuses
/// to write a string of more than 32767 bytes, which a valid text of 32500
/// multibyte characters takes.
fn command_suggestion(id: u32, text: &str) -> Vec<u8> {
    let mut packet = raw(ServerboundCommandSuggestion {
        id,
        command: String::new(),
    });
    assert_eq!(packet.pop(), Some(0), "the empty text ends the packet");
    packet.extend(var_int(text.len() as u32));
    packet.extend(text.as_bytes());
    packet
}

/// A valid Use Item On: its id, the hand, the block position (8 bytes), the
/// face at index 10, the cursor (12 bytes), two flags, the sequence.
fn use_item_on() -> Vec<u8> {
    let packet = raw(ServerboundUseItemOn {
        hand: InteractionHand::MainHand,
        block_hit: BlockHit {
            block_pos: BlockPos::new(0, 64, 0),
            direction: Direction::Up,
            location: Vec3 {
                x: 0.5,
                y: 65.0,
                z: 0.5,
            },
            inside: false,
            world_border: false,
        },
        seq: 1,
    });
    assert_eq!(packet.len(), 26, "{packet:?}");
    assert_eq!(packet[10], Direction::Up as u8, "{packet:?}");
    packet
}

/// Known play packets whose body VoidMC cannot decode, by what is wrong with
/// them.
fn malformed_play_packets() -> Vec<(&'static str, Vec<u8>)> {
    let valid = use_item_on();
    let mut invalid_face = valid.clone();
    // Faces are 0 to 5.
    invalid_face[10] = 6;
    // Cut after the cursor: no flags, no sequence.
    let truncated = valid[..valid.len() - 3].to_vec();
    let mut trailing = valid.clone();
    trailing.extend([1, 2, 3]);
    // More bytes than a string of the protocol's 32767 characters takes, at
    // most 3 bytes each.
    let over_long = command_suggestion(1, &"a".repeat(32_767 * 3 + 1));
    vec![
        ("a Use Item On with an invalid face", invalid_face),
        ("a truncated Use Item On", truncated),
        ("a Use Item On with trailing bytes", trailing),
        ("an over-long command suggestion text", over_long),
    ]
}

/// The start of the reason a client is shown when the server could not
/// decode one of its packets, after vanilla's "Internal Exception: ...".
const DECODE_FAILURE: &str = "Internal Exception: Failed to decode packet";

fn feet(bot: &Bot) -> BlockPos {
    BlockPos::from(bot.position())
}

fn ground(bot: &Bot) -> BlockPos {
    let feet = feet(bot);
    bot.view(|view| view.ground_below(feet))
        .unwrap_or_else(|| panic!("{} holds no chunk under its feet", bot.name()))
}

/// Waits until the server counts the bot's player as ready, as it does once
/// the client reports it has loaded the world around it.
async fn wait_until_ready(server: &TestServer, bot: &Bot) {
    let player = server.player_entity(bot.uuid()).await;
    server
        .wait_for("the player to be ready", move |world| {
            world.get::<PlayerReady>(player).map(|_| ())
        })
        .await;
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
