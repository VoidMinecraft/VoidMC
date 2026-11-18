use std::collections::HashSet;

use voidmc_e2e::block::BlockState;
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, ChunkPos, Vec3};
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_player_action::{Action, ServerboundPlayerAction},
    s_set_creative_mode_slot::ServerboundSetCreativeModeSlot,
    s_use_item_on::{BlockHit, ServerboundUseItemOn},
};
use voidmc_e2e::registry::builtin::{BlockKind, ItemKind};
use voidmc_e2e::{Bot, BotOptions, TestServer};

use crate::{feet, ground};

fn square(center: ChunkPos, radius: i32) -> HashSet<ChunkPos> {
    (-radius..=radius)
        .flat_map(|dx| {
            (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
        })
        .collect()
}

fn loaded(bot: &Bot) -> HashSet<ChunkPos> {
    bot.view(|view| view.chunks.keys().copied().collect())
}

fn kind(state: Option<BlockState>) -> Option<BlockKind> {
    state.map(BlockKind::from)
}

#[tokio::test]
async fn chunks_follow_the_player_and_are_forgotten_behind_it() {
    let server = TestServer::builder()
        .config(|config| config.view_distance(6))
        .start()
        .await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 2 })
        .await;

    let spawn = ChunkPos::from(feet(&alice));
    alice
        .wait_until("the chunks within its view distance", |view| {
            view.chunks.keys().copied().collect::<HashSet<_>>() == square(spawn, 2)
        })
        .await;

    let start = alice.position();
    let mut position = start;
    for _ in 0..6 {
        position = Vec3::new(position.x + 8.0, position.y, position.z);
        alice.move_to(position).await;
    }
    let target = ChunkPos::from(BlockPos::from(position));
    assert_eq!(target.x - spawn.x, 3);

    alice
        .wait_until("chunks re-centred three chunks east", |view| {
            view.chunk_center == Some(target)
                && view.chunks.keys().copied().collect::<HashSet<_>>() == square(target, 2)
        })
        .await;
    alice.sync().await;
    assert_eq!(loaded(&alice), square(target, 2));
    ground(&alice);

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn block_changes_reach_other_players_and_late_joiners() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    let broken = ground(&alice);
    let support = BlockPos::new(broken.x + 2, broken.y, broken.z);
    let support = bob
        .view(|view| view.ground_below(BlockPos::new(support.x, support.y + 8, support.z)))
        .expect("Bob holds the chunk around Alice");
    let placed = support.up(1);
    assert!(bob.block_at(broken).is_some_and(|state| !state.is_air()));

    alice
        .send(ServerboundPlayerAction {
            action: Action::StartDestroyBlock,
            pos: broken,
            direction: Direction::Up,
            seq: 1,
        })
        .await;
    alice
        .expect("the break acknowledgement", |packet| match packet {
            ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == 1 => Some(()),
            _ => None,
        })
        .await;
    bob.wait_until("the broken block turn to air", |view| {
        view.block_at(broken).is_some_and(|state| state.is_air())
    })
    .await;

    alice
        .send(ServerboundSetCreativeModeSlot {
            slot_num: 36,
            item_stack: ItemStack::Present(ItemStackData::new(ItemKind::Stone, 1)),
        })
        .await;
    alice
        .send(ServerboundUseItemOn {
            hand: Default::default(),
            block_hit: BlockHit {
                block_pos: support,
                direction: Direction::Up,
                location: Vec3::new(
                    f64::from(support.x) + 0.5,
                    f64::from(support.y) + 1.0,
                    f64::from(support.z) + 0.5,
                ),
                inside: false,
                world_border: false,
            },
            seq: 2,
        })
        .await;
    alice
        .expect("the placement acknowledgement", |packet| match packet {
            ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == 2 => Some(()),
            _ => None,
        })
        .await;
    bob.wait_until("the placed stone", |view| {
        kind(view.block_at(placed)) == Some(BlockKind::Stone)
    })
    .await;
    alice.sync().await;
    assert_eq!(kind(alice.block_at(placed)), Some(BlockKind::Stone));
    assert!(alice.block_at(broken).is_some_and(|state| state.is_air()));

    let carol = server.join("Carol").await;
    assert!(
        carol.block_at(broken).is_some_and(|state| state.is_air()),
        "the chunk sent to a late joiner keeps the broken block"
    );
    assert_eq!(
        kind(carol.block_at(placed)),
        Some(BlockKind::Stone),
        "the chunk sent to a late joiner keeps the placed block"
    );

    for bot in [&alice, &bob, &carol] {
        bot.assert_healthy();
    }
    server.stop().await;
}
