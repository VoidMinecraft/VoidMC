//! The engine-agnostic navigation core: packed voxel cells, a section cache,
//! the walk/fly/swim movement model, a resumable A* and a path follower.
//! Nothing here depends on VoidMC or Bevy; an engine plugs in through
//! [`CellSource`] (or [`NavWorld`]) and reads [`Steering`] back.

mod cell;
mod follow;
mod grid;
mod hash;
mod math;
mod movement;
mod path;
mod profile;
mod search;

#[cfg(test)]
mod tests;

pub use cell::{Cell, CellKind, MAX_TOP, Side};
pub use follow::{FollowStatus, PathFollower, Steering};
pub use grid::{
    ArrayWorld, CacheStats, CachedWorld, CellCache, CellSource, NavWorld, SECTION_CELLS,
    SectionCells, SectionPos, cell_index,
};
pub use math::{BlockPos, Vec3, yaw_towards};
pub use movement::{Move, Stand, heuristic, neighbors, stand};
pub use path::Path;
pub use profile::{Mobility, MoveModel, NavigationProfile};
pub use search::{Pathfinder, SearchRequest, SearchStats, SearchStatus};
