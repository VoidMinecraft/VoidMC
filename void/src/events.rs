use bevy_ecs::prelude::*;
use void_protocol::serverbound;

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
