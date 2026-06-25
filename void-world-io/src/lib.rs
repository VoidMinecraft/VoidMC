//! World persistence for VoidMC.
//!
//! Saves modified chunks to disk and reloads them on startup, as an opt-in
//! plugin. Add [`WorldPersistencePlugin`] to your server and configure it with
//! [`PersistenceConfig`].
#![allow(clippy::map_entry)]

pub mod config;
pub mod error;
pub mod format;
pub mod plugin;
pub mod region;
pub mod store;
pub mod systems;

pub use config::PersistenceConfig;
pub use error::{PersistenceError, Result};
pub use plugin::WorldPersistencePlugin;
pub use region::RegionChunkStore;
pub use store::{ChunkStore, ChunkStoreResource, StoreLoader};
