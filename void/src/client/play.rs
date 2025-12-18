use std::sync::Arc;
use tokio::sync::Mutex;
use void_net::ClientSocket;

use super::login::ClientIdentity;
use crate::game::Game;
use void_protocol::{
    clientbound::{self, GameEvent, Login, SynchronizePlayerPosition},
    serverbound,
};

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
        println!("[{}] State is now Play", socket.1);
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

        socket
            .send(&clientbound::PlayPacket::GameEvent(GameEvent {
                event: clientbound::GameEventType::StartWaitingForLevelChunks,
                value: 0.0,
            }))
            .await?;

        socket
            .send(&clientbound::PlayPacket::SynchronizePlayerPosition(
                SynchronizePlayerPosition {
                    teleport_id: 0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
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
        loop {
            match self.socket.receive::<serverbound::PlayPacket>().await {
                Ok(packet) => match packet {
                    serverbound::PlayPacket::TickEnd(_) => {}
                    serverbound::PlayPacket::SetPlayerPos(_) => {}
                },
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        return Err(e);
                    }
                    eprintln!("Failed to receive packet: {:?}", e);
                }
            }
        }
    }
}
