//! WorldEdit-style in-game world editing.
//!
//! The editing core (selections, regions, patterns, masks, operations,
//! budgeted jobs, undo history, clipboard, schematics and brushes) only
//! talks to the world through the [`Extent`] trait, one 16³ section at a
//! time. The `engine` feature adds the VoidMC adapter: chunk storage as an
//! extent, update packets, per-player selection outlines, the wand, brushes
//! and the `//` commands.

pub mod block;
pub mod brush;
pub mod clipboard;
pub mod extent;
pub mod history;
pub mod job;
pub mod mask;
pub mod math;
pub mod operation;
pub mod pattern;
pub mod region;
pub mod schematic;
pub mod selection;

#[cfg(feature = "engine")]
pub mod engine;

pub use block::{BlockError, BlockState};
pub use brush::{Brush, BrushShape, raycast};
pub use clipboard::Clipboard;
pub use extent::{ChangeMask, Extent, MemoryExtent, SectionBlocks};
pub use history::{ChangeSet, History};
pub use job::{Budget, CopyJob, EditJob, EditStats};
pub use mask::{BlockSet, Mask, MaskError};
pub use math::{Axis, BlockPos, Direction, SectionPos, Vec3};
pub use operation::{BlockList, Fill, Operation, Paste, Restore, Sequence};
pub use pattern::{Pattern, PatternError};
pub use region::{Cuboid, Cylinder, Ellipsoid, Faces, Region, Walls};
pub use schematic::{Schematic, SchematicError};
pub use selection::{Selection, SelectionError, SelectionShape};

#[cfg(feature = "engine")]
pub use engine::{Permission, WorldEditConfig, WorldEditPlugin};
