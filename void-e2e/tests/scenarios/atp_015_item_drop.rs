//! VMC-ATP-015: a thrown item becomes a visible world entity and can be
//! collected by a nearby player after its pickup delay.
//!
//! Every scenario builds the same arena in the air above the players with
//! block edits on the game thread (`mutate_block`, the path player edits take,
//! so the clients are sent the blocks): a stone floor two blocks thick, a wall
//! three blocks high and a tower ten blocks high standing on it. Players are
//! moved onto it like clients walking there, and they drop items with the
//! packets a vanilla client sends: the drop key (Player Action), inventory
//! clicks (Container Click) and the creative inventory (Set Creative Mode
//! Slot, slot -1).
//!
//! A recorder system samples every item entity once per tick, after the
//! simulation, together with the number of diamonds in the whole world
//! (inventories, cursors and item entities). The assertions therefore see
//! every tick of a fall and of the pickup delay, and that no diamond is ever
//! created or lost.

use azalea_entity::{EntityDataValue, LookDirection};
use bevy_app::PostUpdate;
use bevy_ecs::prelude::*;
use voidmc::components::{
    EntityDimension, Grounded, ItemEntity, MinecraftEntityId, Position, Velocity,
};
use voidmc::world::{
    BlockMutation, ChunkData, ChunkIndex, ChunkPosition, DimensionId, block_state_at_world,
    is_solid_block_state, mutate_block,
};
use voidmc::{BlockFace, BlockPosition, Inventory, ItemId, ItemStack as ServerItemStack};
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, Vec3};
use voidmc_e2e::inventory::operations::ClickType;
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::common::movements::MoveFlags;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::protocol::packets::game::s_container_click::{
    HashedActualItem, HashedPatchMap, HashedStack, ServerboundContainerClick,
};
use voidmc_e2e::protocol::packets::game::s_move_player_pos_rot::ServerboundMovePlayerPosRot;
use voidmc_e2e::protocol::packets::game::s_player_action::{Action, ServerboundPlayerAction};
use voidmc_e2e::protocol::packets::game::s_set_creative_mode_slot::ServerboundSetCreativeModeSlot;
use voidmc_e2e::registry::builtin::{EntityKind, ItemKind};
use voidmc_e2e::{Bot, TestServer};
use voidmc_protocol::clientbound::chunk::blocks;

use crate::feet;

/// The documented pickup delay, in ticks.
const PICKUP_DELAY: u64 = 40;
/// The documented pickup reach, from a player's feet to the item.
const PICKUP_RADIUS: f64 = 1.5;
/// A throw leaves 0.3 below a standing player's eyes (1.62).
const THROW_HEIGHT: f64 = 1.32;
/// The item entity's collision box.
const ITEM_HALF_WIDTH: f64 = 0.125;
const ITEM_HEIGHT: f64 = 0.25;
/// Entity-metadata index of an item entity's stack.
const ITEM_INDEX: u8 = 8;
/// Vanilla yaw facing east (+x).
const EAST: f32 = -90.0;
/// Pitch looking straight down.
const DOWN: f32 = 90.0;
const PLAYER_WINDOW: i32 = 0;
const FIRST_HOTBAR_SLOT: usize = 36;
/// Slot number of a click outside the inventory window.
const OUTSIDE: i16 = -999;
/// The floor spans `-HALF..=HALF` blocks around the arena's centre.
const HALF: i32 = 8;

#[derive(Clone, Copy, Debug)]
struct Item {
    name: &'static str,
    kind: ItemKind,
}

const DIAMOND: Item = Item {
    name: "minecraft:diamond",
    kind: ItemKind::Diamond,
};
const EMERALD: Item = Item {
    name: "minecraft:emerald",
    kind: ItemKind::Emerald,
};
const GOLD: Item = Item {
    name: "minecraft:gold_ingot",
    kind: ItemKind::GoldIngot,
};
const IRON: Item = Item {
    name: "minecraft:iron_ingot",
    kind: ItemKind::IronIngot,
};
const STONE: Item = Item {
    name: "minecraft:stone",
    kind: ItemKind::Stone,
};

impl Item {
    fn stack(self, count: u8) -> ServerItemStack {
        ServerItemStack::of(self.name, count).expect("a known item")
    }

    fn client_stack(self, count: i32) -> ItemStack {
        ItemStack::Present(ItemStackData::new(self.kind, count))
    }
}

/// Whether the client stack is `count` × `item`; a count of 0 is the empty slot.
fn shows(stack: &ItemStack, item: Item, count: i32) -> bool {
    match stack {
        ItemStack::Empty => count == 0,
        ItemStack::Present(data) => data.kind == item.kind && data.count == count,
    }
}

/// A floor in the air, two blocks thick, with a three-block wall and a
/// ten-block tower standing on it.
#[derive(Clone, Copy, Debug)]
struct Arena {
    x: i32,
    z: i32,
    /// The height of the floor's top layer.
    top: i32,
}

impl Arena {
    /// The feet height of anything standing on the floor.
    fn ground(self) -> f64 {
        f64::from(self.top + 1)
    }

    fn at(self, dx: f64, dz: f64) -> Vec3 {
        Vec3::new(
            f64::from(self.x) + dx,
            self.ground(),
            f64::from(self.z) + dz,
        )
    }

    /// The west face of the wall, which spans `z + 3 ..= z + 5`.
    fn wall_face(self) -> f64 {
        f64::from(self.x + 6)
    }

    /// Standing on top of the tower, ten blocks above the floor.
    fn tower_top(self) -> Vec3 {
        Vec3::new(
            f64::from(self.x) - 6.5,
            self.ground() + 10.0,
            f64::from(self.z) - 4.5,
        )
    }

    fn blocks(self) -> Vec<[i32; 3]> {
        let mut blocks = Vec::new();
        for dx in -HALF..=HALF {
            for dz in -HALF..=HALF {
                blocks.push([self.x + dx, self.top - 1, self.z + dz]);
                blocks.push([self.x + dx, self.top, self.z + dz]);
            }
        }
        for y in 1..=3 {
            for dz in 3..=5 {
                blocks.push([self.x + 6, self.top + y, self.z + dz]);
            }
        }
        for y in 1..=10 {
            blocks.push([self.x - 7, self.top + y, self.z - 5]);
        }
        blocks
    }
}

/// Builds the arena twenty blocks above Alice's feet and waits until she is
/// sent its last block.
async fn build_arena(server: &TestServer, alice: &Bot) -> Arena {
    let feet = feet(alice);
    let arena = Arena {
        x: feet.x,
        z: feet.z,
        top: feet.y + 20,
    };
    let actor = server.player_entity(alice.uuid()).await;
    let placed = arena.blocks();
    let count = placed.len();
    let changed = server
        .with_world(move |world| {
            placed
                .into_iter()
                .filter(|&[x, y, z]| {
                    let position = BlockPosition { x, y: y as i16, z };
                    mutate_block(
                        world,
                        actor,
                        DimensionId::Overworld,
                        position,
                        blocks::STONE,
                        BlockFace::Top,
                        BlockMutation::Place,
                    )
                    .is_some()
                })
                .count()
        })
        .await;
    assert_eq!(changed, count, "the arena was built in empty air");
    let last = BlockPos::new(arena.x - 7, arena.top + 10, arena.z - 5);
    alice
        .wait_until("the arena", |view| {
            view.block_at(last).is_some_and(|state| !state.is_air())
        })
        .await;
    arena
}

/// One item entity in one tick, as the server holds it after the simulation.
#[derive(Clone, Copy, Debug)]
struct Sample {
    tick: u64,
    id: i32,
    position: [f64; 3],
    grounded: bool,
    count: u8,
    /// A solid block that the item's box overlaps with a positive volume.
    overlap: Option<[i32; 3]>,
}

#[derive(Resource, Default)]
struct Trace {
    tick: u64,
    samples: Vec<Sample>,
    /// The number of diamonds in the world at the end of each tick.
    diamonds: Vec<(u64, u32)>,
}

fn count_of(stack: &ServerItemStack, item: ItemId) -> u32 {
    if stack.item == item {
        u32::from(stack.count)
    } else {
        0
    }
}

fn record(
    mut trace: ResMut<Trace>,
    index: Res<ChunkIndex>,
    chunks: Query<(&ChunkPosition, &ChunkData)>,
    items: Query<(
        &MinecraftEntityId,
        &Position,
        &Grounded,
        &ItemEntity,
        &EntityDimension,
    )>,
    inventories: Query<&Inventory>,
) {
    trace.tick += 1;
    let tick = trace.tick;
    let diamond = ItemId::from_name(DIAMOND.name).unwrap();
    let mut diamonds: u32 = inventories
        .iter()
        .map(|inventory| {
            (0..Inventory::SIZE)
                .map(|slot| count_of(inventory.get(slot), diamond))
                .sum::<u32>()
                + count_of(inventory.cursor(), diamond)
        })
        .sum();
    for (id, position, grounded, item, dimension) in &items {
        diamonds += count_of(&item.stack, diamond);
        // Exact geometry, independent of the server's collision code: every
        // block in these ranges overlaps the box; touching a face does not.
        let low = |c: f64| c.floor() as i32;
        let high = |c: f64| c.ceil() as i32 - 1;
        let (x0, x1) = (
            low(position.x - ITEM_HALF_WIDTH),
            high(position.x + ITEM_HALF_WIDTH),
        );
        let (y0, y1) = (low(position.y), high(position.y + ITEM_HEIGHT));
        let (z0, z1) = (
            low(position.z - ITEM_HALF_WIDTH),
            high(position.z + ITEM_HALF_WIDTH),
        );
        let overlap = (x0..=x1)
            .flat_map(|x| (y0..=y1).flat_map(move |y| (z0..=z1).map(move |z| [x, y, z])))
            .find(|&[x, y, z]| {
                block_state_at_world(&index, &chunks, dimension.0, x, y, z)
                    .is_some_and(is_solid_block_state)
            });
        trace.samples.push(Sample {
            tick,
            id: id.0,
            position: [position.x, position.y, position.z],
            grounded: grounded.0,
            count: item.stack.count,
            overlap,
        });
    }
    trace.diamonds.push((tick, diamonds));
}

async fn recorded_server() -> TestServer {
    TestServer::builder()
        .plugin(|app| {
            app.init_resource::<Trace>().add_systems(PostUpdate, record);
        })
        .start()
        .await
}

async fn current_tick(server: &TestServer) -> u64 {
    server
        .with_world(|world| world.resource::<Trace>().tick)
        .await
}

async fn wait_for_tick(server: &TestServer, tick: u64) {
    server
        .wait_for(&format!("tick {tick}"), move |world| {
            (world.resource::<Trace>().tick >= tick).then_some(())
        })
        .await;
}

/// The recorded samples of one item, oldest first.
async fn samples(server: &TestServer, id: i32) -> Vec<Sample> {
    server
        .with_world(move |world| {
            world
                .resource::<Trace>()
                .samples
                .iter()
                .filter(|sample| sample.id == id)
                .copied()
                .collect()
        })
        .await
}

/// Every recorded diamond total after `tick` equals `expected`.
async fn assert_diamonds_conserved(server: &TestServer, after: u64, expected: u32) {
    let totals: Vec<(u64, u32)> = server
        .with_world(move |world| {
            world
                .resource::<Trace>()
                .diamonds
                .iter()
                .copied()
                .filter(|(tick, _)| *tick > after)
                .collect()
        })
        .await;
    assert!(
        totals.len() > 40,
        "the scenario ran for a while: {totals:?}"
    );
    let wrong: Vec<_> = totals
        .iter()
        .filter(|(_, total)| *total != expected)
        .collect();
    assert!(
        wrong.is_empty(),
        "the world held other than {expected} diamonds at {wrong:?}"
    );
}

/// The item's position and stack size on the server, if it still exists.
async fn item_state(server: &TestServer, id: i32) -> Option<([f64; 3], u8)> {
    server
        .with_world(move |world| {
            world
                .query::<(&MinecraftEntityId, &Position, &ItemEntity)>()
                .iter(world)
                .find(|(entity, _, _)| entity.0 == id)
                .map(|(_, position, item)| ([position.x, position.y, position.z], item.stack.count))
        })
        .await
}

/// Waits until the item lies still on a block and returns its position.
async fn rest(server: &TestServer, id: i32) -> [f64; 3] {
    server
        .wait_for("the item to come to rest", move |world| {
            world
                .query::<(&MinecraftEntityId, &Position, &Grounded, &Velocity)>()
                .iter(world)
                .find(|(entity, _, _, _)| entity.0 == id)
                .filter(|(_, _, grounded, velocity)| {
                    grounded.0 && velocity.x == 0.0 && velocity.y == 0.0 && velocity.z == 0.0
                })
                .map(|(_, position, _, _)| [position.x, position.y, position.z])
        })
        .await
}

async fn item_entity_count(server: &TestServer) -> usize {
    server
        .with_world(|world| world.query::<&ItemEntity>().iter(world).count())
        .await
}

/// Sets a slot of the player's inventory on the server and waits until the
/// client is sent it.
async fn set_slot(server: &TestServer, bot: &Bot, slot: usize, item: Item, count: u8) {
    let player = server.player_entity(bot.uuid()).await;
    let stack = item.stack(count);
    server
        .with_world(move |world| {
            world
                .get_mut::<Inventory>(player)
                .expect("players have an inventory")
                .set(slot, stack);
        })
        .await;
    bot.wait_until(&format!("{count} × {} in slot {slot}", item.name), |view| {
        shows(&view.inventory[slot], item, i32::from(count))
    })
    .await;
}

/// How many of `item` the player holds, server side.
async fn holding(server: &TestServer, bot: &Bot, item: Item) -> u32 {
    let player = server.player_entity(bot.uuid()).await;
    server
        .with_world(move |world| {
            let item = ItemId::from_name(item.name).unwrap();
            let inventory = world.get::<Inventory>(player).unwrap();
            (0..Inventory::SIZE)
                .map(|slot| count_of(inventory.get(slot), item))
                .sum::<u32>()
                + count_of(inventory.cursor(), item)
        })
        .await
}

/// Moves the bot there and turns it, like a client walking and looking.
async fn place(bot: &Bot, position: Vec3, yaw: f32, pitch: f32) {
    bot.move_to(position).await;
    bot.send(ServerboundMovePlayerPosRot {
        pos: position,
        look_direction: LookDirection::new(yaw, pitch),
        flags: MoveFlags {
            on_ground: true,
            horizontal_collision: false,
        },
    })
    .await;
    bot.sync().await;
}

/// Presses the drop key: one item, or the whole stack with Ctrl.
async fn drop_key(bot: &Bot, whole_stack: bool) {
    bot.send(ServerboundPlayerAction {
        action: if whole_stack {
            Action::DropAllItems
        } else {
            Action::DropItem
        },
        pos: BlockPos::new(0, 0, 0),
        direction: Direction::Down,
        seq: 0,
    })
    .await;
}

/// An item entity added for the bot: its id, position and velocity.
struct Added {
    id: i32,
    position: Vec3,
    motion: Vec3,
}

/// Expects the add-entity of an item and the metadata that makes a vanilla
/// client render it as `count` × `item`.
async fn expect_item(bot: &Bot, item: Item, count: i32) -> Added {
    let added = bot
        .expect("an item entity added", |packet| match packet {
            ClientboundGamePacket::AddEntity(add) if add.entity_type == EntityKind::Item => {
                Some(Added {
                    id: add.id.0,
                    position: add.position,
                    motion: add.movement.to_vec3(),
                })
            }
            _ => None,
        })
        .await;
    expect_item_metadata(bot, added.id, item, count).await;
    added
}

async fn expect_item_metadata(bot: &Bot, id: i32, item: Item, count: i32) {
    bot.expect(
        &format!("item {id} shown as {count} × {}", item.name),
        |packet| match packet {
            ClientboundGamePacket::SetEntityData(data) if data.id.0 == id => data
                .packed_items
                .0
                .iter()
                .any(|entry| {
                    entry.index == ITEM_INDEX
                        && matches!(&entry.value, EntityDataValue::ItemStack(stack)
                            if shows(stack, item, count))
                })
                .then_some(()),
            _ => None,
        },
    )
    .await;
}

/// Expects the pickup animation of item `id`; returns the collector and the
/// amount.
async fn expect_take(bot: &Bot, id: i32) -> (i32, u32) {
    bot.expect(&format!("item {id} taken"), |packet| match packet {
        ClientboundGamePacket::TakeItemEntity(take) if take.item_id as i32 == id => {
            Some((take.player_id.0, take.amount))
        }
        _ => None,
    })
    .await
}

async fn expect_removed(bot: &Bot, id: i32) {
    bot.expect(&format!("item {id} removed"), |packet| match packet {
        ClientboundGamePacket::RemoveEntities(remove)
            if remove.entity_ids.iter().any(|removed| removed.0 == id) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;
}

async fn expect_slot(bot: &Bot, slot: usize, item: Item, count: i32) {
    bot.expect(
        &format!("slot {slot} set to {count} × {}", item.name),
        |packet| match packet {
            ClientboundGamePacket::ContainerSetSlot(set)
                if set.container_id == PLAYER_WINDOW
                    && usize::from(set.slot) == slot
                    && shows(&set.item_stack, item, count) =>
            {
                Some(())
            }
            _ => None,
        },
    )
    .await;
}

/// Packets about item `id` still queued for the bot.
fn queued_about(bot: &Bot, id: i32) -> Vec<String> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::TakeItemEntity(take) if take.item_id as i32 == id => {
            Some(format!("{take:?}"))
        }
        ClientboundGamePacket::SetEntityData(data) if data.id.0 == id => Some(format!("{data:?}")),
        ClientboundGamePacket::RemoveEntities(remove)
            if remove.entity_ids.iter().any(|removed| removed.0 == id) =>
        {
            Some(format!("{remove:?}"))
        }
        _ => None,
    })
}

fn queued_item_adds(bot: &Bot) -> usize {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::AddEntity(add) if add.entity_type == EntityKind::Item => Some(()),
        _ => None,
    })
    .len()
}

/// The client's accumulated position of the item matches the server's, to
/// the 1/4096-block precision of relative moves.
fn assert_seen_at(bot: &Bot, id: i32, server_position: [f64; 3]) {
    let seen = bot
        .view(|view| view.entities.get(&id).map(|entity| entity.position))
        .unwrap_or_else(|| panic!("{} does not see item {id}", bot.name()));
    let off = [
        seen.x - server_position[0],
        seen.y - server_position[1],
        seen.z - server_position[2],
    ];
    assert!(
        off.iter().all(|d| d.abs() <= 1.0 / 4096.0),
        "{} sees item {id} at {seen:?}, the server has it at {server_position:?}",
        bot.name()
    );
}

/// The item never overlapped a block nor went below the floor, and once it
/// reached `rest` it never moved again.
fn assert_fell_cleanly(samples: &[Sample], ground: f64, rest: [f64; 3]) {
    assert!(!samples.is_empty());
    for sample in samples {
        assert!(sample.position.iter().all(|c| c.is_finite()), "{sample:?}");
        assert_eq!(sample.overlap, None, "the box overlaps a block: {sample:?}");
        assert!(sample.position[1] >= ground, "below the floor: {sample:?}");
    }
    let landed = samples
        .iter()
        .position(|sample| sample.position == rest)
        .expect("the item reached its resting place");
    for sample in &samples[landed..] {
        assert_eq!(sample.position, rest, "moved after coming to rest");
        assert!(sample.grounded, "resting but not grounded: {sample:?}");
    }
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[tokio::test]
async fn a_dropped_item_is_seen_by_everyone_and_collected_by_bob_after_the_delay() {
    let server = recorded_server().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let arena = build_arena(&server, &alice).await;
    let ground = arena.ground();

    let thrower = arena.at(-2.5, 0.5);
    let bob_waits = arena.at(3.5, -3.5);
    place(&alice, thrower, EAST, 0.0).await;
    place(&bob, bob_waits, 0.0, 0.0).await;
    set_slot(&server, &alice, FIRST_HOTBAR_SLOT, DIAMOND, 5).await;
    set_slot(&server, &bob, 9, DIAMOND, 10).await;
    let start = current_tick(&server).await;

    // Step 1: Alice presses the drop key once.
    drop_key(&alice, false).await;
    expect_slot(&alice, FIRST_HOTBAR_SLOT, DIAMOND, 4).await;
    assert_eq!(holding(&server, &alice, DIAMOND).await, 4);

    // Both clients add one item entity, rendered as one diamond, thrown from
    // Alice's eyes (shown one tick along its arc) eastwards, where she looks.
    let added = expect_item(&alice, DIAMOND, 1).await;
    let seen_by_bob = expect_item(&bob, DIAMOND, 1).await;
    let id = added.id;
    assert_eq!(seen_by_bob.id, id);
    assert_eq!(seen_by_bob.position, added.position);
    let ahead = added.position.x - thrower.x;
    assert!(
        ahead > 0.0 && ahead <= 0.31,
        "added at {:?}, Alice is at {thrower:?}",
        added.position
    );
    assert!(close(added.position.y, thrower.y + THROW_HEIGHT, 0.05));
    assert!(close(added.position.z, thrower.z, 1e-9));
    let motion = added.motion;
    assert!(
        close(motion.x, 0.3, 0.02) && close(motion.y, 0.0, 0.1) && close(motion.z, 0.0, 0.01),
        "thrown at {motion:?}"
    );

    // Step 2: it falls onto the floor ahead of Alice, out of her reach, and
    // lies there past the delay.
    let rest_at = rest(&server, id).await;
    assert_eq!(rest_at[1], ground);
    assert!(close(rest_at[2], thrower.z, 1e-9));
    let distance = rest_at[0] - thrower.x;
    assert!(
        distance > PICKUP_RADIUS + 0.2 && distance < 4.0,
        "landed {distance} blocks ahead of Alice"
    );
    let first = samples(&server, id).await[0].tick;
    wait_for_tick(&server, first + PICKUP_DELAY + 10).await;
    let trace = samples(&server, id).await;
    assert_fell_cleanly(&trace, ground, rest_at);
    let airborne = trace.iter().filter(|sample| !sample.grounded).count();
    assert!(airborne >= 4, "it flew in an arc: {trace:?}");
    assert!(
        trace[0].position[1] > thrower.y + THROW_HEIGHT,
        "it rose in its first tick: {trace:?}"
    );
    assert!(
        trace
            .windows(2)
            .all(|w| w[1].position[0] >= w[0].position[0]),
        "it only went east: {trace:?}"
    );
    assert_eq!(
        item_state(&server, id).await,
        Some((rest_at, 1)),
        "the thrower, out of reach, did not take it back"
    );
    for bot in [&alice, &bob] {
        bot.sync().await;
        assert_seen_at(bot, id, rest_at);
    }

    // Bob leaves the area and comes back: the item is removed for him, then
    // added again with its stack.
    place(&bob, arena.at(300.0, -3.5), 0.0, 0.0).await;
    expect_removed(&bob, id).await;
    place(&bob, bob_waits, 0.0, 0.0).await;
    let again = expect_item(&bob, DIAMOND, 1).await;
    assert_eq!(again.id, id);
    assert_eq!(
        again.position,
        Vec3::new(rest_at[0], rest_at[1], rest_at[2])
    );

    // A player who joins now is sent the item and its stack too.
    let carol = server.join("Carol").await;
    let late = expect_item(&carol, DIAMOND, 1).await;
    assert_eq!(late.id, id);
    assert_eq!(late.position, again.position);

    // Step 3: Bob walks up to the item: just out of reach, then within.
    alice.clear();
    let item_at = Vec3::new(rest_at[0], ground, rest_at[2]);
    place(
        &bob,
        item_at + Vec3::new(PICKUP_RADIUS + 0.05, 0.0, 0.0),
        0.0,
        0.0,
    )
    .await;
    server.wait_ticks(2).await;
    bob.sync().await;
    assert!(queued_about(&bob, id).is_empty(), "taken from out of reach");
    assert_eq!(item_state(&server, id).await, Some((rest_at, 1)));

    place(
        &bob,
        item_at + Vec3::new(PICKUP_RADIUS - 0.05, 0.0, 0.0),
        0.0,
        0.0,
    )
    .await;
    for viewer in [&alice, &bob, &carol] {
        assert_eq!(
            expect_take(viewer, id).await,
            (bob.entity_id(), 1),
            "{} sees Bob take one diamond",
            viewer.name()
        );
        expect_removed(viewer, id).await;
    }

    // Step 4: the diamond went into Bob's partial stack, once.
    expect_slot(&bob, 9, DIAMOND, 11).await;
    assert_eq!(item_state(&server, id).await, None);
    assert_eq!(holding(&server, &bob, DIAMOND).await, 11);
    assert_eq!(holding(&server, &alice, DIAMOND).await, 4);
    for viewer in [&alice, &bob, &carol] {
        viewer.sync().await;
        assert!(
            queued_about(viewer, id).is_empty(),
            "{} got more about the item: {:?}",
            viewer.name(),
            queued_about(viewer, id)
        );
    }
    let alice_inventory = alice.queued(|packet| match packet {
        ClientboundGamePacket::ContainerSetSlot(set) if set.container_id == PLAYER_WINDOW => {
            Some(format!("{set:?}"))
        }
        ClientboundGamePacket::ContainerSetContent(set) if set.container_id == PLAYER_WINDOW => {
            Some(format!("{set:?}"))
        }
        _ => None,
    });
    assert!(
        alice_inventory.is_empty(),
        "Alice's inventory changed: {alice_inventory:?}"
    );
    assert_eq!(
        bob.view(|view| view.inventory[9].clone()),
        DIAMOND.client_stack(11)
    );
    assert_diamonds_conserved(&server, start, 15).await;

    alice.assert_healthy();
    bob.assert_healthy();
    carol.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn the_item_waits_out_the_pickup_delay_and_only_one_player_gets_it() {
    let server = recorded_server().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let carol = server.join("Carol").await;
    let arena = build_arena(&server, &alice).await;
    let ground = arena.ground();

    // Alice looks at her feet; Bob and Carol stand right where she does, so
    // all three are within reach of the item from the moment it lands.
    let spot = arena.at(0.5, 0.5);
    place(&alice, spot, 0.0, DOWN).await;
    place(&bob, spot, 0.0, 0.0).await;
    place(&carol, spot, 0.0, 0.0).await;
    set_slot(&server, &alice, FIRST_HOTBAR_SLOT, DIAMOND, 5).await;
    let start = current_tick(&server).await;

    drop_key(&alice, false).await;
    let id = expect_item(&alice, DIAMOND, 1).await.id;
    for viewer in [&bob, &carol] {
        assert_eq!(expect_item(viewer, DIAMOND, 1).await.id, id);
    }
    let mut collector = None;
    for viewer in [&alice, &bob, &carol] {
        let (taker, amount) = expect_take(viewer, id).await;
        assert_eq!(amount, 1);
        assert!(collector.is_none_or(|collector| collector == taker));
        collector = Some(taker);
        expect_removed(viewer, id).await;
    }

    // The item lay at the players' feet, in reach of all three, for exactly
    // the delay: it was thrown in its first tick and taken in the 40th after.
    let trace = samples(&server, id).await;
    let first = trace.first().unwrap().tick;
    let last = trace.last().unwrap().tick;
    assert_eq!(
        last - first + 1,
        PICKUP_DELAY,
        "the item existed from tick {first} to tick {last}"
    );
    let rest_at = [spot.x, ground, spot.z];
    assert_fell_cleanly(&trace, ground, rest_at);
    let landed = trace.iter().position(|s| s.position == rest_at).unwrap();
    assert!(landed < 10, "landed in tick {landed}: {trace:?}");

    // Exactly one of them got it, once.
    for viewer in [&alice, &bob, &carol] {
        viewer.sync().await;
        assert!(
            queued_about(viewer, id).is_empty(),
            "{}: {:?}",
            viewer.name(),
            queued_about(viewer, id)
        );
    }
    let collector = collector.unwrap();
    let mut gains = Vec::new();
    for (bot, before) in [(&alice, 4), (&bob, 0), (&carol, 0)] {
        let gain = holding(&server, bot, DIAMOND).await - before;
        gains.push(gain);
        assert_eq!(
            gain,
            u32::from(bot.entity_id() == collector),
            "{}",
            bot.name()
        );
    }
    assert_eq!(gains.iter().sum::<u32>(), 1);
    assert_diamonds_conserved(&server, start, 5).await;

    alice.assert_healthy();
    bob.assert_healthy();
    carol.assert_healthy();
    server.stop().await;
}

fn hashed(item: Item, count: i32) -> HashedStack {
    HashedStack(Some(HashedActualItem {
        kind: item.kind,
        count,
        components: HashedPatchMap {
            added_components: Vec::new(),
            removed_components: Vec::new(),
        },
    }))
}

async fn click(
    bot: &Bot,
    slot: i16,
    button: u8,
    click_type: ClickType,
    changed: Vec<(u16, HashedStack)>,
    carried: HashedStack,
) {
    let state_id = bot.view(|view| view.inventory_state_id);
    bot.send(ServerboundContainerClick {
        container_id: PLAYER_WINDOW,
        state_id,
        slot_num: slot,
        button_num: button,
        click_type,
        changed_slots: changed.into_iter().collect(),
        carried_item: carried,
    })
    .await;
}

#[tokio::test]
async fn every_drop_path_throws_exactly_the_dropped_stack() {
    let server = recorded_server().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let arena = build_arena(&server, &alice).await;
    place(&alice, arena.at(-4.5, 0.5), EAST, 0.0).await;
    place(&bob, arena.at(-4.5, -6.5), 0.0, 0.0).await;
    set_slot(&server, &alice, FIRST_HOTBAR_SLOT, DIAMOND, 5).await;

    // The drop key: one item, then the rest of the stack.
    drop_key(&alice, false).await;
    expect_slot(&alice, FIRST_HOTBAR_SLOT, DIAMOND, 4).await;
    expect_item(&alice, DIAMOND, 1).await;
    expect_item(&bob, DIAMOND, 1).await;
    drop_key(&alice, true).await;
    expect_slot(&alice, FIRST_HOTBAR_SLOT, DIAMOND, 0).await;
    expect_item(&alice, DIAMOND, 4).await;
    expect_item(&bob, DIAMOND, 4).await;
    assert_eq!(holding(&server, &alice, DIAMOND).await, 0);

    // With an empty hand, the drop key throws nothing.
    drop_key(&alice, false).await;
    drop_key(&alice, true).await;
    alice.sync().await;
    bob.sync().await;
    assert_eq!(queued_item_adds(&alice), 0);
    assert_eq!(queued_item_adds(&bob), 0);
    assert_eq!(item_entity_count(&server).await, 2);

    // The inventory screen: Q over a slot, then Ctrl+Q.
    set_slot(&server, &alice, 9, EMERALD, 5).await;
    click(
        &alice,
        9,
        0,
        ClickType::Throw,
        vec![(9, hashed(EMERALD, 4))],
        HashedStack(None),
    )
    .await;
    expect_item(&alice, EMERALD, 1).await;
    expect_item(&bob, EMERALD, 1).await;
    expect_slot(&alice, 9, EMERALD, 4).await;
    click(
        &alice,
        9,
        1,
        ClickType::Throw,
        vec![(9, HashedStack(None))],
        HashedStack(None),
    )
    .await;
    expect_item(&alice, EMERALD, 4).await;
    expect_item(&bob, EMERALD, 4).await;
    expect_slot(&alice, 9, EMERALD, 0).await;

    // A stack picked up on the cursor and clicked outside the window.
    set_slot(&server, &alice, 10, GOLD, 3).await;
    click(
        &alice,
        10,
        0,
        ClickType::Pickup,
        vec![(10, HashedStack(None))],
        hashed(GOLD, 3),
    )
    .await;
    click(
        &alice,
        OUTSIDE,
        0,
        ClickType::Pickup,
        Vec::new(),
        HashedStack(None),
    )
    .await;
    expect_item(&alice, GOLD, 3).await;
    expect_item(&bob, GOLD, 3).await;
    assert_eq!(holding(&server, &alice, GOLD).await, 0);

    // The creative inventory throws with slot -1.
    alice
        .send(ServerboundSetCreativeModeSlot {
            slot_num: u16::MAX,
            item_stack: IRON.client_stack(2),
        })
        .await;
    expect_item(&alice, IRON, 2).await;
    expect_item(&bob, IRON, 2).await;

    // One entity per drop and nothing else.
    alice.sync().await;
    bob.sync().await;
    assert_eq!(queued_item_adds(&alice), 0);
    assert_eq!(queued_item_adds(&bob), 0);
    assert_eq!(item_entity_count(&server).await, 6);

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn a_full_inventory_leaves_the_item_and_a_partial_pickup_keeps_the_rest() {
    let server = recorded_server().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let arena = build_arena(&server, &alice).await;
    let spot = arena.at(0.5, 0.5);

    // Bob's storage is full but for one diamond on a stack of 63.
    let bob_entity = server.player_entity(bob.uuid()).await;
    server
        .with_world(move |world| {
            let mut inventory = world.get_mut::<Inventory>(bob_entity).unwrap();
            for slot in Inventory::MAIN_START..Inventory::OFFHAND {
                inventory.set(slot, STONE.stack(64));
            }
            inventory.set(9, DIAMOND.stack(63));
        })
        .await;
    bob.wait_until("a full inventory", |view| {
        shows(&view.inventory[9], DIAMOND, 63) && shows(&view.inventory[44], STONE, 64)
    })
    .await;
    set_slot(&server, &alice, FIRST_HOTBAR_SLOT, DIAMOND, 3).await;
    place(&bob, spot, 0.0, 0.0).await;
    place(&alice, spot, 0.0, DOWN).await;
    let start = current_tick(&server).await;

    // Alice drops her three diamonds at her feet and walks away.
    drop_key(&alice, true).await;
    alice.move_to(arena.at(-6.5, 6.5)).await;
    let id = expect_item(&bob, DIAMOND, 3).await.id;
    expect_item(&alice, DIAMOND, 3).await;

    // After the delay Bob takes the one diamond that fits; two stay.
    for viewer in [&bob, &alice] {
        assert_eq!(expect_take(viewer, id).await, (bob.entity_id(), 1));
        expect_item_metadata(viewer, id, DIAMOND, 2).await;
    }
    expect_slot(&bob, 9, DIAMOND, 64).await;
    let rest_at = [spot.x, arena.ground(), spot.z];
    assert_eq!(item_state(&server, id).await, Some((rest_at, 2)));

    // Standing on it with a full inventory takes nothing more.
    server.wait_ticks(20).await;
    for viewer in [&bob, &alice] {
        viewer.sync().await;
        assert!(
            queued_about(viewer, id).is_empty(),
            "{}: {:?}",
            viewer.name(),
            queued_about(viewer, id)
        );
    }
    assert_eq!(item_state(&server, id).await, Some((rest_at, 2)));
    assert_eq!(holding(&server, &bob, DIAMOND).await, 64);

    // Carol joins, is shown two diamonds, and walks onto them.
    let carol = server.join("Carol").await;
    assert_eq!(expect_item(&carol, DIAMOND, 2).await.id, id);
    place(&carol, spot, 0.0, 0.0).await;
    for viewer in [&alice, &bob, &carol] {
        assert_eq!(expect_take(viewer, id).await, (carol.entity_id(), 2));
        expect_removed(viewer, id).await;
    }
    expect_slot(&carol, FIRST_HOTBAR_SLOT, DIAMOND, 2).await;
    assert_eq!(holding(&server, &carol, DIAMOND).await, 2);
    assert_eq!(holding(&server, &bob, DIAMOND).await, 64);
    assert_eq!(item_state(&server, id).await, None);
    assert_diamonds_conserved(&server, start, 66).await;

    alice.assert_healthy();
    bob.assert_healthy();
    carol.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn thrown_items_never_clip_through_the_floor_or_a_wall() {
    let server = recorded_server().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let arena = build_arena(&server, &alice).await;
    let ground = arena.ground();
    place(&bob, arena.at(6.5, -6.5), 0.0, 0.0).await;
    set_slot(&server, &alice, FIRST_HOTBAR_SLOT, DIAMOND, 2).await;

    // Against the wall: Alice faces it from a step away, at the height of
    // her throw, then climbs the tower.
    place(&alice, arena.at(5.6, 4.5), EAST, 0.0).await;
    drop_key(&alice, false).await;
    let at_wall = expect_item(&bob, DIAMOND, 1).await.id;

    // From the tower, ten blocks above the floor.
    place(&alice, arena.tower_top(), EAST, 0.0).await;
    drop_key(&alice, false).await;
    let from_tower = expect_item(&bob, DIAMOND, 1).await.id;

    let wall_rest = rest(&server, at_wall).await;
    let tower_rest = rest(&server, from_tower).await;
    let rested = current_tick(&server).await;
    wait_for_tick(&server, rested + 20).await;

    let wall_trace = samples(&server, at_wall).await;
    assert_fell_cleanly(&wall_trace, ground, wall_rest);
    assert_eq!(wall_rest[1], ground);
    assert!(wall_rest[0] + ITEM_HALF_WIDTH <= arena.wall_face());

    let tower_trace = samples(&server, from_tower).await;
    assert_fell_cleanly(&tower_trace, ground, tower_rest);
    assert_eq!(tower_rest[1], ground);
    let highest = tower_trace
        .iter()
        .map(|sample| sample.position[1])
        .fold(f64::MIN, f64::max);
    assert!(
        highest - ground > 10.0,
        "fell from the tower: {tower_trace:?}"
    );
    assert!(
        tower_rest[0] > arena.tower_top().x + 1.0,
        "flew off the tower: {tower_rest:?}"
    );

    for bot in [&alice, &bob] {
        bot.sync().await;
        assert_seen_at(bot, at_wall, wall_rest);
        assert_seen_at(bot, from_tower, tower_rest);
    }
    assert!(wall_trace.iter().all(|sample| sample.count == 1));

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}
