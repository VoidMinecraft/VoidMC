//! The server map in a vanilla dialog: terrain seen from above, moved by clicks,
//! with clickable markers.
//!
//! The core has no packets and no ECS. [`terrain::Terrain`] samples a
//! [`voidmc::WorldGenerator`] over a [`window::Window`], [`relief::render`] turns
//! that into pixels, and [`markers::MarkerField`] scatters reproducible points of
//! interest. [`view`] draws them as a dialog and [`plugin`] answers the clicks.

pub mod markers;
pub mod plugin;
pub mod relief;
pub mod terrain;
pub mod view;
pub mod window;

#[cfg(test)]
mod testing;
