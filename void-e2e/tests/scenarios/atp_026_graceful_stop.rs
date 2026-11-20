//! VMC-ATP-026: `/stop` stops the server gracefully.
//!
//! A player breaks a block on a server whose periodic save never fires, then
//! runs `/stop`. The server has to stop on its own, save the dirty chunk while
//! shutting down, log no error and return `AppExit::Success` from
//! `VoidServer::run` (exit code 0 for a `main` that returns it); a second
//! server over the same world directory has to serve the broken block as air.
//! The signal paths (Ctrl-C, SIGTERM, SIGHUP) cannot be reached in-process:
//! `void-world-io/tests/graceful_stop.rs` sends those signals to a server
//! running in its own process, and `void-example/tests/diagnostics.rs` checks
//! the exit code 130 they give the example server.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bevy_app::AppExit;
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_player_action::{Action, ServerboundPlayerAction},
};
use voidmc_e2e::{TestServer, TestServerBuilder};
use voidmc_world_io::{PersistenceConfig, WorldPersistencePlugin};

use crate::ground;

/// A world directory under the system temp directory, removed on drop.
struct TempWorld(PathBuf);

impl TempWorld {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "voidmc-e2e-atp-026-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("failed to create the temporary world directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempWorld {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A server persisting to `world` whose periodic save never fires, so only
/// the shutdown flush can write a dirty chunk to disk.
fn persistent(world: &TempWorld) -> TestServerBuilder {
    let config = PersistenceConfig::new(world.path()).save_interval_ticks(u64::MAX);
    TestServer::builder().plugin(move |app| {
        app.add_plugins(WorldPersistencePlugin::new(config));
    })
}

#[tokio::test]
async fn stop_command_saves_dirty_chunks_and_stops_the_server() {
    let world = TempWorld::new();

    let server = persistent(&world).operators().start().await;
    let alice = server.join("Alice").await;
    let broken = ground(&alice);
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
    alice
        .wait_until("the broken block turn to air", |view| {
            view.block_at(broken).is_some_and(|state| state.is_air())
        })
        .await;
    alice.assert_healthy();

    alice.command("stop").await;
    // The harness does not ask the server to stop: `/stop` alone has to end
    // the game loop, without a panic or a logged error, and report a success.
    let exit = server.wait_for_exit().await;
    assert_eq!(exit, AppExit::Success, "VoidServer::run returned {exit:?}");
    // Shutting down closes Alice's connection, so only her view is checked.
    assert_eq!(alice.anomalies(), Vec::<String>::new());
    alice.disconnect().await;

    // A new server over the same directory loads the chunk saved on shutdown
    // instead of regenerating the block.
    let server = persistent(&world).start().await;
    let bob = server.join("Bob").await;
    bob.wait_until("the chunk holding the broken block", |view| {
        view.block_at(broken).is_some()
    })
    .await;
    let state = bob.block_at(broken).expect("the chunk is loaded");
    assert!(
        state.is_air(),
        "the block Alice broke before /stop is {state:?} after a restart, not air"
    );

    bob.assert_healthy();
    server.stop().await;
}
