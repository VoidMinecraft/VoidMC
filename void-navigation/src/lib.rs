//! Entity navigation for VoidMC.
//!
//! - [`pathing`]: the engine-agnostic core (voxel cells, movement model,
//!   resumable A*, path smoothing and following). It only needs a
//!   [`pathing::CellSource`] from the host engine.
//! - [`adapter`]: the VoidMC binding — the [`Navigator`] component, goals,
//!   behaviours, outcome events and the chunk-backed cell cache.
//! - [`remote`]: an opt-in operator layer that renders a selected mob's path
//!   and lets the operator drive it.

pub mod pathing;

#[cfg(feature = "voidmc")]
pub mod adapter;
#[cfg(feature = "voidmc")]
pub mod remote;

pub use pathing::{BlockPos, Mobility, NavigationProfile, Path, Vec3};

#[cfg(feature = "voidmc")]
pub use adapter::*;
#[cfg(feature = "voidmc")]
pub use remote::{NavRemote, NavRemotePlugin, RemoteAccess};
