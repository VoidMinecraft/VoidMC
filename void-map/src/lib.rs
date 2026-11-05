//! The server map as a block model the player looks down on: the client renders
//! real chunks, and the mouse moves a cursor drawn on them.
//!
//! The core has no packets and no ECS. [`terrain::Terrain`] samples a
//! [`voidmc::WorldGenerator`] over a [`window::Window`], [`scene::Scene`] scales it
//! into columns of blocks, and [`markers::MarkerField`] scatters reproducible points
//! of interest. [`pointer::Pointer`] turns mouse movement into a cursor and a view.
//! [`plugin`] moves players in and out of the map.

pub mod dimension;
pub mod markers;
pub mod plugin;
pub mod pointer;
pub mod scene;
pub mod terrain;
pub mod window;

#[cfg(test)]
mod testing;
