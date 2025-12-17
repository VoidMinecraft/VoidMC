use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error};
use void_net::ClientSocket;

use super::login::ClientIdentity;
use crate::game::Game;
use void_protocol::{
    clientbound::{self, Chunk, ChunkDataAndLight, GameEvent, Login, SetCenterChunk, SynchronizePlayerPosition},
    serverbound,
};

/// Generates a chunk at the given coordinates.
/// If the chunk is at (0, 0), it will have a stone layer at section 3.
pub fn generate_chunk(chunk_x: i32, chunk_z: i32) -> Chunk {
    let stone_section_index = 3;

    if chunk_x == 0 && chunk_z == 0 {
        Chunk::flat_stone(chunk_x, chunk_z, stone_section_index)
    } else {
        Chunk::empty(chunk_x, chunk_z)
    }
}

pub struct PlayClient {
    socket: ClientSocket,
    #[allow(unused)]
    game: Arc<Mutex<Game>>,
    #[allow(unused)]
    identity: ClientIdentity,
}

impl PlayClient {
    pub async fn new(
        mut socket: ClientSocket,
        game: Arc<Mutex<Game>>,
        identity: ClientIdentity,
    ) -> std::io::Result<Self> {
        let ip = socket.1.to_string();
        let username = identity.name.clone();
        let uuid = identity.uuid.to_string();

        debug!(
            client_ip = %ip,
            username = %username,
            uuid = %uuid,
            "Player entered Play state"
        );

        // 1. Login
        socket
            .send(&clientbound::PlayPacket::Login(Login {
                entity_id: 1,
                is_hardcore: false,
                dimension_names: vec!["minecraft:overworld".to_string()],
                max_players: 100,
                view_distance: 10,
                simulation_distance: 10,
                reduced_debug_info: false,
                enable_respawn_screen: true,
                do_limited_crafting: false,
                dimension_type: 0,
                dimension_name: "minecraft:overworld".to_string(),
                hashed_seed: 0,
                game_mode: 0,
                previous_game_mode: -1,
                is_debug: false,
                is_flat: false,
                last_death_location: None,
                portal_cooldown: 10,
                sea_level: 63,
                enforces_secure_chat: false,
            }))
            .await?;

        // 2. Set Center Chunk
        socket
            .send(&clientbound::PlayPacket::SetCenterChunk(clientbound::SetCenterChunk {
                chunk_x: 0,
                chunk_z: 0,
            }))
            .await?;

        // 3. Send Chunks (3x3 for now)
        println!("Sending chunks");
        let radius = 1;
        for x in -radius..=radius {
            for z in -radius..=radius {
                let chunk = generate_chunk(x, z);
                let chunk_packet = chunk.to_packet();
                socket.send(&clientbound::PlayPacket::ChunkDataAndLight(chunk_packet)).await?;
            }
        }

        // 4. Game Event
        println!("Sending game event");
        socket
            .send(&clientbound::PlayPacket::GameEvent(GameEvent {
                event: clientbound::GameEventType::StartWaitingForLevelChunks,
                value: 0.0,
            }))
            .await?;

        // 5. Player Position
        println!("Sending player position");
        socket
            .send(&clientbound::PlayPacket::SynchronizePlayerPosition(
                SynchronizePlayerPosition {
                    teleport_id: 0,
                    x: 8.5,
                    y: 0.0,
                    z: 8.5,
                    vx: 0.0,
                    vy: 0.0,
                    vz: 0.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    flags: clientbound::TeleportFlags::empty(),
                },
            ))
            .await?;

        Ok(Self {
            socket,
            game,
            identity: identity,
        })
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        let ip = self.socket.1.to_string();
        let username = self.identity.name.clone();
        let uuid = self.identity.uuid.to_string();

        debug!(
            client_ip = %ip,
            username = %username,
            uuid = %uuid,
            "Player is now playing"
        );

        loop {
            match self.socket.receive::<serverbound::PlayPacket>().await {
                Ok(packet) => match packet {
                    serverbound::PlayPacket::ConfirmTeleportation(_) => {
                        debug!(client_ip = %ip, username = %username, "Confirmed teleportation");
                    }
                    serverbound::PlayPacket::TickEnd(_) => {
                        debug!(client_ip = %ip, username = %username, "Tick end");
                    }
                    serverbound::PlayPacket::SetPlayerPos(_) => {
                        debug!(client_ip = %ip, username = %username, "Player moved");
                    }
                    serverbound::PlayPacket::SetPlayerPosAndRot(_) => {
                        debug!(client_ip = %ip, username = %username, "Player moved and rotated");
                    }
                    serverbound::PlayPacket::PlayerLoaded(_) => {
                        debug!(client_ip = %ip, username = %username, "Player loaded");
                    }
                },
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        return Err(e);
                    }
                    error!(
                        client_ip = %ip,
                        username = %username,
                        error = ?e,
                        "Packet decode error"
                    );
                }
            }
        }
    }
}
