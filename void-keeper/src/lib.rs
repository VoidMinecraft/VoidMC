//! A development proxy that keeps a vanilla client in game across server restarts.
//!
//! [`session`] is the whole logic and does no I/O, [`proxy`] runs it on sockets.

pub mod packs;
pub mod proxy;
pub mod script;
pub mod session;
pub mod wire;
