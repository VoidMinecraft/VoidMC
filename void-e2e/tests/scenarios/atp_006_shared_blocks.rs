//! VMC-ATP-006: block breaking and placing change the authoritative world
//! and reach every player that holds the chunk.
//!
//! `world::block_changes_reach_other_players_and_late_joiners` already covers
//! the break and placement broadcast, their acknowledgements and a late
//! joiner. These scenarios add the second player filling the cleared block
//! with the selected item, placement against every face, the convergence of
//! interleaved changes, a change made out of a player's view, invalid
//! interactions, and a placement that brings a section past 256 block states.
//!
//! The default placement follows vanilla: it only fills a replaceable block
//! (into the clicked block itself when that one is replaceable), never one an
//! entity stands in, never out of reach, takes one item outside Creative, and
//! orients the block from the clicked face, the cursor and the player's
//! rotation.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use azalea_entity::LookDirection;
use bevy_ecs::prelude::On;
use voidmc::events::{PlayerStartDiggingEvent, PlayerUseItemOnBlockEvent};
use voidmc::world::{
    ChunkData, ChunkIndex, ChunkPos as ServerChunkPos, DimensionId, set_block_state,
};
use voidmc::{BlockPosition, Inventory, ItemStack as ServerItemStack};
use voidmc_e2e::block::{BlockState, BlockTrait};
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, ChunkPos, Vec3};
use voidmc_e2e::inventory::components::EquipmentSlot;
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::common::movements::MoveFlags;
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_interact::InteractionHand,
    s_move_player_rot::ServerboundMovePlayerRot,
    s_player_action::{Action, ServerboundPlayerAction},
    s_set_carried_item::ServerboundSetCarriedItem,
    s_set_creative_mode_slot::ServerboundSetCreativeModeSlot,
    s_use_item::ServerboundUseItem,
    s_use_item_on::{BlockHit, ServerboundUseItemOn},
};
use voidmc_e2e::registry::builtin::{BlockKind, EntityKind, ItemKind};
use voidmc_e2e::{Bot, BotOptions, TestServer};

use crate::{feet, ground, wait_for_equipment};

const HOTBAR_START: u16 = 36;
const OFF_HAND: u16 = 45;

fn kind(state: Option<BlockState>) -> Option<BlockKind> {
    state.map(BlockKind::from)
}

fn state_id(state: Option<BlockState>) -> Option<i32> {
    state.map(|state| u32::from(state) as i32)
}

/// Every block of the box between two corners, both included.
fn cuboid(a: BlockPos, b: BlockPos) -> Vec<BlockPos> {
    let mut positions = Vec::new();
    for x in a.x.min(b.x)..=a.x.max(b.x) {
        for y in a.y.min(b.y)..=a.y.max(b.y) {
            for z in a.z.min(b.z)..=a.z.max(b.z) {
                positions.push(BlockPos::new(x, y, z));
            }
        }
    }
    positions
}

/// The block states the server holds, `None` where it holds no chunk or the
/// position is outside the world height.
async fn server_blocks(server: &TestServer, positions: Vec<BlockPos>) -> Vec<Option<i32>> {
    server
        .with_world(move |world| {
            positions
                .iter()
                .map(|pos| {
                    let chunk = ServerChunkPos::new(pos.x.div_euclid(16), pos.z.div_euclid(16));
                    let entity = *world
                        .resource::<ChunkIndex>()
                        .0
                        .get(&(DimensionId::Overworld, chunk))?;
                    world.get::<ChunkData>(entity)?.get_block(
                        pos.x.rem_euclid(16) as u8,
                        pos.y,
                        pos.z.rem_euclid(16) as u8,
                    )
                })
                .collect()
        })
        .await
}

async fn server_block(server: &TestServer, pos: BlockPos) -> Option<i32> {
    server_blocks(server, vec![pos]).await[0]
}

async fn server_holds_chunk(server: &TestServer, pos: BlockPos) -> bool {
    server
        .with_world(move |world| {
            let chunk = ServerChunkPos::new(pos.x.div_euclid(16), pos.z.div_euclid(16));
            world
                .resource::<ChunkIndex>()
                .0
                .contains_key(&(DimensionId::Overworld, chunk))
        })
        .await
}

/// Every bot and the server hold the same block state at every position.
async fn assert_converged(server: &TestServer, bots: &[&Bot], positions: &[BlockPos]) {
    let expected = server_blocks(server, positions.to_vec()).await;
    for bot in bots {
        for (pos, expected) in positions.iter().zip(&expected) {
            assert!(expected.is_some(), "the server holds no block at {pos:?}");
            assert_eq!(
                state_id(bot.block_at(*pos)),
                *expected,
                "{} and the server disagree at {pos:?}",
                bot.name()
            );
        }
    }
}

/// Returns once the server has sent every bot what the packets of every bot
/// sent before the call caused.
async fn settle(bots: &[&Bot]) {
    for _ in 0..2 {
        for bot in bots {
            bot.sync().await;
        }
    }
}

async fn hold(bot: &Bot, slot: u16, item: Option<ItemKind>) {
    bot.send(ServerboundSetCreativeModeSlot {
        slot_num: slot,
        item_stack: match item {
            Some(item) => ItemStack::Present(ItemStackData::new(item, 1)),
            None => ItemStack::Empty,
        },
    })
    .await;
}

async fn break_block(bot: &Bot, pos: BlockPos, seq: u32) {
    bot.send(ServerboundPlayerAction {
        action: Action::StartDestroyBlock,
        pos,
        direction: Direction::Up,
        seq,
    })
    .await;
}

fn hit(clicked: BlockPos, face: Direction) -> BlockHit {
    let normal = face.normal();
    BlockHit {
        block_pos: clicked,
        direction: face,
        location: Vec3::new(
            f64::from(clicked.x) + 0.5 + f64::from(normal.x) * 0.5,
            f64::from(clicked.y) + 0.5 + f64::from(normal.y) * 0.5,
            f64::from(clicked.z) + 0.5 + f64::from(normal.z) * 0.5,
        ),
        inside: false,
        world_border: false,
    }
}

async fn use_on(bot: &Bot, hand: InteractionHand, block_hit: BlockHit, seq: u32) {
    bot.send(ServerboundUseItemOn {
        hand,
        block_hit,
        seq,
    })
    .await;
}

async fn place(bot: &Bot, clicked: BlockPos, face: Direction, seq: u32) {
    use_on(bot, InteractionHand::MainHand, hit(clicked, face), seq).await;
}

async fn expect_ack(bot: &Bot, seq: u32) {
    bot.expect(
        &format!("the acknowledgement of sequence {seq}"),
        |packet| match packet {
            ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == seq => Some(()),
            _ => None,
        },
    )
    .await;
}

/// The queued block updates for any of `positions`.
fn updates_at(bot: &Bot, positions: &HashSet<BlockPos>) -> Vec<BlockPos> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::BlockUpdate(update) if positions.contains(&update.pos) => {
            Some(update.pos)
        }
        _ => None,
    })
}

#[tokio::test]
async fn a_block_broken_by_one_player_is_refilled_by_the_other() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    let target = ground(&alice);
    let below = target.down(1);
    let around = cuboid(
        target.down(1).west(1).north(1),
        target.up(1).east(1).south(1),
    );
    assert!(bob.block_at(target).is_some_and(|state| !state.is_air()));
    assert!(bob.block_at(below).is_some_and(|state| !state.is_air()));
    assert_converged(&server, &[&alice, &bob], &around).await;

    // Step 1-2: Alice breaks a visible block; both players and the server lose it.
    break_block(&alice, target, 7).await;
    expect_ack(&alice, 7).await;
    for bot in [&alice, &bob] {
        bot.wait_until("the broken block turn to air", |view| {
            view.block_at(target).is_some_and(|state| state.is_air())
        })
        .await;
    }
    assert_eq!(server_block(&server, target).await, Some(0));

    // Step 3-4: Bob fills the hole with the item of his selected hotbar slot,
    // clicking the top of the block under it.
    hold(&bob, HOTBAR_START, Some(ItemKind::Stone)).await;
    hold(&bob, HOTBAR_START + 4, Some(ItemKind::OakPlanks)).await;
    bob.send(ServerboundSetCarriedItem { slot: 4 }).await;
    place(&bob, below, Direction::Up, 3).await;
    expect_ack(&bob, 3).await;
    for bot in [&alice, &bob] {
        bot.wait_until("oak planks in the cleared position", |view| {
            kind(view.block_at(target)) == Some(BlockKind::OakPlanks)
        })
        .await;
    }
    let stored = server_block(&server, target).await;
    assert_eq!(
        stored
            .and_then(|id| BlockState::try_from(id).ok())
            .map(BlockKind::from),
        Some(BlockKind::OakPlanks),
        "the server stores the placed planks"
    );

    settle(&[&alice, &bob]).await;
    assert_converged(&server, &[&alice, &bob], &around).await;
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn blocks_are_placed_against_the_clicked_face() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // A pillar three blocks above a stone two blocks east of Bob (a block
    // cannot go where he stands), built by clicking top faces.
    let base = ground(&bob).east(2);
    let mut cleared = vec![(base, state_of(BlockKind::Stone, &[]))];
    cleared.extend(
        cuboid(base.up(1).west(1).north(1), base.up(5).east(1).south(1))
            .into_iter()
            .map(|pos| (pos, 0)),
    );
    build(&server, &cleared).await;
    settle(&[&alice, &bob]).await;
    let mut seq = 1;
    hold(&bob, HOTBAR_START, Some(ItemKind::Stone)).await;
    for height in 0..3 {
        place(&bob, base.up(height), Direction::Up, seq).await;
        expect_ack(&bob, seq).await;
        seq += 1;
    }
    let anchor = base.up(3);
    for bot in [&alice, &bob] {
        bot.wait_until("the pillar", |view| {
            kind(view.block_at(anchor)) == Some(BlockKind::Stone)
        })
        .await;
    }

    // One distinct block per face, so that two swapped faces cannot pass.
    let faces = [
        (anchor, Direction::Up, anchor.up(1), ItemKind::OakPlanks),
        (anchor, Direction::North, anchor.north(1), ItemKind::Glass),
        (anchor, Direction::South, anchor.south(1), ItemKind::Bricks),
        (
            anchor,
            Direction::West,
            anchor.west(1),
            ItemKind::Cobblestone,
        ),
        (anchor, Direction::East, anchor.east(1), ItemKind::Sandstone),
        (
            anchor.north(1),
            Direction::Down,
            anchor.north(1).down(1),
            ItemKind::GoldBlock,
        ),
    ];
    for (clicked, face, target, item) in faces {
        assert!(
            bob.block_at(target).is_some_and(|state| state.is_air()),
            "{target:?} is free before Bob clicks {face:?} of {clicked:?}"
        );
        hold(&bob, HOTBAR_START, Some(item)).await;
        place(&bob, clicked, face, seq).await;
        expect_ack(&bob, seq).await;
        seq += 1;
        let expected = item_block(item);
        for bot in [&alice, &bob] {
            bot.wait_until(&format!("{expected:?} on the {face:?} face"), |view| {
                kind(view.block_at(target)) == Some(expected)
            })
            .await;
        }
    }

    settle(&[&alice, &bob]).await;
    let around = cuboid(
        anchor.down(2).west(2).north(2),
        anchor.up(2).east(2).south(2),
    );
    assert_converged(&server, &[&alice, &bob], &around).await;
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

fn item_block(item: ItemKind) -> BlockKind {
    match item {
        ItemKind::OakPlanks => BlockKind::OakPlanks,
        ItemKind::Glass => BlockKind::Glass,
        ItemKind::Bricks => BlockKind::Bricks,
        ItemKind::Cobblestone => BlockKind::Cobblestone,
        ItemKind::Sandstone => BlockKind::Sandstone,
        ItemKind::GoldBlock => BlockKind::GoldBlock,
        ItemKind::Stone => BlockKind::Stone,
        other => panic!("no block for {other:?}"),
    }
}

#[tokio::test]
async fn a_block_held_in_the_off_hand_is_placed_when_the_main_hand_is_empty() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // A stone two blocks east of Bob, to place on.
    let base = ground(&bob).east(2);
    let target = base.up(1);
    build(
        &server,
        &[(base, state_of(BlockKind::Stone, &[])), (target, 0)],
    )
    .await;
    settle(&[&alice, &bob]).await;
    assert!(bob.block_at(target).is_some_and(|state| state.is_air()));

    // Like the vanilla client: the main hand passes, then the off hand is tried.
    hold(&bob, HOTBAR_START, None).await;
    hold(&bob, OFF_HAND, Some(ItemKind::Bricks)).await;
    use_on(&bob, InteractionHand::MainHand, hit(base, Direction::Up), 1).await;
    use_on(&bob, InteractionHand::OffHand, hit(base, Direction::Up), 2).await;
    expect_ack(&bob, 1).await;
    expect_ack(&bob, 2).await;
    for bot in [&alice, &bob] {
        bot.wait_until("the bricks from the off hand", |view| {
            kind(view.block_at(target)) == Some(BlockKind::Bricks)
        })
        .await;
    }

    settle(&[&alice, &bob]).await;
    assert_converged(&server, &[&alice, &bob], &[base, target]).await;
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_click_with_no_block_in_hand_leaves_the_neighbouring_block() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // Two clicked blocks whose neighbour across the top face is not air: a
    // stone (as a lagging client view sees it) and short grass (which a
    // vanilla client lets the player click through). Both are two blocks
    // from Bob: a block cannot go where he stands.
    let below_bob = ground(&bob);
    let base = below_bob.west(2);
    build(
        &server,
        &[(base, state_of(BlockKind::Stone, &[])), (base.up(1), 0)],
    )
    .await;
    let other = bob
        .view(|view| {
            view.ground_below(BlockPos::new(below_bob.x + 2, below_bob.y + 8, below_bob.z))
        })
        .expect("Bob holds the chunk next to him");
    let mut seq = 1;
    for (clicked, item) in [(base, ItemKind::Stone), (other, ItemKind::ShortGrass)] {
        hold(&bob, HOTBAR_START, Some(item)).await;
        place(&bob, clicked, Direction::Up, seq).await;
        expect_ack(&bob, seq).await;
        seq += 1;
    }
    let neighbours = [base.up(1), other.up(1)];
    settle(&[&alice, &bob]).await;
    let before = server_blocks(&server, neighbours.to_vec()).await;
    assert_eq!(
        neighbours.map(|pos| kind(bob.block_at(pos))),
        [Some(BlockKind::Stone), Some(BlockKind::ShortGrass)]
    );
    alice.clear();
    bob.clear();

    hold(&bob, OFF_HAND, None).await;
    for main_hand in [None, Some(ItemKind::Diamond)] {
        hold(&bob, HOTBAR_START, main_hand).await;
        for clicked in [base, other] {
            // Like the vanilla client: the main hand on the block, a use in
            // the air if it holds an item, then the off hand on the block.
            use_on(
                &bob,
                InteractionHand::MainHand,
                hit(clicked, Direction::Up),
                seq,
            )
            .await;
            let mut sent = vec![seq];
            seq += 1;
            if main_hand.is_some() {
                bob.send(ServerboundUseItem {
                    hand: InteractionHand::MainHand,
                    seq,
                    y_rot: 0.0,
                    x_rot: 90.0,
                })
                .await;
                sent.push(seq);
                seq += 1;
            }
            use_on(
                &bob,
                InteractionHand::OffHand,
                hit(clicked, Direction::Up),
                seq,
            )
            .await;
            sent.push(seq);
            seq += 1;
            for s in sent {
                expect_ack(&bob, s).await;
            }
        }
    }

    settle(&[&alice, &bob]).await;
    assert_eq!(
        server_blocks(&server, neighbours.to_vec()).await,
        before,
        "an empty hand or a non-block item places nothing"
    );
    let watched = HashSet::from(neighbours);
    for bot in [&alice, &bob] {
        assert_eq!(updates_at(bot, &watched), [], "{} saw a change", bot.name());
    }
    assert_converged(&server, &[&alice, &bob], &neighbours).await;
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn interleaved_changes_by_two_players_converge() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // The ground on both sides of the players, two blocks or more from the
    // block they stand in: a player's box is 0.6 wide, so one standing near
    // an edge of its block (the spawn is on a block corner) also reaches into
    // the next column, where Alice's planks could not go above Bob's stone.
    let centre = feet(&alice);
    assert_eq!(feet(&bob), centre, "Bob stands in Alice's block");
    let columns: Vec<BlockPos> = [-4, -3, -2, 2, 3, 4]
        .into_iter()
        .map(|dx| {
            alice
                .view(|view| {
                    view.ground_below(BlockPos::new(centre.x + dx, centre.y + 8, centre.z))
                })
                .expect("Alice holds the ground around her")
        })
        .collect();
    hold(&bob, HOTBAR_START, Some(ItemKind::Stone)).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::OakPlanks)).await;
    alice.sync().await;
    bob.sync().await;

    // Both players change the same blocks without waiting for each other:
    // Alice breaks each ground block and clicks its top with planks, Bob
    // clicks the block below with stone, before Alice on even columns and
    // after her on odd ones; the server may still handle Bob's click between
    // Alice's two packets. A click whose target is already filled is
    // refused, so the planks end up in the hole, or on Bob's stone when it
    // filled the hole between Alice's break and her click.
    for (index, ground) in columns.iter().enumerate() {
        let (alice_seq, bob_seq) = (100 + 2 * index as u32, 200 + index as u32);
        if index % 2 == 0 {
            place(&bob, ground.down(1), Direction::Up, bob_seq).await;
        }
        break_block(&alice, *ground, alice_seq).await;
        place(&alice, *ground, Direction::Up, alice_seq + 1).await;
        if index % 2 == 1 {
            place(&bob, ground.down(1), Direction::Up, bob_seq).await;
        }
    }
    for index in 0..columns.len() as u32 {
        expect_ack(&alice, 100 + 2 * index).await;
        expect_ack(&alice, 101 + 2 * index).await;
        expect_ack(&bob, 200 + index).await;
    }
    settle(&[&alice, &bob]).await;
    for ground in &columns {
        let state = kind(alice.block_at(*ground));
        let above = kind(alice.block_at(ground.up(1)));
        assert!(
            state == Some(BlockKind::OakPlanks)
                || state == Some(BlockKind::Stone) && above == Some(BlockKind::OakPlanks),
            "{ground:?} holds Alice's planks, or Bob's stone under them, not {state:?} under {above:?}"
        );
    }

    // Then both clear the columns at once: Alice the block above the ground,
    // Bob the ground.
    let alice_turn = async {
        for (index, ground) in columns.iter().enumerate() {
            break_block(&alice, ground.up(1), 300 + index as u32).await;
        }
    };
    let bob_turn = async {
        for (index, ground) in columns.iter().enumerate() {
            break_block(&bob, *ground, 400 + index as u32).await;
        }
    };
    tokio::join!(alice_turn, bob_turn);
    for index in 0..columns.len() as u32 {
        expect_ack(&alice, 300 + index).await;
        expect_ack(&bob, 400 + index).await;
    }
    settle(&[&alice, &bob]).await;
    for ground in &columns {
        for pos in [*ground, ground.up(1)] {
            assert!(
                alice.block_at(pos).is_some_and(|state| state.is_air()),
                "{pos:?} was cleared"
            );
        }
    }

    let low = columns.iter().map(|pos| pos.y).min().unwrap();
    let high = columns.iter().map(|pos| pos.y).max().unwrap();
    let area = cuboid(
        BlockPos::new(centre.x - 5, low - 2, centre.z - 1),
        BlockPos::new(centre.x + 5, high + 2, centre.z + 1),
    );
    assert_converged(&server, &[&alice, &bob], &area).await;

    let carol = server.join("Carol").await;
    assert_converged(&server, &[&carol], &area).await;
    for bot in [&alice, &bob, &carol] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_change_made_out_of_view_is_seen_on_return() {
    let server = TestServer::start().await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 2 })
        .await;
    let bob = server.join("Bob").await;

    let start = alice.position();
    let broken = ground(&bob);
    let placed = bob
        .view(|view| view.ground_below(BlockPos::new(broken.x + 1, broken.y + 8, broken.z)))
        .expect("Bob holds the ground next to him")
        .up(1);
    let watched = ChunkPos::from(broken);
    let around = cuboid(
        broken.down(1).west(1).north(1),
        placed.up(1).east(1).south(1),
    );
    assert!(alice.view(|view| view.chunks.contains_key(&watched)));

    // Alice walks three chunks east, out of range of the watched chunk.
    let mut position = start;
    for _ in 0..6 {
        position = Vec3::new(position.x + 8.0, position.y, position.z);
        alice.move_to(position).await;
    }
    alice
        .wait_until("the watched chunk to be forgotten", |view| {
            !view.chunks.contains_key(&watched)
        })
        .await;
    alice.sync().await;
    alice.clear();

    // Bob changes two blocks of it while Alice is away.
    hold(&bob, HOTBAR_START, Some(ItemKind::Glass)).await;
    break_block(&bob, broken, 1).await;
    place(&bob, placed.down(1), Direction::Up, 2).await;
    expect_ack(&bob, 1).await;
    expect_ack(&bob, 2).await;
    bob.wait_until("both changes", |view| {
        view.block_at(broken).is_some_and(|state| state.is_air())
            && kind(view.block_at(placed)) == Some(BlockKind::Glass)
    })
    .await;
    settle(&[&alice, &bob]).await;
    assert!(
        updates_at(&alice, &HashSet::from([broken, placed])).is_empty(),
        "Alice got block updates for a chunk she does not hold"
    );

    // Alice walks back and receives the chunk with both changes.
    for _ in 0..6 {
        position = Vec3::new(position.x - 8.0, position.y, position.z);
        alice.move_to(position).await;
    }
    alice
        .wait_until("the watched chunk again", |view| {
            view.chunks.contains_key(&watched)
        })
        .await;
    assert!(
        alice.block_at(broken).is_some_and(|state| state.is_air()),
        "the re-sent chunk keeps the broken block"
    );
    assert_eq!(
        kind(alice.block_at(placed)),
        Some(BlockKind::Glass),
        "the re-sent chunk keeps the placed block"
    );
    settle(&[&alice, &bob]).await;
    assert_converged(&server, &[&alice, &bob], &around).await;

    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn invalid_interactions_leave_the_server_up_and_the_world_consistent() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    let base = ground(&alice);
    let air = base.up(3);
    let far = BlockPos::new(1_000_000, 64, 1_000_000);
    let edge = BlockPos::new(33_554_431, 64, 0);
    assert!(alice.block_at(air).is_some_and(|state| state.is_air()));
    assert!(!server_holds_chunk(&server, far).await);
    let reach = bob
        .view(|view| view.ground_below(BlockPos::new(base.x + 40, base.y + 8, base.z)))
        .expect("Bob holds the ground 40 blocks away");
    hold(&alice, HOTBAR_START, Some(ItemKind::Stone)).await;
    settle(&[&alice, &bob]).await;
    alice.clear();
    bob.clear();

    // Interactions that must change nothing.
    let mut untouched = HashSet::new();
    let mut seqs = Vec::new();
    let mut seq = 0u32;
    let mut next = || {
        seq += 1;
        seq
    };

    // Breaking air, with a face that does not face the player.
    let s = next();
    alice
        .send(ServerboundPlayerAction {
            action: Action::StartDestroyBlock,
            pos: air,
            direction: Direction::Down,
            seq: s,
        })
        .await;
    seqs.push(s);
    untouched.insert(air);

    // Breaking and placing above and below the world height.
    for y in [320, 2047, -65, -2048] {
        let pos = BlockPos::new(base.x, y, base.z);
        let s = next();
        break_block(&alice, pos, s).await;
        seqs.push(s);
        untouched.insert(pos);
    }
    let s = next();
    place(&alice, BlockPos::new(base.x, 319, base.z), Direction::Up, s).await;
    seqs.push(s);
    untouched.insert(BlockPos::new(base.x, 320, base.z));
    let s = next();
    place(
        &alice,
        BlockPos::new(base.x, -64, base.z),
        Direction::Down,
        s,
    )
    .await;
    seqs.push(s);
    untouched.insert(BlockPos::new(base.x, -65, base.z));

    // Breaking and placing in chunks the server never generated.
    for (clicked, face) in [(far, Direction::Up), (edge, Direction::East)] {
        let s = next();
        break_block(&alice, clicked, s).await;
        seqs.push(s);
        let s = next();
        place(&alice, clicked, face, s).await;
        seqs.push(s);
        untouched.insert(clicked);
        untouched.insert(clicked.offset_with_direction(face));
    }

    // Using a block item in the air, an empty hand and items that are not blocks.
    let s = next();
    alice
        .send(ServerboundUseItem {
            hand: InteractionHand::MainHand,
            seq: s,
            y_rot: 0.0,
            x_rot: 90.0,
        })
        .await;
    seqs.push(s);
    for item in [None, Some(ItemKind::Stick), Some(ItemKind::DiamondSword)] {
        hold(&alice, HOTBAR_START, item).await;
        let s = next();
        place(&alice, base, Direction::Up, s).await;
        seqs.push(s);
    }
    untouched.insert(base.up(1));

    // A sequence number the client never used.
    break_block(&alice, air, u32::MAX).await;
    seqs.push(u32::MAX);

    for s in &seqs {
        expect_ack(&alice, *s).await;
    }
    settle(&[&alice, &bob]).await;
    for bot in [&alice, &bob] {
        assert_eq!(
            updates_at(bot, &untouched),
            Vec::<BlockPos>::new(),
            "{} got block updates for interactions that change nothing",
            bot.name()
        );
    }
    assert!(
        !server_holds_chunk(&server, far).await,
        "an interaction generated a far chunk"
    );
    assert!(alice.block_at(air).is_some_and(|state| state.is_air()));
    assert!(
        alice
            .block_at(base.up(1))
            .is_some_and(|state| state.is_air())
    );

    // Interactions a vanilla server refuses, which change nothing and leave
    // every player with the real blocks: placing into an occupied block,
    // into the block the player stands in (with a cursor outside the block),
    // and breaking far out of reach.
    let mut checked = cuboid(base.down(1), base.up(1));
    checked.push(reach);
    let before = server_blocks(&server, checked.clone()).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::Glass)).await;
    let occupied = 900;
    place(&alice, base.down(1), Direction::Up, occupied).await;
    let odd_cursor = 901;
    use_on(
        &alice,
        InteractionHand::MainHand,
        BlockHit {
            block_pos: base,
            direction: Direction::Up,
            location: Vec3 {
                x: f64::NAN,
                y: f64::INFINITY,
                z: -1.0e30,
            },
            inside: true,
            world_border: true,
        },
        odd_cursor,
    )
    .await;
    let out_of_reach = 902;
    break_block(&alice, reach, out_of_reach).await;
    for s in [occupied, odd_cursor, out_of_reach] {
        expect_ack(&alice, s).await;
    }
    settle(&[&alice, &bob]).await;
    assert_eq!(
        server_blocks(&server, checked.clone()).await,
        before,
        "refused interactions change nothing"
    );
    assert_converged(&server, &[&alice, &bob], &checked).await;

    // The server still applies and shares valid changes.
    let side = base.east(2);
    build(
        &server,
        &[(side, state_of(BlockKind::Stone, &[])), (side.up(1), 0)],
    )
    .await;
    let valid = side.up(1);
    place(&alice, side, Direction::Up, 1000).await;
    expect_ack(&alice, 1000).await;
    bob.wait_until("a valid placement after the invalid ones", |view| {
        kind(view.block_at(valid)) == Some(BlockKind::Glass)
    })
    .await;

    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

/// Every cell of `section` holds its `expected` state for `bot`.
fn assert_section_seen(bot: &Bot, section: &[BlockPos], expected: &HashMap<BlockPos, i32>) {
    for pos in section {
        assert_eq!(
            state_id(bot.block_at(*pos)),
            Some(expected.get(pos).copied().unwrap_or(0)),
            "{} sees the wrong block at {pos:?}",
            bot.name()
        );
    }
}

#[tokio::test]
async fn a_placement_bringing_a_section_past_256_states_keeps_every_block() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // An empty section above the players, in the chunk they stand in.
    let feet = feet(&alice);
    let base = BlockPos::new(
        feet.x.div_euclid(16) * 16,
        (feet.y.div_euclid(16) + 2) * 16,
        feet.z.div_euclid(16) * 16,
    );
    let section = cuboid(base, BlockPos::new(base.x + 15, base.y + 15, base.z + 15));
    assert!(
        server_blocks(&server, section.clone())
            .await
            .iter()
            .all(|state| *state == Some(0)),
        "the section above the players starts empty"
    );

    // Air plus 255 distinct states on its bottom layer: 256 states, the most
    // an 8-bit palette holds. They are states 2 to 256, granite at the
    // section's corner; stone (state 1) is kept for the placement.
    let mut expected = HashMap::new();
    for index in 0..255 {
        let pos = BlockPos::new(base.x + index % 16, base.y, base.z + index / 16);
        expected.insert(pos, 2 + index);
    }
    let changes: Vec<(BlockPos, i32)> = expected.iter().map(|(pos, id)| (*pos, *id)).collect();
    server
        .with_world(move |world| {
            for (pos, id) in changes {
                let position = BlockPosition {
                    x: pos.x,
                    y: pos.y as i16,
                    z: pos.z,
                };
                set_block_state(world, DimensionId::Overworld, position, id, None);
            }
        })
        .await;
    settle(&[&alice, &bob]).await;
    for bot in [&alice, &bob] {
        assert_section_seen(bot, &section, &expected);
    }

    // Alice puts stone on the granite: the section's 257th state. Stone, not
    // a block with a higher id: Azalea grows a palette past 8 bits to 9-bit
    // global ids on a block update, so it panics on any id from 512 up.
    // Alice first moves under the section, within reach of the granite.
    let target = base.up(1);
    alice
        .move_to(Vec3::new(
            f64::from(base.x) + 2.5,
            f64::from(base.y - 2),
            f64::from(base.z) + 0.5,
        ))
        .await;
    hold(&alice, HOTBAR_START, Some(ItemKind::Stone)).await;
    place(&alice, base, Direction::Up, 1).await;
    expect_ack(&alice, 1).await;
    for bot in [&alice, &bob] {
        bot.wait_until("the stone on the granite", |view| {
            kind(view.block_at(target)) == Some(BlockKind::Stone)
        })
        .await;
    }
    let stone = state_id(bob.block_at(target)).expect("Bob holds the section");
    expected.insert(target, stone);
    let states: HashSet<i32> = expected.values().copied().chain([0]).collect();
    assert_eq!(
        states.len(),
        257,
        "the stone is a new state for the section"
    );

    settle(&[&alice, &bob]).await;
    let stored = server_blocks(&server, section.clone()).await;
    for (pos, stored) in section.iter().zip(stored) {
        assert_eq!(
            stored,
            Some(expected.get(pos).copied().unwrap_or(0)),
            "the server stores the wrong block at {pos:?}"
        );
    }
    for bot in [&alice, &bob] {
        assert_section_seen(bot, &section, &expected);
    }

    // A player who joins afterwards gets the section in the chunk data.
    let carol = server.join("Carol").await;
    assert_section_seen(&carol, &section, &expected);

    for bot in [&alice, &bob, &carol] {
        bot.assert_healthy();
    }
    server.stop().await;
}

// ---------------------------------------------------------------------------
// The vanilla placement rules: what may be replaced, entities in the way,
// reach, the item used up, and the placed block's orientation. Each test
// builds what it clicks on in the air above the spawn and stands Alice next
// to it.
// ---------------------------------------------------------------------------

/// A block-state id by the client's (Azalea's) numbering, which is the
/// server's: `kind` with the listed properties, the others at their default.
fn state_of(kind: BlockKind, properties: &[(&str, &str)]) -> i32 {
    let mut block: Box<dyn BlockTrait> = kind.into();
    for (name, value) in properties {
        block
            .set_property(name, value)
            .unwrap_or_else(|_| panic!("{kind:?} has no {name}={value}"));
    }
    u32::from(block.as_block_state()) as i32
}

/// `id[property=value,...]` with sorted properties, `nothing` outside the
/// chunks held.
fn describe(state: Option<BlockState>) -> String {
    let Some(state) = state else {
        return "nothing".to_string();
    };
    let block: Box<dyn BlockTrait> = state.into();
    let mut properties: Vec<String> = block
        .property_map()
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    properties.sort();
    format!("{}[{}]", block.id(), properties.join(","))
}

/// Whether `state` is a `kind` with every listed property value.
fn is_block(state: Option<BlockState>, kind: BlockKind, properties: &[(&str, &str)]) -> bool {
    let Some(state) = state else {
        return false;
    };
    let block: Box<dyn BlockTrait> = state.into();
    BlockKind::from(state) == kind
        && properties
            .iter()
            .all(|(name, value)| block.get_property(name) == Some(*value))
}

fn to_server(pos: BlockPos) -> BlockPosition {
    BlockPosition {
        x: pos.x,
        y: pos.y as i16,
        z: pos.z,
    }
}

/// Sets blocks on the server like a plugin does; the players holding the
/// chunk are sent them.
async fn build(server: &TestServer, blocks: &[(BlockPos, i32)]) {
    let blocks = blocks.to_vec();
    server
        .with_world(move |world| {
            for (pos, state) in blocks {
                set_block_state(world, DimensionId::Overworld, to_server(pos), state, None);
            }
        })
        .await;
}

/// A spot well above the spawn, where a test builds what it clicks on.
fn sky(bot: &Bot) -> BlockPos {
    feet(bot).up(12)
}

/// The middle of the bottom of block `pos`, moved `dx` blocks along X:
/// where a player standing on the block below `pos` has its feet.
fn feet_at(pos: BlockPos, dx: f64) -> Vec3 {
    Vec3::new(
        f64::from(pos.x) + 0.5 + dx,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    )
}

async fn look(bot: &Bot, yaw: f32, pitch: f32) {
    bot.send(ServerboundMovePlayerRot {
        look_direction: LookDirection::new(yaw, pitch),
        flags: MoveFlags {
            on_ground: true,
            horizontal_collision: false,
        },
    })
    .await;
}

/// Moves `bot` to `at` (no physics) and turns it to `yaw` and `pitch`.
async fn stand(bot: &Bot, at: Vec3, yaw: f32, pitch: f32) {
    bot.move_to(at).await;
    look(bot, yaw, pitch).await;
}

/// The yaw of a player looking east, south or north.
const EAST: f32 = -90.0;
const SOUTH: f32 = 0.0;
const NORTH: f32 = 180.0;

/// A click on `face` of `clicked` at `cursor`, in block coordinates.
fn hit_at(clicked: BlockPos, face: Direction, cursor: (f64, f64, f64)) -> BlockHit {
    BlockHit {
        block_pos: clicked,
        direction: face,
        location: Vec3::new(
            f64::from(clicked.x) + cursor.0,
            f64::from(clicked.y) + cursor.1,
            f64::from(clicked.z) + cursor.2,
        ),
        inside: false,
        world_border: false,
    }
}

/// A block update or an acknowledgement queued for a bot.
#[derive(Debug, PartialEq)]
enum Traffic {
    Update(BlockPos, i32),
    Ack(u32),
}

/// The block updates and acknowledgements queued for `bot`, in order.
fn block_traffic(bot: &Bot) -> Vec<Traffic> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::BlockUpdate(update) => Some(Traffic::Update(
            update.pos,
            u32::from(update.block_state) as i32,
        )),
        ClientboundGamePacket::BlockChangedAck(ack) => Some(Traffic::Ack(ack.seq)),
        _ => None,
    })
}

/// Waits until every bot sees `pos` as `kind` with `properties`, then checks
/// that the server stores the state the first bot sees.
async fn expect_placed(
    server: &TestServer,
    bots: &[&Bot],
    pos: BlockPos,
    kind: BlockKind,
    properties: &[(&str, &str)],
) {
    for bot in bots {
        let reached = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            bot.wait_until(&format!("{kind:?} {properties:?} at {pos:?}"), |view| {
                is_block(view.block_at(pos), kind, properties)
            })
            .await
        })
        .await;
        assert!(
            reached.is_ok(),
            "{} sees {} at {pos:?}, not {kind:?} with {properties:?}",
            bot.name(),
            describe(bot.block_at(pos))
        );
    }
    assert_eq!(
        server_block(server, pos).await,
        state_id(bots[0].block_at(pos)),
        "the server stores what {} sees at {pos:?}",
        bots[0].name()
    );
}

async fn server_slot(server: &TestServer, bot: &Bot, slot: usize) -> ServerItemStack {
    let player = server.player_entity(bot.uuid()).await;
    server
        .with_world(move |world| world.get::<Inventory>(player).unwrap().get(slot).clone())
        .await
}

/// Whether the client shows `count` items in its first hotbar slot.
fn holds(bot: &Bot, count: i32) -> bool {
    bot.view(|view| {
        matches!(
            &view.inventory[HOTBAR_START as usize],
            ItemStack::Present(data) if data.count == count
        )
    })
}

/// Puts `count` of `item` in the first hotbar slot from the server side (a
/// survival player cannot use the creative inventory) and waits for the
/// client to show it.
async fn give_main_hand(server: &TestServer, bot: &Bot, item: &str, count: u8) {
    let player = server.player_entity(bot.uuid()).await;
    let stack = ServerItemStack::of(item, count).unwrap();
    server
        .with_world(move |world| {
            world
                .get_mut::<Inventory>(player)
                .unwrap()
                .set(Inventory::hotbar_slot_index(0), stack);
        })
        .await;
    bot.sync().await;
    assert!(
        holds(bot, i32::from(count)),
        "{} shows the stack",
        bot.name()
    );
}

#[tokio::test]
async fn a_placement_into_a_solid_block_is_refused_and_the_placer_resynced() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    let clicked = sky(&alice);
    let target = clicked.up(1);
    let stone = state_of(BlockKind::Stone, &[]);
    let bricks = state_of(BlockKind::Bricks, &[]);
    build(&server, &[(clicked, stone), (target, bricks)]).await;
    stand(&alice, feet_at(clicked.down(1), -2.0), EAST, 0.0).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::Glass)).await;
    settle(&[&alice, &bob]).await;
    alice.clear();
    bob.clear();

    // Alice clicks the top of the stone while bricks stand on it, as a
    // client that missed the bricks would.
    place(&alice, clicked, Direction::Up, 1).await;
    settle(&[&alice, &bob]).await;

    assert_eq!(
        server_block(&server, target).await,
        Some(bricks),
        "the bricks are not replaced"
    );
    assert_eq!(
        block_traffic(&alice),
        [
            Traffic::Update(clicked, stone),
            Traffic::Update(target, bricks),
            Traffic::Ack(1),
        ],
        "Alice is sent the real blocks, then the acknowledgement"
    );
    assert_eq!(block_traffic(&bob), [], "nothing changed for Bob");
    assert_converged(&server, &[&alice, &bob], &[clicked, target]).await;
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn replaceable_blocks_are_replaced_and_a_replaceable_clicked_block_is_filled() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // Three stones: under short grass, under water, and under short grass
    // that Alice clicks itself.
    let base = sky(&alice);
    let (under_grass, under_water, clicked_grass) = (base, base.south(2), base.north(2).up(1));
    let stone = state_of(BlockKind::Stone, &[]);
    let grass = state_of(BlockKind::ShortGrass, &[]);
    let water = state_of(BlockKind::Water, &[]);
    build(
        &server,
        &[
            (under_grass, stone),
            (under_grass.up(1), grass),
            (under_grass.up(2), 0),
            (under_water, stone),
            (under_water.up(1), water),
            (under_water.up(2), 0),
            (clicked_grass.down(1), stone),
            (clicked_grass, grass),
            (clicked_grass.up(1), 0),
        ],
    )
    .await;
    stand(&alice, feet_at(base.down(1), -2.0), EAST, 0.0).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::Glass)).await;
    settle(&[&alice, &bob]).await;

    place(&alice, under_grass, Direction::Up, 1).await;
    place(&alice, under_water, Direction::Up, 2).await;
    place(&alice, clicked_grass, Direction::Up, 3).await;
    for seq in 1..=3 {
        expect_ack(&alice, seq).await;
    }
    let both = [&alice, &bob];
    expect_placed(&server, &both, under_grass.up(1), BlockKind::Glass, &[]).await;
    expect_placed(&server, &both, under_water.up(1), BlockKind::Glass, &[]).await;
    expect_placed(&server, &both, clicked_grass, BlockKind::Glass, &[]).await;
    settle(&both).await;
    assert_eq!(
        server_block(&server, clicked_grass.up(1)).await,
        Some(0),
        "the glass went into the clicked grass, not above it"
    );
    assert_converged(
        &server,
        &both,
        &[
            under_grass.up(2),
            under_water.up(2),
            clicked_grass.up(1),
            clicked_grass.down(1),
        ],
    )
    .await;
    for bot in both {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_placement_into_a_player_or_a_mob_is_refused() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // A stone floor from x-2 to x+2 and a stone east of Bob's head: Alice
    // stands at the west end of the floor, Bob at the east end, a pig in
    // the middle.
    let floor = sky(&alice);
    let at = |dx: i32, dy: i32| BlockPos::new(floor.x + dx, floor.y + dy, floor.z);
    let stone = state_of(BlockKind::Stone, &[]);
    let mut blocks = vec![(at(3, 2), stone)];
    for dx in -2..=2 {
        blocks.extend([(at(dx, 0), stone), (at(dx, 1), 0), (at(dx, 2), 0)]);
    }
    build(&server, &blocks).await;
    stand(&alice, feet_at(at(-2, 1), 0.0), EAST, 0.0).await;
    stand(&bob, feet_at(at(2, 1), 0.0), NORTH, 0.0).await;
    alice
        .command(&format!(
            "summon minecraft:pig {} {} {}",
            f64::from(floor.x) + 0.5,
            floor.y + 1,
            f64::from(floor.z) + 0.5
        ))
        .await;
    alice
        .wait_until("the pig on the floor", |view| {
            view.entities.values().any(|entity| {
                entity.kind == EntityKind::Pig && BlockPos::from(entity.position) == at(0, 1)
            })
        })
        .await;
    hold(&alice, HOTBAR_START, Some(ItemKind::Glass)).await;
    settle(&[&alice, &bob]).await;
    alice.clear();
    bob.clear();

    // Into Bob's feet and head, Alice's own feet and the pig.
    let refused = [
        (at(2, 0), Direction::Up, at(2, 1)),
        (at(3, 2), Direction::West, at(2, 2)),
        (at(-2, 0), Direction::Up, at(-2, 1)),
        (at(0, 0), Direction::Up, at(0, 1)),
    ];
    let mut expected = Vec::new();
    for (seq, (clicked, face, target)) in (1..).zip(refused) {
        place(&alice, clicked, face, seq).await;
        expected.extend([
            Traffic::Update(clicked, stone),
            Traffic::Update(target, 0),
            Traffic::Ack(seq),
        ]);
    }
    settle(&[&alice, &bob]).await;
    for (_, _, target) in refused {
        assert_eq!(
            server_block(&server, target).await,
            Some(0),
            "nothing is placed into the entity at {target:?}"
        );
    }
    assert_eq!(block_traffic(&alice), expected);
    assert_eq!(block_traffic(&bob), []);

    // A free block next to them takes the glass, and a lever, which has no
    // collision box, goes at Bob's feet.
    place(&alice, at(1, 0), Direction::Up, 10).await;
    expect_ack(&alice, 10).await;
    expect_placed(&server, &[&alice, &bob], at(1, 1), BlockKind::Glass, &[]).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::Lever)).await;
    place(&alice, at(2, 0), Direction::Up, 11).await;
    expect_ack(&alice, 11).await;
    expect_placed(&server, &[&alice, &bob], at(2, 1), BlockKind::Lever, &[]).await;

    settle(&[&alice, &bob]).await;
    let mut area = Vec::new();
    for dx in -2..=3 {
        area.extend([at(dx, 0), at(dx, 1), at(dx, 2)]);
    }
    assert_converged(&server, &[&alice, &bob], &area).await;
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn placing_and_breaking_out_of_reach_is_refused_before_any_event() {
    let uses = Arc::new(AtomicUsize::new(0));
    let digs = Arc::new(AtomicUsize::new(0));
    let (use_count, dig_count) = (uses.clone(), digs.clone());
    let server = TestServer::builder()
        .plugin(move |app| {
            app.add_observer(move |_: On<PlayerUseItemOnBlockEvent>| {
                use_count.fetch_add(1, Ordering::SeqCst);
            });
            app.add_observer(move |_: On<PlayerStartDiggingEvent>| {
                dig_count.fetch_add(1, Ordering::SeqCst);
            });
        })
        .start()
        .await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // A stone seven blocks east, at the height of Alice's eyes. A creative
    // player reaches 5 blocks plus 1, from its eyes to the nearest point of
    // the block: 6.05 blocks is out of reach, 5.95 within.
    let origin = sky(&alice);
    let block = origin.east(7).up(1);
    let stone = state_of(BlockKind::Stone, &[]);
    build(&server, &[(block, stone), (block.up(1), 0)]).await;
    let x = f64::from(origin.x);
    let (y, z) = (f64::from(origin.y), f64::from(origin.z) + 0.5);
    stand(&alice, Vec3::new(x + 0.95, y, z), EAST, 0.0).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::Glass)).await;
    settle(&[&alice, &bob]).await;
    alice.clear();
    bob.clear();

    place(&alice, block, Direction::Up, 1).await;
    break_block(&alice, block, 2).await;
    settle(&[&alice, &bob]).await;
    assert_eq!(server_block(&server, block).await, Some(stone));
    assert_eq!(server_block(&server, block.up(1)).await, Some(0));
    assert_eq!(
        block_traffic(&alice),
        [
            Traffic::Update(block, stone),
            Traffic::Update(block.up(1), 0),
            Traffic::Ack(1),
            Traffic::Update(block, stone),
            Traffic::Ack(2),
        ],
        "both refusals resend the real blocks before the acknowledgement"
    );
    assert_eq!(block_traffic(&bob), []);
    assert_eq!(uses.load(Ordering::SeqCst), 0, "no use event out of reach");
    assert_eq!(digs.load(Ordering::SeqCst), 0, "no dig event out of reach");

    // A tenth of a block closer, both work.
    stand(&alice, Vec3::new(x + 1.05, y, z), EAST, 0.0).await;
    place(&alice, block, Direction::Up, 3).await;
    expect_ack(&alice, 3).await;
    expect_placed(&server, &[&alice, &bob], block.up(1), BlockKind::Glass, &[]).await;
    break_block(&alice, block, 4).await;
    expect_ack(&alice, 4).await;
    expect_placed(&server, &[&alice, &bob], block, BlockKind::Air, &[]).await;
    assert_eq!(uses.load(Ordering::SeqCst), 1);
    assert_eq!(digs.load(Ordering::SeqCst), 1);

    settle(&[&alice, &bob]).await;
    assert_converged(&server, &[&alice, &bob], &[block, block.up(1)]).await;
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_survival_placement_uses_one_item_and_the_last_one_empties_the_hand() {
    let server = TestServer::builder()
        .config(|config| config.game_mode(0))
        .start()
        .await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    let base = sky(&alice);
    let supports = [base, base.south(2), base.north(2)];
    let stone = state_of(BlockKind::Stone, &[]);
    let mut blocks = Vec::new();
    for support in supports {
        blocks.extend([(support, stone), (support.up(1), 0), (support.up(2), 0)]);
    }
    build(&server, &blocks).await;
    stand(&alice, feet_at(base.down(1), -2.0), EAST, 0.0).await;
    give_main_hand(&server, &alice, "minecraft:stone", 3).await;
    wait_for_equipment(
        &bob,
        &alice,
        &[(EquipmentSlot::Mainhand, ItemKind::Stone, 3)],
    )
    .await;
    settle(&[&alice, &bob]).await;
    alice.clear();

    for (seq, support) in (1..).zip(supports) {
        place(&alice, support, Direction::Up, seq).await;
        expect_ack(&alice, seq).await;
        expect_placed(
            &server,
            &[&alice, &bob],
            support.up(1),
            BlockKind::Stone,
            &[],
        )
        .await;
        let left = 3 - seq as u8;
        let held = server_slot(&server, &alice, Inventory::hotbar_slot_index(0)).await;
        if left == 0 {
            assert!(held.is_empty(), "the last stone empties the hand: {held:?}");
            wait_for_equipment(&bob, &alice, &[]).await;
        } else {
            assert_eq!(held.count, left, "one stone per placement");
            wait_for_equipment(
                &bob,
                &alice,
                &[(EquipmentSlot::Mainhand, ItemKind::Stone, i32::from(left))],
            )
            .await;
        }
    }
    settle(&[&alice, &bob]).await;
    let resent: Vec<u16> = alice.queued(|packet| match packet {
        ClientboundGamePacket::ContainerSetSlot(set) => Some(set.slot),
        _ => None,
    });
    assert_eq!(
        resent,
        Vec::<u16>::new(),
        "a client that predicted each placement is not sent its hand again"
    );

    // With the hand empty, a fourth click places nothing.
    place(&alice, supports[0].up(1), Direction::Up, 4).await;
    expect_ack(&alice, 4).await;
    settle(&[&alice, &bob]).await;
    assert_eq!(server_block(&server, supports[0].up(2)).await, Some(0));
    let mut area = Vec::new();
    for support in supports {
        area.extend([support, support.up(1), support.up(2)]);
    }
    assert_converged(&server, &[&alice, &bob], &area).await;
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_refused_survival_placement_uses_nothing() {
    let server = TestServer::builder()
        .config(|config| config.game_mode(0))
        .start()
        .await;
    let alice = server.join("Alice").await;

    let clicked = sky(&alice);
    let stone = state_of(BlockKind::Stone, &[]);
    let bricks = state_of(BlockKind::Bricks, &[]);
    build(&server, &[(clicked, stone), (clicked.up(1), bricks)]).await;
    stand(&alice, feet_at(clicked.down(1), -2.0), EAST, 0.0).await;
    give_main_hand(&server, &alice, "minecraft:stone", 5).await;

    place(&alice, clicked, Direction::Up, 1).await;
    expect_ack(&alice, 1).await;
    alice.sync().await;
    assert_eq!(server_block(&server, clicked.up(1)).await, Some(bricks));
    assert_eq!(
        server_slot(&server, &alice, Inventory::hotbar_slot_index(0))
            .await
            .count,
        5,
        "a refused placement uses nothing"
    );
    assert!(holds(&alice, 5), "Alice is shown her five stone");
    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn a_creative_placement_keeps_the_stack() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;

    let clicked = sky(&alice);
    build(
        &server,
        &[
            (clicked, state_of(BlockKind::Stone, &[])),
            (clicked.up(1), 0),
        ],
    )
    .await;
    stand(&alice, feet_at(clicked.down(1), -2.0), EAST, 0.0).await;
    alice
        .send(ServerboundSetCreativeModeSlot {
            slot_num: HOTBAR_START,
            item_stack: ItemStack::Present(ItemStackData::new(ItemKind::Stone, 5)),
        })
        .await;
    place(&alice, clicked, Direction::Up, 1).await;
    expect_ack(&alice, 1).await;
    expect_placed(&server, &[&alice], clicked.up(1), BlockKind::Stone, &[]).await;
    assert_eq!(
        server_slot(&server, &alice, Inventory::hotbar_slot_index(0))
            .await
            .count,
        5,
        "a creative placement uses nothing"
    );
    alice.assert_healthy();
    server.stop().await;
}

/// A stone in the air with its neighbours cleared, and Alice standing two
/// blocks west of it, one block lower, turned to `yaw` and `pitch`.
async fn orientation_bench(server: &TestServer, alice: &Bot, yaw: f32, pitch: f32) -> BlockPos {
    let support = sky(alice);
    let mut blocks = vec![(support, state_of(BlockKind::Stone, &[]))];
    for face in [
        Direction::Up,
        Direction::Down,
        Direction::North,
        Direction::South,
        Direction::East,
    ] {
        blocks.push((support.offset_with_direction(face), 0));
    }
    blocks.extend([(support.up(2), 0), (support.down(2), 0)]);
    build(server, &blocks).await;
    stand(alice, feet_at(support.down(1), -2.0), yaw, pitch).await;
    support
}

#[tokio::test]
async fn a_lever_attaches_to_the_floor_a_wall_or_the_ceiling_it_is_placed_on() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let support = orientation_bench(&server, &alice, EAST, 0.0).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::Lever)).await;

    let cases = [
        (Direction::Up, [("face", "floor"), ("facing", "east")]),
        (Direction::North, [("face", "wall"), ("facing", "north")]),
        (Direction::Down, [("face", "ceiling"), ("facing", "east")]),
    ];
    for (seq, (face, properties)) in (1..).zip(cases) {
        place(&alice, support, face, seq).await;
        expect_ack(&alice, seq).await;
        let placed = support.offset_with_direction(face);
        expect_placed(
            &server,
            &[&alice, &bob],
            placed,
            BlockKind::Lever,
            &properties,
        )
        .await;
    }
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_log_takes_the_axis_of_the_clicked_face() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let support = orientation_bench(&server, &alice, EAST, 0.0).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::OakLog)).await;

    let cases = [
        (Direction::Up, "y"),
        (Direction::East, "x"),
        (Direction::South, "z"),
    ];
    for (seq, (face, axis)) in (1..).zip(cases) {
        place(&alice, support, face, seq).await;
        expect_ack(&alice, seq).await;
        let placed = support.offset_with_direction(face);
        expect_placed(
            &server,
            &[&alice, &bob],
            placed,
            BlockKind::OakLog,
            &[("axis", axis)],
        )
        .await;
    }
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn stairs_face_the_way_the_player_looks_upright_or_upside_down() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let support = orientation_bench(&server, &alice, SOUTH, 0.0).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::OakStairs)).await;

    // Looking south: on the top face, on the upper and the lower half of a
    // side face. Then looking north, under the block.
    let cases = [
        (Direction::Up, (0.5, 1.0, 0.5), "bottom"),
        (Direction::North, (0.5, 0.75, 0.0), "top"),
        (Direction::South, (0.5, 0.25, 1.0), "bottom"),
    ];
    for (seq, (face, cursor, half)) in (1..).zip(cases) {
        let click = hit_at(support, face, cursor);
        use_on(&alice, InteractionHand::MainHand, click, seq).await;
        expect_ack(&alice, seq).await;
        expect_placed(
            &server,
            &[&alice, &bob],
            support.offset_with_direction(face),
            BlockKind::OakStairs,
            &[("facing", "south"), ("half", half), ("shape", "straight")],
        )
        .await;
    }
    look(&alice, NORTH, 0.0).await;
    place(&alice, support, Direction::Down, 4).await;
    expect_ack(&alice, 4).await;
    expect_placed(
        &server,
        &[&alice, &bob],
        support.down(1),
        BlockKind::OakStairs,
        &[("facing", "north"), ("half", "top")],
    )
    .await;
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_slab_takes_the_clicked_half_and_a_second_slab_makes_it_double() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let support = orientation_bench(&server, &alice, EAST, 0.0).await;
    build(
        &server,
        &[(support.down(2), state_of(BlockKind::Stone, &[]))],
    )
    .await;
    hold(&alice, HOTBAR_START, Some(ItemKind::OakSlab)).await;
    let both = [&alice, &bob];

    let cases = [
        (Direction::Up, (0.5, 1.0, 0.5), "bottom"),
        (Direction::Down, (0.5, 0.0, 0.5), "top"),
        (Direction::North, (0.5, 0.75, 0.0), "top"),
        (Direction::South, (0.5, 0.25, 1.0), "bottom"),
    ];
    for (seq, (face, cursor, half)) in (1..).zip(cases) {
        let click = hit_at(support, face, cursor);
        use_on(&alice, InteractionHand::MainHand, click, seq).await;
        expect_ack(&alice, seq).await;
        let placed = support.offset_with_direction(face);
        expect_placed(
            &server,
            &both,
            placed,
            BlockKind::OakSlab,
            &[("type", half)],
        )
        .await;
    }

    // A slab on top of the bottom slab fills it, and one against the stone
    // under the top slab fills that one.
    let on_slab = hit_at(support.up(1), Direction::Up, (0.5, 0.5, 0.5));
    use_on(&alice, InteractionHand::MainHand, on_slab, 5).await;
    expect_ack(&alice, 5).await;
    let double = [("type", "double")];
    expect_placed(&server, &both, support.up(1), BlockKind::OakSlab, &double).await;
    place(&alice, support.down(2), Direction::Up, 6).await;
    expect_ack(&alice, 6).await;
    expect_placed(&server, &both, support.down(1), BlockKind::OakSlab, &double).await;
    settle(&both).await;
    assert_eq!(server_block(&server, support.up(2)).await, Some(0));
    assert_converged(&server, &both, &[support.up(2), support.down(2)]).await;
    for bot in both {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn facing_blocks_turn_toward_the_player_on_four_or_six_sides() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let support = orientation_bench(&server, &alice, EAST, 0.0).await;
    let both = [&alice, &bob];

    // A furnace faces back at a player looking east.
    hold(&alice, HOTBAR_START, Some(ItemKind::Furnace)).await;
    place(&alice, support, Direction::Up, 1).await;
    expect_ack(&alice, 1).await;
    let west = [("facing", "west")];
    expect_placed(&server, &both, support.up(1), BlockKind::Furnace, &west).await;

    // A dispenser faces back along the player's look, up or down included.
    hold(&alice, HOTBAR_START, Some(ItemKind::Dispenser)).await;
    place(&alice, support, Direction::South, 2).await;
    expect_ack(&alice, 2).await;
    expect_placed(
        &server,
        &both,
        support.south(1),
        BlockKind::Dispenser,
        &west,
    )
    .await;
    look(&alice, EAST, 90.0).await;
    place(&alice, support, Direction::North, 3).await;
    expect_ack(&alice, 3).await;
    let up = [("facing", "up")];
    expect_placed(&server, &both, support.north(1), BlockKind::Dispenser, &up).await;

    // An observer faces where the player looks.
    look(&alice, EAST, 0.0).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::Observer)).await;
    place(&alice, support, Direction::Down, 4).await;
    expect_ack(&alice, 4).await;
    let east = [("facing", "east")];
    expect_placed(&server, &both, support.down(1), BlockKind::Observer, &east).await;
    for bot in both {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_fence_keeps_its_default_state_next_to_another_block() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let support = orientation_bench(&server, &alice, EAST, 0.0).await;
    // A stone east of where the fence goes: vanilla connects the fence to
    // it, but shapes that depend on the neighbours are not computed.
    let stone = state_of(BlockKind::Stone, &[]);
    build(&server, &[(support.up(1).east(1), stone)]).await;
    hold(&alice, HOTBAR_START, Some(ItemKind::OakFence)).await;
    place(&alice, support, Direction::Up, 1).await;
    expect_ack(&alice, 1).await;
    expect_placed(&server, &[&alice], support.up(1), BlockKind::OakFence, &[]).await;
    assert_eq!(
        server_block(&server, support.up(1)).await,
        Some(state_of(BlockKind::OakFence, &[])),
        "the fence keeps its default state"
    );
    alice.assert_healthy();
    server.stop().await;
}
