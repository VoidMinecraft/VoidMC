use void_codec::{Decode, Encode};

/// Represents the different connection states of a Minecraft client in the protocol lifecycle.
///
/// Each state determines which packets are valid and how the server interacts with the client.
#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Debug, Hash, Encode, Decode)]
#[repr(u8)]
pub enum State {
    /// Initial state where the client sends a handshake packet to initiate a connection.
    Handshake = 0x0,

    /// The state used for server list pings, allowing the client to retrieve server status information.
    Status = 0x1,

    /// Handles authentication and login processes before the client enters the game.
    Login = 0x2,

    /// A temporary state used when transferring a player between servers without disconnecting.
    Transfer = 0x3,

    /// A configuration phase used to send settings, registry data, and resource packs before entering gameplay.
    Configuration = 0x4,

    /// The main gameplay state where all in-game interactions and movement occur.
    Play = 0x5,
}

pub mod clientbound;
pub mod serverbound;
pub mod types;
