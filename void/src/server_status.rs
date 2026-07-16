use std::sync::{Arc, RwLock};

use bevy_ecs::prelude::Resource;
use voidmc_protocol::{MINECRAFT_VERSION, PROTOCOL_VERSION, clientbound};

use crate::ServerConfig;

#[derive(Clone, Resource)]
pub(crate) struct ServerStatusSnapshot(Arc<RwLock<ServerStatusData>>);

struct ServerStatusData {
    max_players: i32,
    online_players: i32,
    motd: String,
}

impl ServerStatusSnapshot {
    pub(crate) fn new(config: &ServerConfig) -> Self {
        Self(Arc::new(RwLock::new(ServerStatusData {
            max_players: config.max_players,
            online_players: 0,
            motd: config.motd.clone(),
        })))
    }

    pub(crate) fn update(&self, max_players: i32, online_players: usize, motd: &str) {
        let mut status = self.0.write().expect("server status lock poisoned");
        status.max_players = max_players;
        status.online_players = player_count(online_players);
        motd.clone_into(&mut status.motd);
    }

    pub(crate) fn response(&self) -> clientbound::StatusResponse {
        let status = self.0.read().expect("server status lock poisoned");

        clientbound::StatusResponse {
            status: server_status(
                status.max_players,
                status.online_players,
                status.motd.clone(),
            ),
        }
    }
}

pub(crate) fn player_count(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

pub(crate) fn server_status(
    max_players: i32,
    online_players: i32,
    motd: String,
) -> clientbound::Status {
    clientbound::Status {
        version: clientbound::Version {
            name: MINECRAFT_VERSION.to_string(),
            protocol: PROTOCOL_VERSION,
        },
        players: clientbound::Players {
            max: max_players,
            online: online_players,
            sample: vec![],
        },
        description: clientbound::Description { text: motd },
        favicon: "".to_string(),
        enforces_secure_chat: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_advertises_supported_protocol_and_server_details() {
        let status = server_status(100, 3, "VoidMC".to_string());

        assert_eq!(status.version.name, "26.1.2");
        assert_eq!(status.version.protocol, 775);
        assert_eq!(status.players.max, 100);
        assert_eq!(status.players.online, 3);
        assert_eq!(status.description.text, "VoidMC");
    }

    #[test]
    fn player_count_saturates_at_protocol_limit() {
        assert_eq!(player_count(0), 0);
        assert_eq!(player_count(42), 42);
        assert_eq!(player_count(i32::MAX as usize + 1), i32::MAX);
    }
}
