use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use uuid::Uuid;
use voidmc::{Players, events::PlayerReadyEvent, network::PacketEvent};
use voidmc_protocol::{
    clientbound::ResourcePackPush,
    serverbound::{CustomClickAction, ResourcePackResponse, ResourcePackStatus},
};

use crate::pack::BuiltPack;
use crate::text::Text;

/// A player clicked a [`Text`] or a dialog button that has this id.
#[derive(Event, Debug, Clone, PartialEq, Eq)]
pub struct UiClick {
    pub player: Entity,
    pub id: String,
}

/// The client is done with the pack: it loaded it, or will play without it.
#[derive(Event, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackSettled {
    pub player: Entity,
    pub loaded: bool,
}

/// Serves a pack over HTTP: clients can only download packs from a URL.
#[derive(Debug, Clone)]
pub struct PackServer {
    id: Uuid,
    hash: String,
    url: String,
}

impl PackServer {
    /// Starts serving on `address`. `url` is what clients are told to fetch.
    pub fn start(pack: BuiltPack, address: &str, url: impl Into<String>) -> io::Result<Self> {
        let listener = TcpListener::bind(address)?;
        let server = Self {
            id: pack.id(),
            hash: pack.hash(),
            url: url.into(),
        };
        let pack = Arc::new(pack);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Err(error) = respond(stream, &pack) {
                    tracing::warn!(%error, "resource pack request failed");
                }
            }
        });
        Ok(server)
    }
}

/// Answers every request with the archive; the path does not matter.
fn respond(mut stream: TcpStream, pack: &BuiltPack) -> io::Result<()> {
    let mut request = [0; 1024];
    let _ = stream.read(&mut request)?;
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        pack.bytes().len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(pack.bytes())
}

#[derive(Resource)]
struct Pack {
    server: PackServer,
    required: bool,
    prompt: Text,
}

/// Sends the pack to every player who joins, and turns client packets into
/// [`UiClick`] and [`PackSettled`] events.
pub struct UiPlugin {
    server: PackServer,
    required: bool,
    prompt: Text,
}

impl UiPlugin {
    pub fn new(server: PackServer, prompt: Text) -> Self {
        Self {
            server,
            required: false,
            prompt,
        }
    }

    /// Disconnects players who refuse the pack.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Pack {
            server: self.server.clone(),
            required: self.required,
            prompt: self.prompt.clone(),
        })
        .add_observer(push_pack)
        .add_observer(pack_response)
        .add_observer(click);
    }
}

fn push_pack(event: On<PlayerReadyEvent>, pack: Res<Pack>, players: Players) {
    players.send(
        event.entity,
        ResourcePackPush {
            id: pack.server.id,
            url: pack.server.url.clone(),
            hash: pack.server.hash.clone(),
            required: pack.required,
            prompt: Some(pack.prompt.to_nbt()),
        },
    );
}

fn pack_response(
    event: On<PacketEvent<ResourcePackResponse>>,
    pack: Res<Pack>,
    mut commands: Commands,
) {
    if event.packet.id != pack.server.id {
        return;
    }
    let Some(status) = event.packet.status() else {
        return;
    };
    tracing::debug!(?status, "resource pack");
    if status.is_terminal() && status != ResourcePackStatus::Discarded {
        commands.trigger(PackSettled {
            player: event.entity,
            loaded: status == ResourcePackStatus::SuccessfullyLoaded,
        });
    }
}

fn click(event: On<PacketEvent<CustomClickAction>>, mut commands: Commands) {
    commands.trigger(UiClick {
        player: event.entity,
        id: event.packet.id.clone(),
    });
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;
    use crate::ResourcePack;

    #[test]
    fn the_server_answers_with_the_whole_archive() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let pack = ResourcePack::new("test").build();
        let archive = pack.bytes().to_vec();
        PackServer::start(pack, &address.to_string(), "http://test").unwrap();

        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(b"GET /pack.zip HTTP/1.1\r\n\r\n").unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(response.ends_with(&archive));
    }
}
