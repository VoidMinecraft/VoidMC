//! VMC-ATP-006: block breaking and placing change the authoritative world
//! and reach every player that holds the chunk.
//!
//! `world::block_changes_reach_other_players_and_late_joiners` already covers
//! the break and placement broadcast, their acknowledgements and a late
//! joiner. These scenarios add the second player filling the cleared block
//! with the selected item, placement against every face, the convergence of
//! interleaved changes, a change made out of a player's view, and invalid
//! interactions.

use std::collections::HashSet;

use voidmc::world::{ChunkData, ChunkIndex, ChunkPos as ServerChunkPos, DimensionId};
use voidmc_e2e::block::BlockState;
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, ChunkPos, Vec3};
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_interact::InteractionHand,
    s_player_action::{Action, ServerboundPlayerAction},
    s_set_carried_item::ServerboundSetCarriedItem,
    s_set_creative_mode_slot::ServerboundSetCreativeModeSlot,
    s_use_item::ServerboundUseItem,
    s_use_item_on::{BlockHit, ServerboundUseItemOn},
};
use voidmc_e2e::registry::builtin::{BlockKind, ItemKind};
use voidmc_e2e::{Bot, BotOptions, TestServer};

use crate::{feet, ground};

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

    // A pillar three blocks above the ground, built by clicking top faces.
    let base = ground(&bob);
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

    let base = ground(&bob);
    let target = base.up(1);
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
    // vanilla client lets the player click through).
    let base = ground(&bob);
    let other = bob
        .view(|view| view.ground_below(BlockPos::new(base.x + 2, base.y + 8, base.z)))
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

    let centre = feet(&alice);
    let columns: Vec<BlockPos> = (-3..=3)
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
    // Alice breaks each ground block and puts planks on top of it, Bob refills
    // it with stone, before Alice on even columns and after her on odd ones.
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
        assert!(
            matches!(state, Some(BlockKind::Air | BlockKind::Stone)),
            "{ground:?} is air or Bob's stone, not {state:?}"
        );
        assert_eq!(
            kind(alice.block_at(ground.up(1))),
            Some(BlockKind::OakPlanks),
            "Alice's planks stand on {ground:?}"
        );
    }

    // Then both clear the columns at once: Alice her planks, Bob the ground.
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
        BlockPos::new(centre.x - 4, low - 2, centre.z - 1),
        BlockPos::new(centre.x + 4, high + 2, centre.z + 1),
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

    // Interactions a vanilla server would refuse, but which the world may
    // accept as long as every player ends up with the same block: placing
    // into an occupied block, with a cursor outside the block, and breaking
    // far out of reach.
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
    let mut checked = cuboid(base.down(1), base.up(1));
    checked.push(reach);
    assert_converged(&server, &[&alice, &bob], &checked).await;

    // The server still applies and shares valid changes.
    let valid = base.up(2);
    place(&alice, base.up(1), Direction::Up, 1000).await;
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
