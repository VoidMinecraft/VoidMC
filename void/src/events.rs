use bevy_ecs::prelude::*;
use void_protocol::serverbound;

// Raw packet messages — kept for plugin consumption via MessageReader<T>
#[derive(Message)]
pub struct HandshakePacketEvent {
    pub client_id: u32,
    pub entity: Entity,
    pub packet: serverbound::HandshakePacket,
}

#[derive(Message)]
pub struct StatusPacketEvent {
    pub client_id: u32,
    pub entity: Entity,
    pub packet: serverbound::StatusPacket,
}

#[derive(Message)]
pub struct LoginPacketEvent {
    pub client_id: u32,
    pub entity: Entity,
    pub packet: serverbound::LoginPacket,
}

#[derive(Message)]
pub struct ConfigurationPacketEvent {
    pub client_id: u32,
    pub entity: Entity,
    pub packet: serverbound::ConfigurationPacket,
}

#[derive(Message)]
pub struct PlayPacketEvent {
    pub client_id: u32,
    pub entity: Entity,
    pub packet: serverbound::PlayPacket,
}

// Semantic game events — triggered via world.trigger() and handled by observers
#[derive(Event)]
pub struct PlayerJoinEvent {
    pub client_id: u32,
    pub entity: Entity,
}

#[derive(Event)]
pub struct PlayerQuitEvent {
    pub client_id: u32,
    pub entity: Entity,
}

#[derive(Event)]
pub struct PlayerMoveEvent {
    pub entity: Entity,
    pub old_x: f64,
    pub old_y: f64,
    pub old_z: f64,
    pub new_x: f64,
    pub new_y: f64,
    pub new_z: f64,
}

#[derive(Event)]
pub struct PlayerRotateEvent {
    pub entity: Entity,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Event)]
pub struct PlayerReadyEvent {
    pub client_id: u32,
    pub entity: Entity,
}

#[derive(Event)]
pub struct ChatCommandEvent {
    pub entity: Entity,
    pub client_id: u32,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Event)]
pub struct ChatMessageEvent {
    pub entity: Entity,
    pub client_id: u32,
    pub message: String,
}
