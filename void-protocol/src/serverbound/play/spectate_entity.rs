use voidmc_codec::{Decode, Encode};

/// Serverbound Spectate Entity: a spectator attacked the entity under the
/// crosshair. It is the only click a spectator sends.
#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct SpectateEntity {
    #[codec(varint32)]
    pub entity_id: i32,
}
