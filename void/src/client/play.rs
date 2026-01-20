use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error};
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
        let ip = socket.1.to_string();
        let username = identity.name.clone();
        let uuid = identity.uuid.to_string();

        debug!(
            client_ip = %ip,
            username = %username,
            uuid = %uuid,
            "Player entered Play state"
        );
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
