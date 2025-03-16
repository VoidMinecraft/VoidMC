use void_net::clientbound::{Description, Players, Status, Version};

#[derive(Debug)]
pub struct Game {
    pub motd: String,
    pub favicon: String,
}

impl Game {
    pub fn status(&self, protocol: i32) -> Status {
        Status {
            version: Version {
                name: "1.21.4".to_string(),
                protocol,
            },
            players: Players {
                max: -1,
                online: 0,
                sample: vec![],
            },
            description: Description {
                text: self.motd.clone(),
            },
            favicon: self.favicon.clone(),
            enforces_secure_chat: false,
        }
    }
}
