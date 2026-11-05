//! Pure core of the server map: no packets, no ECS, no display.
//!
//! [`terrain::Terrain`] samples a [`voidmc::WorldGenerator`] over a
//! [`window::Window`], [`relief::render`] turns that into RGBA pixels, and
//! [`markers::MarkerField`] scatters reproducible points of interest.

pub mod markers;
pub mod relief;
pub mod terrain;
pub mod window;

#[cfg(test)]
mod testing;
