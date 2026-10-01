//! End-to-end tests for VoidMC: a real server in-process, real protocol
//! clients over TCP.
//!
//! ```no_run
//! # async fn demo() {
//! use voidmc_e2e::TestServer;
//!
//! let server = TestServer::start().await;
//! let alice = server.join("Alice").await;
//! alice.command("tp 0 100 0").await;
//! alice.sync().await;
//! server.stop().await;
//! # }
//! ```

mod bot;
mod logs;
mod server;
mod view;

use std::future::Future;
use std::time::Duration;

pub use bot::{Bot, BotOptions, JoinTranscript};
pub use logs::LogRecord;
pub use server::{TestServer, TestServerBuilder};
pub use view::{ClientView, SeenEntity};

pub use azalea_block as block;
pub use azalea_brigadier as brigadier;
pub use azalea_core as core;
pub use azalea_inventory as inventory;
pub use azalea_protocol as protocol;
pub use azalea_registry as registry;

pub(crate) const TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) async fn timeout<T>(what: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(TIMEOUT, future)
        .await
        .unwrap_or_else(|_| panic!("timed out after {TIMEOUT:?} waiting for {what}"))
}
