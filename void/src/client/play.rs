use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error};
use ussr_nbt::owned::Nbt;
use void_net::ClientSocket;

use super::login::ClientIdentity;
use crate::game::Game;
use void_protocol::{
    clientbound::{self, ChunkDataAndLight, GameEvent, Login, SetCenterChunk, SynchronizePlayerPosition},
    serverbound,
};

fn write_varint_to_vec(vec: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        if (value & !0x7F) == 0 {
            vec.push(value as u8);
            return;
        }
        vec.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
}

pub fn generate_chunk(chunk_x: i32, chunk_z: i32) -> ChunkDataAndLight {
    let mut data = Vec::new();

    let stone_section_index = 3;

    for section_idx in 0..24 {
        let mut block_count: i16 = 0;
        let mut block_state_id = 0; // Air
        let biome_id = 1; // Plains

        // If we are at chunk 0,0 and at section Y=0 -> Stone
        if chunk_x == 0 && chunk_z == 0 && section_idx == stone_section_index {
            block_count = 4096;
            block_state_id = 1; // Stone
        }

        // 1. Block Count
        data.extend_from_slice(&block_count.to_be_bytes());

        // 2. Block States (Single Value Palette)
        data.push(0); // BPE = 0
        write_varint_to_vec(&mut data, block_state_id); // Palette ID
        write_varint_to_vec(&mut data, 0); // Data Length = 0

        // 3. Biomes (Single Value Palette)
        data.push(0); // BPE = 0
        write_varint_to_vec(&mut data, biome_id);
        write_varint_to_vec(&mut data, 0);
    }

    // --- Heightmap (NBT) ---
    // Client needs NBT Compound containing "MOTION_BLOCKING".
    // Longs Table (i64).
    // Size: 256 values * 9 bits = 2304 bits. 2304 / 64 = 36 Longs.
    let motion_blocking: Vec<i64> = vec![0i64; 36];

    let heightmaps = Nbt {
        name: "".into(),
        compound: vec![
            ("MOTION_BLOCKING".into(), motion_blocking.into()),
        ].into(),
    };

    // --- Lights ---
    // 24 sections + 2 sentinels = 26 bits.
    let empty_mask = vec![0xFFFFFFFFFFFFFFFF];
    let zero_mask = vec![0];

    ChunkDataAndLight {
        chunk_x,
        chunk_z,
        heightmaps,
        data,
        block_entities: Vec::new(),
        sky_light_mask: zero_mask.clone(),
        block_light_mask: zero_mask.clone(),
        empty_sky_light_mask: empty_mask.clone(),
        empty_block_light_mask: empty_mask.clone(),
        sky_light_arrays: Vec::new(),
        block_light_arrays: Vec::new(),
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
                let chunk_packet = generate_chunk(x, z);
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
