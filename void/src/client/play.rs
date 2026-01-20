use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error};
use void_net::ClientSocket;

use super::login::ClientIdentity;
use crate::game::Game;
use void_protocol::clientbound::{
    self, blocks, Chunk, ChunkBuilder, GameEvent, Login, SetCenterChunk, SynchronizePlayerPosition,
};
use void_protocol::serverbound;

/// Water level for lakes
const WATER_LEVEL: i32 = 62;

/// View radius in chunks
const VIEW_RADIUS: i32 = 8;

/// Generates a plains chunk
pub fn generate_chunk(chunk_x: i32, chunk_z: i32) -> Chunk {
    ChunkBuilder::new(chunk_x, chunk_z)
        .with_heightmap_layered(
            |wx, wz| {
                let base = 64.0;
                let hills = ((wx as f64 * 0.02).sin() + (wz as f64 * 0.02).sin()) * 4.0;
                let detail = ((wx as f64 * 0.05).sin() * (wz as f64 * 0.04).cos()) * 2.0;
                let lake_noise = ((wx as f64 * 0.008).cos() * (wz as f64 * 0.008).sin()) * 8.0;
                (base + hills + detail + lake_noise) as i32
            },
            &[
                (1, blocks::GRASS_BLOCK),
                (3, blocks::STONE),
                (i32::MAX, blocks::STONE),
            ],
        )
        .add_water(WATER_LEVEL)
        .build()
}

/// Generates chunk coordinates sorted by distance from center (spiral pattern)
fn generate_chunk_coords_spiral(radius: i32) -> Vec<(i32, i32)> {
    let mut coords: Vec<(i32, i32)> = Vec::new();

    for x in -radius..=radius {
        for z in -radius..=radius {
            coords.push((x, z));
        }
    }

    // Sort by distance from center (Manhattan distance for simplicity)
    coords.sort_by_key(|(x, z)| x.abs() + z.abs());
    coords
}

pub struct PlayClient {
    socket: ClientSocket,
    #[allow(unused)]
    game: Arc<Mutex<Game>>,
    #[allow(unused)]
    identity: ClientIdentity,
    /// Queue of chunks to send asynchronously
    pending_chunks: VecDeque<(i32, i32)>,
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
                view_distance: VIEW_RADIUS,
                simulation_distance: VIEW_RADIUS,
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

        // 3. Send ONLY the central chunk first (for fast spawn)
        let central_chunk = generate_chunk(0, 0);
        socket
            .send(&clientbound::PlayPacket::ChunkDataAndLight(central_chunk.to_packet()))
            .await?;

        // 4. Game Event - allow player to spawn
        socket
            .send(&clientbound::PlayPacket::GameEvent(GameEvent {
                event: clientbound::GameEventType::StartWaitingForLevelChunks,
                value: 0.0,
            }))
            .await?;

        // 5. Player Position - spawn immediately
        socket
            .send(&clientbound::PlayPacket::SynchronizePlayerPosition(
                SynchronizePlayerPosition {
                    teleport_id: 0,
                    x: 8.5,
                    y: 70.0,
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

        debug!(
            client_ip = %ip,
            username = %username,
            "Player spawned, loading terrain..."
        );

        // 6. Queue remaining chunks (skip 0,0 which is already sent)
        let all_coords = generate_chunk_coords_spiral(VIEW_RADIUS);
        let pending_chunks: VecDeque<(i32, i32)> = all_coords
            .into_iter()
            .filter(|&(x, z)| !(x == 0 && z == 0))
            .collect();

        Ok(Self {
            socket,
            game,
            identity,
            pending_chunks,
        })
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        use tokio::time::{Duration, interval};

        let ip = self.socket.1.to_string();
        let username = self.identity.name.clone();

        debug!(
            client_ip = %ip,
            username = %username,
            "Player is now playing"
        );

        // Send chunks in batches while processing packets
        let mut chunk_interval = interval(Duration::from_millis(10));
        let chunks_per_tick = 4; // Send 4 chunks every 10ms

        loop {
            tokio::select! {
                // Handle incoming packets
                result = self.socket.receive::<serverbound::PlayPacket>() => {
                    match result {
                        Ok(packet) => match packet {
                            serverbound::PlayPacket::ConfirmTeleportation(_) => {
                                debug!(client_ip = %ip, username = %username, "Confirmed teleportation");
                            }
                            serverbound::PlayPacket::TickEnd(_) => {}
                            serverbound::PlayPacket::SetPlayerPos(_) => {}
                            serverbound::PlayPacket::SetPlayerPosAndRot(_) => {}
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

                // Send pending chunks
                _ = chunk_interval.tick(), if !self.pending_chunks.is_empty() => {
                    for _ in 0..chunks_per_tick {
                        if let Some((x, z)) = self.pending_chunks.pop_front() {
                            let chunk = generate_chunk(x, z);
                            if let Err(e) = self.socket.send(&clientbound::PlayPacket::ChunkDataAndLight(chunk.to_packet())).await {
                                error!(client_ip = %ip, username = %username, error = ?e, "Failed to send chunk");
                                break;
                            }
                        } else {
                            debug!(client_ip = %ip, username = %username, "All chunks loaded!");
                            break;
                        }
                    }
                }
            }
        }
    }
}
