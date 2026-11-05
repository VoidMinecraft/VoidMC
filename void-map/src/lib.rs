//! The server map as a block model the player looks down on: the client renders
//! real chunks, and the middle of the screen is the pointer.
//!
//! The core has no packets and no ECS. [`terrain::Terrain`] samples a
//! [`voidmc::WorldGenerator`] over a [`window::Window`], [`scene::Scene`] scales it
//! into columns of blocks, and [`markers::MarkerField`] scatters reproducible points
//! of interest. [`pointer`] turns the mouse into a view and an aim, [`regions`] says
//! what a player knows of each place, and [`art`] draws the info cards.
//! [`plugin`] moves players in and out of the map.

pub mod art;
pub mod buildings;
pub mod dimension;
pub mod markers;
pub mod noise;
pub mod plugin;
pub mod pointer;
pub mod regions;
pub mod scene;
pub mod terrain;
pub mod window;

#[cfg(test)]
mod testing;
