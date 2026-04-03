use bevy_ecs::event::Event;
use uuid::Uuid;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode, Event)]
pub struct LoginStart {
    pub name: String,
    pub uuid: Uuid,
}
