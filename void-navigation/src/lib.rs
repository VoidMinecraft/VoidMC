//! Entity navigation for VoidMC.
//!
//! - [`pathing`]: the engine-agnostic core (voxel cells, movement model,
//!   resumable A*, path smoothing and following). It only needs a
//!   [`pathing::CellSource`] from the host engine.

pub mod pathing;

pub use pathing::{BlockPos, Mobility, NavigationProfile, Path, Vec3};
