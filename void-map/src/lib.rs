//! The server map as a block model the player looks down on: the client renders
//! real chunks, and the middle of the screen is the pointer.
//!
//! The core has no packets and no ECS. [`terrain::Terrain`] samples a
//! [`voidmc::WorldGenerator`] over a [`window::Window`], [`scene::Scene`] scales it
//! into columns of blocks, and [`markers::MarkerField`] scatters reproducible points
//! of interest. [`pointer`] turns the mouse into a view and an aim, [`motion`] says how
//! a pan catches up with it, [`regions`] says what a player knows of each place, and
//! [`art`] draws the info cards.
//! [`players`] shows who walks the world, as small figures on the model, and
//! [`cursor`] draws the pointer.
//! [`plugin`] moves players in and out of the map.

pub mod alphabet;
pub mod art;
pub mod biome;
pub mod buildings;
pub mod cursor;
pub mod markers;
pub mod motion;
pub mod noise;
pub mod players;
pub mod plugin;
pub mod pointer;
pub mod regions;
pub mod scene;
pub mod telemetry;
pub mod terrain;
pub mod transition;
pub mod wheel;
pub mod window;

#[cfg(test)]
mod testing;
