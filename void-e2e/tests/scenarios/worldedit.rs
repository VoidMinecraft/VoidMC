//! `voidmc-worldedit`'s `WorldEditPlugin` on a real server: an operator's
//! edit runs to completion within the per-tick time budget it is given.

use std::time::Duration;

use bevy_app::{App, Last};
use bevy_ecs::prelude::*;
use voidmc_e2e::TestServer;
use voidmc_e2e::core::position::{BlockPos, ChunkPos};
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::registry::builtin::BlockKind;
use voidmc_worldedit::WorldEditPlugin;
use voidmc_worldedit::engine::EditQueue;

use crate::feet;

/// Ticks that ended with an edit still queued.
#[derive(Resource, Default)]
struct BusyTicks(u32);

fn count_busy_ticks(queue: Res<EditQueue>, mut busy: ResMut<BusyTicks>) {
    if !queue.is_empty() {
        busy.0 += 1;
    }
}

/// The edited region spans three by three chunks east of the spawn chunk and
/// two sections of each, `y` in `96..128`: 18 sections. Each chunk gets 8192
/// blocks over two sections, too few for the plugin to resend it whole, so
/// the client is sent section updates whether the edit takes one tick or
/// one per section.
const SECTIONS: u32 = 3 * 3 * 2;
const MIN_Y: i32 = 96;
const MAX_Y: i32 = 127;

/// Has `bot` run `//set stone` over the region with the plugin given
/// `time_per_tick`, waits until it is shown the whole region as stone, and
/// returns how many ticks ended with the edit still queued.
async fn set_region(time_per_tick: Duration) -> u32 {
    let server = TestServer::builder()
        .operators()
        .plugin(move |app: &mut App| {
            app.add_plugins(WorldEditPlugin::default().time_per_tick(time_per_tick))
                .init_resource::<BusyTicks>()
                .add_systems(Last, count_busy_ticks);
        })
        .start()
        .await;
    let alice = server.join("Alice").await;

    let spawn = ChunkPos::from(feet(&alice));
    let chunks: Vec<ChunkPos> = (1..=3)
        .flat_map(|dx| (-1..=1).map(move |dz| ChunkPos::new(spawn.x + dx, spawn.z + dz)))
        .collect();
    alice
        .wait_until("the chunks to edit", |view| {
            chunks.iter().all(|chunk| view.chunks.contains_key(chunk))
        })
        .await;

    let low = BlockPos::new((spawn.x + 1) * 16, MIN_Y, (spawn.z - 1) * 16);
    let high = BlockPos::new((spawn.x + 4) * 16 - 1, MAX_Y, (spawn.z + 2) * 16 - 1);
    alice
        .command(&format!("/pos1 {} {} {}", low.x, low.y, low.z))
        .await;
    alice
        .command(&format!("/pos2 {} {} {}", high.x, high.y, high.z))
        .await;
    alice.command("/set stone").await;

    let changed = SECTIONS * 4096;
    alice
        .expect("the edit's completion message", |packet| match packet {
            ClientboundGamePacket::SystemChat(chat) => chat
                .content
                .to_string()
                .contains(&format!("{changed} blocks changed"))
                .then_some(()),
            _ => None,
        })
        .await;
    alice
        .wait_until("the whole region shown as stone", |view| {
            (low.x..=high.x).all(|x| {
                (low.y..=high.y).all(|y| {
                    (low.z..=high.z).all(|z| {
                        view.block_at(BlockPos::new(x, y, z))
                            .is_some_and(|state| BlockKind::from(state) == BlockKind::Stone)
                    })
                })
            })
        })
        .await;

    alice.sync().await;
    let busy = server
        .with_world(|world| world.resource::<BusyTicks>().0)
        .await;
    alice.assert_healthy();
    server.stop().await;
    busy
}

/// `Duration::MAX` is too large to add to the current instant: it means no
/// time limit, and the edit runs whole in the tick it is queued.
#[tokio::test]
async fn an_unbounded_time_per_tick_runs_the_edit_in_one_tick() {
    let busy = set_region(Duration::MAX).await;
    assert!(
        busy <= 1,
        "with no time limit, the edit stayed queued for {busy} ticks"
    );
}

/// A finite limit still splits an edit: with no time at all, each tick runs
/// the one section every tick is allowed.
#[tokio::test]
async fn a_finite_time_per_tick_spreads_the_edit_over_ticks() {
    let busy = set_region(Duration::ZERO).await;
    assert!(
        busy >= SECTIONS - 1,
        "{SECTIONS} sections at one per tick, but the edit stayed queued for only {busy} ticks"
    );
}
