use bevy_ecs::event::Event;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode, Event)]
pub struct ConfirmTeleportation {
    #[codec(varint32)]
    pub teleport_id: i32,
}
