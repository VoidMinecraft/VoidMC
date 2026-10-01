use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use azalea_core::position::{BlockPos, Vec3};
use azalea_core::registry_holder::RegistryHolder;
use azalea_entity::LookDirection;
use azalea_protocol::common::client_information::ClientInformation;
use azalea_protocol::common::movements::MoveFlags;
use azalea_protocol::connect::{Connection, ReadConnection, WriteConnection};
use azalea_protocol::packets::config::{
    ClientboundConfigPacket, s_client_information,
    s_finish_configuration::ServerboundFinishConfiguration,
    s_keep_alive::ServerboundKeepAlive as ConfigKeepAlive,
    s_select_known_packs::ServerboundSelectKnownPacks,
};
use azalea_protocol::packets::game::{
    ClientboundGamePacket, ServerboundGamePacket,
    s_accept_teleportation::ServerboundAcceptTeleportation, s_chat::ServerboundChat,
    s_chat_command::ServerboundChatCommand, s_keep_alive::ServerboundKeepAlive,
    s_move_player_pos::ServerboundMovePlayerPos,
    s_move_player_pos_rot::ServerboundMovePlayerPosRot, s_player_loaded::ServerboundPlayerLoaded,
    s_pong::ServerboundPong,
};
use azalea_protocol::packets::handshake::s_intention::ServerboundIntention;
use azalea_protocol::packets::login::{
    ClientboundLoginPacket, s_hello::ServerboundHello,
    s_login_acknowledged::ServerboundLoginAcknowledged,
};
use azalea_protocol::packets::status::{
    ClientboundStatusPacket, c_status_response::ClientboundStatusResponse,
    s_status_request::ServerboundStatusRequest,
};
use azalea_protocol::packets::{ClientIntention, PROTOCOL_VERSION, Packet, ProtocolPacket};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::server::BARRIER_BIT;
use crate::timeout;
use crate::view::ClientView;

static NEXT_BARRIER: AtomicU32 = AtomicU32::new(1);

/// How a bot presents itself while joining.
#[derive(Clone, Debug)]
pub struct BotOptions {
    pub view_distance: u8,
}

impl Default for BotOptions {
    fn default() -> Self {
        Self { view_distance: 8 }
    }
}

/// What a bot learned while logging in, before it entered the play state.
#[derive(Clone, Debug, Default)]
pub struct JoinTranscript {
    pub compression_threshold: Option<i32>,
    pub known_packs: Vec<String>,
    pub registries: Vec<String>,
    pub tags_received: bool,
}

#[derive(Default)]
struct Inbox {
    packets: VecDeque<ClientboundGamePacket>,
    received: BTreeMap<&'static str, usize>,
    loaded: bool,
    view: ClientView,
    failure: Option<String>,
    closed: bool,
}

struct Shared {
    inbox: Mutex<Inbox>,
    changed: Notify,
}

type Writer = Arc<tokio::sync::Mutex<WriteConnection<ServerboundGamePacket>>>;

/// A protocol-level Minecraft client driven packet by packet.
///
/// Packets are decoded by Azalea, an implementation independent from VoidMC,
/// with strict length checks: a packet the vanilla client could not read
/// fails the test. A background task answers keep-alives, pings and
/// teleports like the vanilla client, and keeps a [`ClientView`] up to date.
pub struct Bot {
    name: String,
    uuid: Uuid,
    entity_id: i32,
    transcript: JoinTranscript,
    writer: Writer,
    shared: Arc<Shared>,
    reader: JoinHandle<()>,
}

impl Bot {
    pub(crate) async fn join(address: SocketAddr, name: &str, options: BotOptions) -> Self {
        let uuid = azalea_crypto::offline::generate_uuid(name);
        let (connection, transcript, registries) = timeout(
            &format!("{name} to log in"),
            login(address, name, uuid, &options),
        )
        .await;
        let (read, write) = connection.into_split();
        let shared = Arc::new(Shared {
            inbox: Mutex::new(Inbox {
                view: ClientView::new(&registries),
                ..Default::default()
            }),
            changed: Notify::new(),
        });
        let writer: Writer = Arc::new(tokio::sync::Mutex::new(write));
        let reader = tokio::spawn(read_loop(read, writer.clone(), shared.clone()));
        let mut bot = Self {
            name: name.to_string(),
            uuid,
            entity_id: 0,
            transcript,
            writer,
            shared,
            reader,
        };
        bot.wait_until(&format!("{name} to spawn"), |view| {
            view.entity_id.is_some() && view.players.contains_key(&uuid)
        })
        .await;
        let entity_id = bot.lock().view.entity_id;
        bot.entity_id = entity_id.expect("spawned bots have an id");
        bot
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    pub fn entity_id(&self) -> i32 {
        self.entity_id
    }

    pub fn transcript(&self) -> &JoinTranscript {
        &self.transcript
    }

    pub async fn send(&self, packet: impl Packet<ServerboundGamePacket>) {
        self.writer
            .lock()
            .await
            .write(packet.into_variant())
            .await
            .unwrap_or_else(|error| panic!("{} could not send a packet: {error}", self.name));
    }

    pub async fn command(&self, command: &str) {
        self.send(ServerboundChatCommand {
            command: command.to_string(),
        })
        .await;
    }

    pub async fn chat(&self, message: &str) {
        self.send(ServerboundChat {
            message: message.to_string(),
            timestamp: 0,
            salt: 0,
            signature: None,
            last_seen_messages: Default::default(),
        })
        .await;
    }

    /// Moves the bot like a client walking there, without physics.
    pub async fn move_to(&self, position: Vec3) {
        self.lock().view.position = position;
        self.send(ServerboundMovePlayerPos {
            pos: position,
            flags: MoveFlags {
                on_ground: true,
                horizontal_collision: false,
            },
        })
        .await;
    }

    pub fn view<R>(&self, f: impl FnOnce(&ClientView) -> R) -> R {
        f(&self.lock().view)
    }

    pub fn position(&self) -> Vec3 {
        self.view(|view| view.position)
    }

    pub fn block_at(&self, pos: BlockPos) -> Option<azalea_block::BlockState> {
        self.view(|view| view.block_at(pos))
    }

    /// Removes and returns the oldest received packet for which `f` returns
    /// `Some`, waiting for it if needed. Packets that do not match stay
    /// queued for later expectations. `f` runs while the bot's state is
    /// locked: it must not call this bot's `view`, `position` or `block_at`.
    pub async fn expect<T>(
        &self,
        what: &str,
        mut f: impl FnMut(&ClientboundGamePacket) -> Option<T>,
    ) -> T {
        let description = format!("{} to receive {what}", self.name);
        let shared = self.shared.clone();
        let found = tokio::time::timeout(crate::TIMEOUT, async {
            loop {
                let changed = shared.changed.notified();
                {
                    let mut inbox = shared.inbox.lock().unwrap();
                    let hit = inbox
                        .packets
                        .iter()
                        .enumerate()
                        .find_map(|(index, packet)| f(packet).map(|value| (index, value)));
                    if let Some((index, value)) = hit {
                        inbox.packets.remove(index);
                        return Ok(value);
                    }
                    if let Some(failure) = inbox.failure.clone() {
                        return Err(failure);
                    }
                }
                changed.await;
            }
        })
        .await;
        match found {
            Ok(Ok(value)) => value,
            Ok(Err(failure)) => panic!("waiting for {description}: {failure}"),
            Err(_) => panic!(
                "timed out waiting for {description}; received so far: {:?}",
                self.lock().received
            ),
        }
    }

    /// Waits until the client view satisfies `f`.
    pub async fn wait_until(&self, what: &str, f: impl Fn(&ClientView) -> bool) {
        let description = format!("{} to see {what}", self.name);
        let shared = self.shared.clone();
        let reached = tokio::time::timeout(crate::TIMEOUT, async {
            loop {
                let changed = shared.changed.notified();
                {
                    let inbox = shared.inbox.lock().unwrap();
                    if f(&inbox.view) {
                        return Ok(());
                    }
                    if let Some(failure) = inbox.failure.clone() {
                        return Err(failure);
                    }
                }
                changed.await;
            }
        })
        .await;
        match reached {
            Ok(Ok(())) => {}
            Ok(Err(failure)) => panic!("waiting for {description}: {failure}"),
            Err(_) => panic!("timed out waiting for {description}"),
        }
    }

    /// Returns once the server has handled every packet this bot sent before
    /// the call, finished the tick it handled them in, and every packet it
    /// sent this bot during that tick has been received.
    pub async fn sync(&self) {
        let id = BARRIER_BIT | NEXT_BARRIER.fetch_add(1, Ordering::Relaxed);
        self.send(ServerboundPong { id }).await;
        self.expect("the sync barrier", |packet| match packet {
            ClientboundGamePacket::Ping(ping) if ping.id == id => Some(()),
            _ => None,
        })
        .await;
    }

    /// Removes every queued packet.
    pub fn clear(&self) {
        self.lock().packets.clear();
    }

    /// The queued packets that `f` matches, without removing them.
    pub fn queued<T>(&self, f: impl FnMut(&ClientboundGamePacket) -> Option<T>) -> Vec<T> {
        self.lock().packets.iter().filter_map(f).collect()
    }

    /// Protocol oddities a vanilla client tolerates but that point at a
    /// server bug (an entity added twice, a block update for an unloaded
    /// chunk, ...).
    pub fn anomalies(&self) -> Vec<String> {
        self.view(|view| view.anomalies.clone())
    }

    pub fn assert_healthy(&self) {
        let inbox = self.lock();
        if let Some(failure) = &inbox.failure {
            panic!("{} failed: {failure}", self.name);
        }
        assert!(
            inbox.view.anomalies.is_empty(),
            "{} saw protocol anomalies: {:#?}",
            self.name,
            inbox.view.anomalies
        );
    }

    /// Closes the connection like a client quitting the game.
    pub async fn disconnect(self) {
        self.reader.abort();
        let _ = self.writer.lock().await.shutdown().await;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inbox> {
        self.shared.inbox.lock().unwrap()
    }
}

impl Drop for Bot {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

pub(crate) async fn status(address: SocketAddr) -> ClientboundStatusResponse {
    let mut connection = Connection::new(&address)
        .await
        .unwrap_or_else(|error| panic!("could not connect to {address}: {error}"));
    connection
        .write(ServerboundIntention {
            protocol_version: PROTOCOL_VERSION,
            hostname: address.ip().to_string(),
            port: address.port(),
            intention: ClientIntention::Status,
        })
        .await
        .expect("handshake");
    let mut connection = connection.status();
    connection
        .write(ServerboundStatusRequest {})
        .await
        .expect("status request");
    match connection.read().await {
        Ok(ClientboundStatusPacket::StatusResponse(response)) => response,
        Ok(other) => panic!("expected a status response, got {other:?}"),
        Err(error) => panic!("failed to read the status response: {error}"),
    }
}

async fn login(
    address: SocketAddr,
    name: &str,
    uuid: Uuid,
    options: &BotOptions,
) -> (
    Connection<ClientboundGamePacket, ServerboundGamePacket>,
    JoinTranscript,
    RegistryHolder,
) {
    let mut transcript = JoinTranscript::default();
    let mut connection = Connection::new(&address)
        .await
        .unwrap_or_else(|error| panic!("{name} could not connect to {address}: {error}"));
    connection
        .write(ServerboundIntention {
            protocol_version: PROTOCOL_VERSION,
            hostname: address.ip().to_string(),
            port: address.port(),
            intention: ClientIntention::Login,
        })
        .await
        .expect("handshake");

    let mut connection = connection.login();
    connection
        .write(ServerboundHello {
            name: name.to_string(),
            profile_id: uuid,
        })
        .await
        .expect("login start");

    loop {
        match connection.read().await {
            Ok(ClientboundLoginPacket::LoginCompression(packet)) => {
                transcript.compression_threshold = Some(packet.compression_threshold);
                connection.set_compression_threshold(packet.compression_threshold);
            }
            Ok(ClientboundLoginPacket::LoginFinished(packet)) => {
                assert_eq!(
                    packet.game_profile.uuid, uuid,
                    "login finished for another UUID"
                );
                assert_eq!(
                    packet.game_profile.name, name,
                    "login finished for another name"
                );
                connection
                    .write(ServerboundLoginAcknowledged {})
                    .await
                    .expect("login acknowledged");
                break;
            }
            Ok(other) => panic!("{name} got an unexpected login packet: {other:?}"),
            Err(error) => panic!("{name} failed to read a login packet: {error}"),
        }
    }

    let mut connection = connection.config();
    connection
        .write(s_client_information::ServerboundClientInformation {
            information: ClientInformation {
                view_distance: options.view_distance,
                ..Default::default()
            },
        })
        .await
        .expect("client information");

    let mut registries = RegistryHolder::default();
    loop {
        match connection.read().await {
            Ok(ClientboundConfigPacket::SelectKnownPacks(packet)) => {
                transcript.known_packs = packet
                    .known_packs
                    .iter()
                    .map(|pack| format!("{}:{}@{}", pack.namespace, pack.id, pack.version))
                    .collect();
                connection
                    .write(ServerboundSelectKnownPacks {
                        known_packs: packet.known_packs,
                    })
                    .await
                    .expect("select known packs");
            }
            Ok(ClientboundConfigPacket::RegistryData(packet)) => {
                transcript.registries.push(packet.registry_id.to_string());
                registries.append(packet.registry_id, packet.entries);
            }
            Ok(ClientboundConfigPacket::UpdateTags(_)) => transcript.tags_received = true,
            Ok(ClientboundConfigPacket::KeepAlive(packet)) => connection
                .write(ConfigKeepAlive { id: packet.id })
                .await
                .expect("configuration keep-alive"),
            Ok(ClientboundConfigPacket::FinishConfiguration(_)) => {
                connection
                    .write(ServerboundFinishConfiguration {})
                    .await
                    .expect("finish configuration");
                break;
            }
            Ok(ClientboundConfigPacket::Disconnect(packet)) => {
                panic!(
                    "{name} was disconnected during configuration: {}",
                    packet.reason
                )
            }
            Ok(_) => {}
            Err(error) => panic!("{name} failed to read a configuration packet: {error}"),
        }
    }

    (connection.game(), transcript, registries)
}

async fn read_loop(
    mut read: ReadConnection<ClientboundGamePacket>,
    writer: Writer,
    shared: Arc<Shared>,
) {
    loop {
        let packet = match read.read().await {
            Ok(packet) => packet,
            Err(error) => {
                let mut inbox = shared.inbox.lock().unwrap();
                inbox.closed = true;
                inbox.failure.get_or_insert_with(|| match *error {
                    azalea_protocol::read::ReadPacketError::ConnectionClosed => {
                        "the server closed the connection".to_string()
                    }
                    error => format!("undecodable packet: {error}"),
                });
                drop(inbox);
                shared.changed.notify_waiters();
                return;
            }
        };

        let replies = {
            let mut inbox = shared.inbox.lock().unwrap();
            *inbox.received.entry(packet.name()).or_default() += 1;
            if let ClientboundGamePacket::Disconnect(disconnect) = &packet {
                inbox
                    .failure
                    .get_or_insert_with(|| format!("kicked: {}", disconnect.reason));
            }
            if let Err(error) = inbox.view.apply(&packet) {
                inbox.failure.get_or_insert(error);
            }
            let replies = replies(&packet, &mut inbox);
            inbox.packets.push_back(packet);
            replies
        };
        shared.changed.notify_waiters();

        let mut writer = writer.lock().await;
        for reply in replies {
            if writer.write(reply).await.is_err() {
                return;
            }
        }
    }
}

fn replies(packet: &ClientboundGamePacket, inbox: &mut Inbox) -> Vec<ServerboundGamePacket> {
    match packet {
        ClientboundGamePacket::KeepAlive(keep_alive) => {
            vec![ServerboundKeepAlive { id: keep_alive.id }.into_variant()]
        }
        ClientboundGamePacket::Ping(ping) if ping.id & BARRIER_BIT == 0 => {
            vec![ServerboundPong { id: ping.id }.into_variant()]
        }
        ClientboundGamePacket::PlayerPosition(teleport) => {
            let mut replies = vec![
                ServerboundAcceptTeleportation { id: teleport.id }.into_variant(),
                ServerboundMovePlayerPosRot {
                    pos: inbox.view.position,
                    look_direction: LookDirection::new(
                        teleport.change.look_direction.y_rot(),
                        teleport.change.look_direction.x_rot(),
                    ),
                    flags: MoveFlags {
                        on_ground: false,
                        horizontal_collision: false,
                    },
                }
                .into_variant(),
            ];
            if !inbox.loaded {
                inbox.loaded = true;
                replies.push(ServerboundPlayerLoaded {}.into_variant());
            }
            replies
        }
        _ => Vec::new(),
    }
}
