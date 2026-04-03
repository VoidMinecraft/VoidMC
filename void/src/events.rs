use bevy_ecs::prelude::*;
use void_protocol::serverbound;
use void_protocol::types::{BlockFace, BlockPosition, Hand};

// Generic packet queue resource — each protocol state gets one
#[derive(Resource)]
pub struct PacketQueue<T: Send + Sync + 'static>(pub Vec<T>);

impl<T: Send + Sync + 'static> Default for PacketQueue<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

#[derive(Event)]
pub struct ConfigurationPacketEvent {
    pub client_id: u32,
    pub entity: Entity,
    pub packet: serverbound::ConfigurationPacket,
}

#[derive(Event)]
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

#[derive(Event)]
pub struct PlayerSneakEvent {
    pub entity: Entity,
    pub sneaking: bool,
}

#[derive(Event)]
pub struct PlayerSprintEvent {
    pub entity: Entity,
    pub sprinting: bool,
}

#[derive(Event)]
pub struct PlayerInteractEntityEvent {
    pub entity: Entity,
    pub target_id: i32,
    pub attack: bool,
    pub hand: Option<Hand>,
    pub target_pos: Option<(f32, f32, f32)>,
    pub sneaking: bool,
}

#[derive(Event)]
pub struct PlayerStartDiggingEvent {
    pub entity: Entity,
    pub position: BlockPosition,
    pub face: BlockFace,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerCancelDiggingEvent {
    pub entity: Entity,
    pub position: BlockPosition,
    pub face: BlockFace,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerFinishDiggingEvent {
    pub entity: Entity,
    pub position: BlockPosition,
    pub face: BlockFace,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerDropItemEvent {
    pub entity: Entity,
    pub drop_stack: bool,
}

#[derive(Event)]
pub struct PlayerSwapHandsEvent {
    pub entity: Entity,
}

#[derive(Event)]
pub struct PlayerUseItemOnBlockEvent {
    pub entity: Entity,
    pub hand: Hand,
    pub position: BlockPosition,
    pub face: BlockFace,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub cursor_z: f32,
    pub inside_block: bool,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerUseItemEvent {
    pub entity: Entity,
    pub hand: Hand,
    pub sequence: i32,
}

#[derive(Event)]
pub struct PlayerSwingArmEvent {
    pub entity: Entity,
    pub hand: Hand,
}

#[derive(Event)]
pub struct PlayerChangeSlotEvent {
    pub entity: Entity,
    pub slot: i16,
}

#[derive(Event)]
pub struct PlayerToggleFlyEvent {
    pub entity: Entity,
    pub flying: bool,
}

#[derive(Event)]
pub struct PlayerCloseContainerEvent {
    pub entity: Entity,
    pub window_id: u8,
}
