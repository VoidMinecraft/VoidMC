//! VMC-ATP-034: an application plugin, installed through the plugin hook and
//! written against the public API only, observes `PlayerJoinEvent`,
//! `PlayerReadyEvent` and `PlayerQuitEvent`, identifies the player on each and
//! reacts to them without destabilizing the server.
//!
//! The plugin is the recording observer of the ATP: every lifecycle event it
//! sees is written to a [`Journal`] with the tick, the player's identity and
//! the server-side state at that moment (connection state, `PlayerReady`).
//! It also records two raw packets, `FinishConfigurationAcknowledged` and
//! `PlayerLoaded`, so the journal shows where in the login sequence each
//! event fired. Its reactions are those of a typical plugin: a welcome
//! message and a starter item for the ready player, join and leave
//! announcements for the others, and a resource listing who is online.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy_app::{App, First};
use bevy_ecs::prelude::*;
use uuid::Uuid;
use voidmc::components::{
    ClientId, ConnectionState, KeepAliveState, MinecraftEntityId, PlayerName, PlayerReady,
    PlayerUuid, Position,
};
use voidmc::events::{PlayerJoinEvent, PlayerQuitEvent, PlayerReadyEvent};
use voidmc::network::PacketEvent;
use voidmc::{
    Audience, Inventory, ItemStack as ServerItemStack, Messages, On, Players, Query, TextColor,
};
use voidmc_e2e::TestServer;
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::common::client_information::ClientInformation;
use voidmc_e2e::protocol::common::movements::MoveFlags;
use voidmc_e2e::protocol::connect::Connection;
use voidmc_e2e::protocol::packets::config::{
    ClientboundConfigPacket, ServerboundConfigPacket,
    s_client_information::ServerboundClientInformation,
    s_finish_configuration::ServerboundFinishConfiguration,
    s_keep_alive::ServerboundKeepAlive as ConfigKeepAlive,
    s_select_known_packs::ServerboundSelectKnownPacks,
};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, ServerboundGamePacket,
    s_accept_teleportation::ServerboundAcceptTeleportation, s_keep_alive::ServerboundKeepAlive,
    s_move_player_pos_rot::ServerboundMovePlayerPosRot, s_player_loaded::ServerboundPlayerLoaded,
    s_pong::ServerboundPong,
};
use voidmc_e2e::protocol::packets::handshake::s_intention::ServerboundIntention;
use voidmc_e2e::protocol::packets::login::{
    ClientboundLoginPacket, s_hello::ServerboundHello,
    s_login_acknowledged::ServerboundLoginAcknowledged,
};
use voidmc_e2e::protocol::packets::{ClientIntention, PROTOCOL_VERSION, Packet, ProtocolPacket};
use voidmc_e2e::protocol::read::ReadPacketError;
use voidmc_e2e::registry::builtin::ItemKind;
use voidmc_protocol::{State, serverbound};

// -------------------------------------------------------------------------
// The plugin
// -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// The raw `FinishConfigurationAcknowledged` packet was dispatched.
    FinishConfiguration,
    Join,
    /// The raw `PlayerLoaded` packet was dispatched.
    Loaded,
    Ready,
    Quit,
}

use Kind::*;

/// One entry of the journal: what the plugin could see when it was called.
#[derive(Clone, Debug)]
struct Seen {
    kind: Kind,
    tick: u64,
    entity: Entity,
    client_id: u32,
    name: Option<String>,
    uuid: Option<Uuid>,
    client_id_component: Option<u32>,
    state: Option<State>,
    ready: bool,
    network_id: Option<i32>,
    has_position: bool,
}

#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<Seen>>>);

impl Journal {
    fn all(&self) -> Vec<Seen> {
        self.0.lock().unwrap().clone()
    }

    fn of(&self, name: &str) -> Vec<Seen> {
        self.all()
            .into_iter()
            .filter(|seen| seen.name.as_deref() == Some(name))
            .collect()
    }

    /// The lifecycle events (not the raw packets) of one player.
    fn events(&self, name: &str) -> Vec<Seen> {
        self.of(name)
            .into_iter()
            .filter(|seen| matches!(seen.kind, Join | Ready | Quit))
            .collect()
    }

    fn kinds(&self, name: &str) -> Vec<Kind> {
        self.events(name).iter().map(|seen| seen.kind).collect()
    }

    fn push(&self, seen: Seen) {
        self.0.lock().unwrap().push(seen);
    }
}

#[derive(Resource, Default)]
struct Tick(u64);

/// The plugin's own state: who is online, filled on join, emptied on quit.
#[derive(Resource, Default, Debug)]
struct Online(BTreeMap<Uuid, Entity>);

type Identity = (
    Option<&'static PlayerName>,
    Option<&'static PlayerUuid>,
    Option<&'static ClientId>,
    Option<&'static ConnectionState>,
    Has<PlayerReady>,
    Option<&'static MinecraftEntityId>,
    Has<Position>,
);

fn observe(
    kind: Kind,
    tick: &Tick,
    players: &Query<Identity>,
    entity: Entity,
    client_id: u32,
) -> Seen {
    let found = players.get(entity).ok();
    Seen {
        kind,
        tick: tick.0,
        entity,
        client_id,
        name: found.and_then(|found| found.0).map(|name| name.0.clone()),
        uuid: found.and_then(|found| found.1).map(|uuid| uuid.0),
        client_id_component: found.and_then(|found| found.2).map(|id| id.0),
        state: found.and_then(|found| found.3).map(|state| state.0),
        ready: found.is_some_and(|found| found.4),
        network_id: found.and_then(|found| found.5).map(|id| id.0),
        has_position: found.is_some_and(|found| found.6),
    }
}

fn name_of(seen: &Seen) -> String {
    seen.name.clone().unwrap_or_else(|| "Someone".to_string())
}

/// The application plugin under test, as `VoidServer::add_plugin` takes it.
fn lifecycle_plugin(journal: Journal) -> impl FnOnce(&mut App) + Send + 'static {
    move |app: &mut App| {
        app.init_resource::<Tick>()
            .init_resource::<Online>()
            .add_systems(First, |mut tick: ResMut<Tick>| tick.0 += 1);

        let log = journal.clone();
        app.add_observer(
            move |event: On<PacketEvent<serverbound::FinishConfigurationAcknowledged>>,
                  tick: Res<Tick>,
                  players: Query<Identity>| {
                log.push(observe(
                    FinishConfiguration,
                    &tick,
                    &players,
                    event.entity,
                    event.client_id,
                ));
            },
        );
        let log = journal.clone();
        app.add_observer(
            move |event: On<PacketEvent<serverbound::PlayerLoaded>>,
                  tick: Res<Tick>,
                  players: Query<Identity>| {
                log.push(observe(
                    Loaded,
                    &tick,
                    &players,
                    event.entity,
                    event.client_id,
                ));
            },
        );

        let log = journal.clone();
        app.add_observer(
            move |event: On<PlayerJoinEvent>,
                  tick: Res<Tick>,
                  players: Query<Identity>,
                  mut online: ResMut<Online>| {
                let seen = observe(Join, &tick, &players, event.entity, event.client_id);
                if let Some(uuid) = seen.uuid {
                    online.0.insert(uuid, event.entity);
                }
                log.push(seen);
            },
        );

        let log = journal.clone();
        app.add_observer(
            move |event: On<PlayerReadyEvent>,
                  tick: Res<Tick>,
                  players: Query<Identity>,
                  messages: Messages,
                  mut inventories: Query<&mut Inventory>| {
                let seen = observe(Ready, &tick, &players, event.entity, event.client_id);
                let name = name_of(&seen);
                let player = event.entity;
                messages
                    .message(player, format!("Welcome, {name}!"))
                    .color(TextColor::Green)
                    .send();
                messages
                    .broadcast(format!("{name} joined the game"))
                    .color(TextColor::Yellow)
                    .audience(Audience::custom(move |recipient| {
                        recipient.entity() != player
                    }))
                    .send();
                if let Ok(mut inventory) = inventories.get_mut(player) {
                    inventory
                        .give(ServerItemStack::of("minecraft:diamond", 3).expect("a known item"));
                }
                log.push(seen);
            },
        );

        let log = journal;
        app.add_observer(
            move |event: On<PlayerQuitEvent>,
                  tick: Res<Tick>,
                  players: Query<Identity>,
                  messages: Messages,
                  mut online: ResMut<Online>| {
                let seen = observe(Quit, &tick, &players, event.entity, event.client_id);
                if let Some(uuid) = seen.uuid {
                    online.0.remove(&uuid);
                }
                let player = event.entity;
                messages
                    .broadcast(format!("{} left the game", name_of(&seen)))
                    .color(TextColor::Yellow)
                    .audience(Audience::custom(move |recipient| {
                        recipient.entity() != player
                    }))
                    .send();
                log.push(seen);
            },
        );
    }
}

async fn start(journal: &Journal) -> TestServer {
    TestServer::builder()
        .plugin(lifecycle_plugin(journal.clone()))
        .start()
        .await
}

async fn online(server: &TestServer) -> BTreeMap<Uuid, Entity> {
    server
        .with_world(|world| world.resource::<Online>().0.clone())
        .await
}

async fn wait_for_kind(server: &TestServer, journal: &Journal, name: &str, kind: Kind) -> Seen {
    let journal = journal.clone();
    let name = name.to_string();
    server
        .wait_for(&format!("{kind:?} of {name} in the journal"), move |_| {
            journal
                .events(&name)
                .into_iter()
                .rev()
                .find(|seen| seen.kind == kind)
        })
        .await
}

/// Checks the identity the plugin saw on a lifecycle event.
fn assert_identity(seen: &Seen, name: &str, uuid: Uuid) {
    assert_eq!(seen.name.as_deref(), Some(name), "{seen:?}");
    assert_eq!(seen.uuid, Some(uuid), "{seen:?}");
    assert_eq!(seen.client_id_component, Some(seen.client_id), "{seen:?}");
    assert_eq!(seen.state, Some(State::Play), "{seen:?}");
    assert!(seen.network_id.is_some() && seen.has_position, "{seen:?}");
}

/// The offline UUID the harness bots use for a name.
fn offline_uuid(name: &str) -> Uuid {
    azalea_crypto::offline::generate_uuid(name)
}

// -------------------------------------------------------------------------
// A client driven by hand, to stop anywhere in the login sequence
// -------------------------------------------------------------------------

const BARRIER_BIT: u32 = 0x8000_0000;

struct Configuring {
    name: String,
    connection: Connection<ClientboundConfigPacket, ServerboundConfigPacket>,
}

/// A client in the play state that only does what the test tells it to.
struct Manual {
    name: String,
    connection: Connection<ClientboundGamePacket, ServerboundGamePacket>,
    /// Every play packet received, by name, in order.
    received: Vec<&'static str>,
    /// Every system chat line received, in order.
    chat: Vec<String>,
    next_barrier: u32,
}

/// Handshake, login and the start of configuration, up to the client
/// information.
async fn log_in(server: &TestServer, name: &str, uuid: Uuid) -> Configuring {
    let address = server.address();
    let mut connection = Connection::new(&address)
        .await
        .unwrap_or_else(|error| panic!("{name} could not connect: {error}"));
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
        match within(name, "a login packet", connection.read()).await {
            Ok(ClientboundLoginPacket::LoginCompression(packet)) => {
                connection.set_compression_threshold(packet.compression_threshold);
            }
            Ok(ClientboundLoginPacket::LoginFinished(_)) => break,
            other => panic!("{name}: unexpected login answer {other:?}"),
        }
    }
    connection
        .write(ServerboundLoginAcknowledged {})
        .await
        .expect("login acknowledged");
    let mut connection = connection.config();
    connection
        .write(ServerboundClientInformation {
            information: ClientInformation::default(),
        })
        .await
        .expect("client information");
    Configuring {
        name: name.to_string(),
        connection,
    }
}

impl Configuring {
    /// Waits for the server's known packs, the first configuration packet.
    async fn known_packs(&mut self) {
        loop {
            match self.read().await {
                ClientboundConfigPacket::SelectKnownPacks(packet) => {
                    self.connection
                        .write(ServerboundSelectKnownPacks {
                            known_packs: packet.known_packs,
                        })
                        .await
                        .expect("select known packs");
                    return;
                }
                ClientboundConfigPacket::KeepAlive(packet) => self.keep_alive(packet.id).await,
                _ => {}
            }
        }
    }

    /// Finishes configuration: answers the server's last configuration
    /// packet with the acknowledgement that moves the connection to play.
    async fn finish(mut self) -> Manual {
        loop {
            match self.read().await {
                ClientboundConfigPacket::SelectKnownPacks(packet) => self
                    .connection
                    .write(ServerboundSelectKnownPacks {
                        known_packs: packet.known_packs,
                    })
                    .await
                    .expect("select known packs"),
                ClientboundConfigPacket::KeepAlive(packet) => self.keep_alive(packet.id).await,
                ClientboundConfigPacket::FinishConfiguration(_) => break,
                _ => {}
            }
        }
        self.connection
            .write(ServerboundFinishConfiguration {})
            .await
            .expect("finish configuration");
        Manual {
            name: self.name,
            connection: self.connection.game(),
            received: Vec::new(),
            chat: Vec::new(),
            next_barrier: 1,
        }
    }

    async fn keep_alive(&mut self, id: u64) {
        self.connection
            .write(ConfigKeepAlive { id })
            .await
            .expect("configuration keep-alive");
    }

    async fn read(&mut self) -> ClientboundConfigPacket {
        within(&self.name, "a configuration packet", self.connection.read())
            .await
            .unwrap_or_else(|error| panic!("{}: configuration read failed: {error}", self.name))
    }
}

impl Manual {
    /// Reads one play packet, answering keep-alives like any client.
    async fn read(&mut self) -> Result<ClientboundGamePacket, String> {
        let packet = within(&self.name, "a play packet", self.connection.read())
            .await
            .map_err(|error| match *error {
                ReadPacketError::ConnectionClosed => "closed".to_string(),
                error => format!("read failed: {error}"),
            })?;
        self.received.push(packet_name(&packet));
        match &packet {
            ClientboundGamePacket::KeepAlive(keep_alive) => {
                self.send(ServerboundKeepAlive { id: keep_alive.id }).await;
            }
            ClientboundGamePacket::SystemChat(chat) => self.chat.push(chat.content.to_string()),
            _ => {}
        }
        Ok(packet)
    }

    async fn expect<T>(
        &mut self,
        what: &str,
        mut f: impl FnMut(&ClientboundGamePacket) -> Option<T>,
    ) -> T {
        loop {
            let packet = self
                .read()
                .await
                .unwrap_or_else(|error| panic!("{} waiting for {what}: {error}", self.name));
            if let Some(found) = f(&packet) {
                return found;
            }
        }
    }

    async fn send(&mut self, packet: impl Packet<ServerboundGamePacket>) {
        let packet = packet.into_variant();
        let name = self.name.clone();
        self.connection
            .write(packet)
            .await
            .unwrap_or_else(|error| panic!("{name} could not send: {error}"));
    }

    /// Returns once the server has handled every packet sent before, and
    /// finished that tick (the harness answers the barrier at its end).
    async fn sync(&mut self) {
        let id = BARRIER_BIT | self.next_barrier;
        self.next_barrier += 1;
        self.send(ServerboundPong { id }).await;
        self.expect("the sync barrier", |packet| match packet {
            ClientboundGamePacket::Ping(ping) if ping.id == id => Some(()),
            _ => None,
        })
        .await;
    }

    /// Reads until both the plugin's welcome message and its starter item
    /// have arrived, in whichever order.
    async fn welcome_and_diamonds(&mut self) {
        let diamonds = ItemStack::Present(ItemStackData::new(ItemKind::Diamond, 3));
        let welcome = format!("Welcome, {}!", self.name);
        let (mut greeted, mut given) = (false, false);
        while !(greeted && given) {
            let packet = self
                .read()
                .await
                .unwrap_or_else(|error| panic!("{} waiting for the welcome: {error}", self.name));
            match packet {
                ClientboundGamePacket::SystemChat(chat) => {
                    greeted |= chat.content.to_string().contains(&welcome);
                }
                ClientboundGamePacket::ContainerSetSlot(slot) if slot.container_id == 0 => {
                    given |= slot.item_stack == diamonds;
                }
                ClientboundGamePacket::ContainerSetContent(content)
                    if content.container_id == 0 =>
                {
                    given |= content.items.contains(&diamonds);
                }
                _ => {}
            }
        }
    }

    /// Reads until the server closes the connection.
    async fn closed(&mut self) {
        loop {
            match self.read().await {
                Ok(_) => {}
                Err(error) if error == "closed" => return,
                Err(error) => panic!("{}: {error}", self.name),
            }
        }
    }
}

fn packet_name(packet: &ClientboundGamePacket) -> &'static str {
    packet.name()
}

async fn within<T>(who: &str, what: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(15), future)
        .await
        .unwrap_or_else(|_| panic!("{who}: timed out waiting for {what}"))
}

// -------------------------------------------------------------------------
// Scenarios
// -------------------------------------------------------------------------

/// Steps 1 to 5 of the ATP with one client driven by hand: the journal and
/// the packets the client received show that the join event fires once the
/// finish-configuration acknowledgement has moved the connection to play and
/// before the player is ready, that the ready event waits for PlayerLoaded,
/// and that the quit event follows the disconnect with the identity intact.
#[tokio::test]
async fn atp_034_events_fire_in_login_order_with_the_player_identity() {
    let journal = Journal::default();
    let server = start(&journal).await;
    let uuid = offline_uuid("Alice");

    let mut configuring = log_in(&server, "Alice", uuid).await;
    configuring.known_packs().await;
    assert!(
        journal.all().is_empty(),
        "nothing fires during login and configuration: {:#?}",
        journal.all()
    );

    // Configuration -> play. The server answers with the play login, the
    // chunks and the position; by the end of that tick PlayerJoinEvent has
    // fired, after the acknowledgement was dispatched, in the play state,
    // and before PlayerReady.
    let mut alice = configuring.finish().await;
    let login = alice
        .expect("the play login", |packet| match packet {
            ClientboundGamePacket::Login(login) => Some(login.player_id.0),
            _ => None,
        })
        .await;
    let teleport = alice
        .expect("the spawn position", |packet| match packet {
            ClientboundGamePacket::PlayerPosition(teleport) => Some(teleport.clone()),
            _ => None,
        })
        .await;
    alice.sync().await;
    let seen = journal.of("Alice");
    assert_eq!(
        seen.iter().map(|seen| seen.kind).collect::<Vec<_>>(),
        [FinishConfiguration, Join],
        "{seen:#?}"
    );
    let (ack, join) = (&seen[0], &seen[1]);
    assert_eq!(
        ack.state,
        Some(State::Configuration),
        "the acknowledgement is dispatched in configuration"
    );
    assert_identity(join, "Alice", uuid);
    assert!(!join.ready, "PlayerReady is not set on join");
    assert_eq!(join.entity, ack.entity);
    assert_eq!(join.network_id, Some(login), "the id the client was given");
    assert_eq!(online(&server).await.get(&uuid), Some(&join.entity));
    assert!(
        alice.received.contains(&"level_chunk_with_light"),
        "the initial chunks arrived with the play login: {:?}",
        alice.received
    );

    // Confirming the teleport and moving is not loading: still not ready.
    alice
        .send(ServerboundAcceptTeleportation { id: teleport.id })
        .await;
    alice
        .send(ServerboundMovePlayerPosRot {
            pos: teleport.change.pos,
            look_direction: teleport.change.look_direction,
            flags: MoveFlags::default(),
        })
        .await;
    alice.sync().await;
    assert_eq!(
        journal.kinds("Alice"),
        [Join],
        "no ready before PlayerLoaded"
    );
    assert!(
        !alice.chat.iter().any(|line| line.contains("Welcome")),
        "{:?}",
        alice.chat
    );

    // PlayerLoaded -> ready, once; the welcome message and the starter item
    // reach the client.
    alice.send(ServerboundPlayerLoaded {}).await;
    alice.welcome_and_diamonds().await;
    alice.sync().await;
    let seen = journal.of("Alice");
    assert_eq!(
        seen.iter().map(|seen| seen.kind).collect::<Vec<_>>(),
        [FinishConfiguration, Join, Loaded, Ready],
        "{seen:#?}"
    );
    let (loaded, ready) = (&seen[2], &seen[3]);
    assert!(
        !loaded.ready,
        "PlayerLoaded is dispatched before PlayerReady"
    );
    assert_identity(ready, "Alice", uuid);
    assert!(ready.ready, "PlayerReady is set when the ready event fires");
    assert_eq!(ready.entity, join.entity);
    assert!(join.tick < ready.tick, "{join:?} {ready:?}");

    // A second PlayerLoaded (a buggy or hostile client) does not make the
    // player ready again.
    alice.send(ServerboundPlayerLoaded {}).await;
    alice.sync().await;
    assert_eq!(journal.kinds("Alice"), [Join, Ready]);

    // Disconnect: the quit event fires once, while the entity and its
    // components can still be read, then the entity is despawned.
    drop(alice);
    let quit = wait_for_kind(&server, &journal, "Alice", Quit).await;
    assert_identity(&quit, "Alice", uuid);
    assert!(quit.ready, "the quitting player was ready");
    assert_eq!(quit.entity, join.entity);
    assert!(ready.tick <= quit.tick);
    let entity = join.entity;
    server
        .wait_for("Alice's entity to be despawned", move |world| {
            world.get_entity(entity).is_err().then_some(())
        })
        .await;
    server.wait_ticks(2).await;
    assert_eq!(journal.kinds("Alice"), [Join, Ready, Quit]);
    assert!(online(&server).await.is_empty());

    // The recorded journal (`-- --nocapture` shows it).
    println!("VMC-ATP-034 journal:");
    for seen in journal.all() {
        println!(
            "  tick {:>3} {:<19} {:?} client {} name {:?} uuid {:?} state {:?} ready {}",
            seen.tick,
            format!("{:?}", seen.kind),
            seen.entity,
            seen.client_id,
            seen.name,
            seen.uuid,
            seen.state,
            seen.ready
        );
    }

    server.stop().await;
}

/// A client that leaves before it is ready: during configuration nothing has
/// fired and nothing fires; after the join event, a client that never sends
/// PlayerLoaded never becomes ready, and its disconnect fires the quit event
/// that pairs with its join, without a ready event.
#[tokio::test]
async fn atp_034_leaving_before_ready_pairs_join_with_quit_and_never_readies() {
    let journal = Journal::default();
    let server = start(&journal).await;

    // Leaves during configuration: no event at all, no entity left over.
    let mut early = log_in(&server, "Early", offline_uuid("Early")).await;
    early.known_packs().await;
    let early_entity = server.player_entity(offline_uuid("Early")).await;
    drop(early);
    server
        .wait_for("Early's entity to be despawned", move |world| {
            world.get_entity(early_entity).is_err().then_some(())
        })
        .await;

    // Joins, then never loads.
    let uuid = offline_uuid("Loader");
    let mut loader = log_in(&server, "Loader", uuid).await.finish().await;
    let teleport = loader
        .expect("the spawn position", |packet| match packet {
            ClientboundGamePacket::PlayerPosition(teleport) => Some(teleport.id),
            _ => None,
        })
        .await;
    loader
        .send(ServerboundAcceptTeleportation { id: teleport })
        .await;
    loader.sync().await;
    let join = wait_for_kind(&server, &journal, "Loader", Join).await;
    assert!(!join.ready);
    server.wait_ticks(5).await;
    loader.sync().await;
    assert_eq!(
        journal.kinds("Loader"),
        [Join],
        "no ready without PlayerLoaded"
    );
    assert!(!entity_ready(&server, join.entity).await);
    assert!(
        !loader.chat.iter().any(|line| line.contains("Welcome")),
        "{:?}",
        loader.chat
    );

    drop(loader);
    let quit = wait_for_kind(&server, &journal, "Loader", Quit).await;
    assert_identity(&quit, "Loader", uuid);
    assert!(
        !quit.ready,
        "the quit event tells a player who never got ready"
    );
    assert_eq!(quit.entity, join.entity);
    server
        .wait_for("Loader's entity to be despawned", move |world| {
            world.get_entity(join.entity).is_err().then_some(())
        })
        .await;
    server.wait_ticks(2).await;
    assert_eq!(journal.kinds("Loader"), [Join, Quit]);
    assert!(journal.of("Early").is_empty(), "{:#?}", journal.of("Early"));
    assert!(
        online(&server).await.is_empty(),
        "the plugin's state does not leak"
    );

    server.stop().await;
}

async fn entity_ready(server: &TestServer, entity: Entity) -> bool {
    server
        .with_world(move |world| world.get::<PlayerReady>(entity).is_some())
        .await
}

/// Each player's own sequence is join, ready, quit, exactly once each, with
/// its own identity, while players join and leave in interleaved order; a
/// player who comes back with the same name gets a fresh sequence on a new
/// entity. The plugin's reactions reach the right players and leave everyone
/// else healthy.
#[tokio::test]
async fn atp_034_interleaved_players_each_get_one_join_ready_quit() {
    let journal = Journal::default();
    let server = start(&journal).await;

    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    alice
        .expect("Bob's join announcement", chat_line("Bob joined the game"))
        .await;
    bob.expect("Bob's welcome", chat_line("Welcome, Bob!"))
        .await;
    let alice_uuid = alice.uuid();
    alice.disconnect().await;
    bob.expect(
        "Alice's leave announcement",
        chat_line("Alice left the game"),
    )
    .await;

    let carol = server.join("Carol").await;
    bob.expect(
        "Carol's join announcement",
        chat_line("Carol joined the game"),
    )
    .await;
    let alice_again = server.join("Alice").await;
    for other in [&bob, &carol] {
        other
            .expect("Alice's second join", chat_line("Alice joined the game"))
            .await;
    }
    bob.disconnect().await;
    for other in [&carol, &alice_again] {
        other
            .expect("Bob's leave announcement", chat_line("Bob left the game"))
            .await;
    }
    carol.sync().await;
    alice_again.sync().await;
    carol.assert_healthy();
    alice_again.assert_healthy();
    assert_eq!(
        carol.queued(chat_line("Welcome, Alice!")),
        [],
        "the welcome goes to the joining player only"
    );
    carol.disconnect().await;
    alice_again.disconnect().await;
    wait_for_count(&server, &journal, Quit, 4).await;
    server.wait_ticks(2).await;

    let sessions = sessions(&journal);
    let names: Vec<(&str, Vec<Kind>)> = sessions
        .iter()
        .map(|(name, events)| (name.as_str(), events.iter().map(|seen| seen.kind).collect()))
        .collect();
    assert_eq!(
        names,
        [
            ("Alice", vec![Join, Ready, Quit]),
            ("Bob", vec![Join, Ready, Quit]),
            ("Carol", vec![Join, Ready, Quit]),
            ("Alice", vec![Join, Ready, Quit]),
        ],
        "one sequence per session, in order of joining"
    );
    for (name, events) in &sessions {
        let uuid = offline_uuid(name);
        for seen in events {
            assert_identity(seen, name, uuid);
            assert_eq!(seen.entity, events[0].entity);
            assert_eq!(seen.client_id, events[0].client_id);
        }
        assert!(!events[0].ready && events[1].ready && events[2].ready);
        assert!(events[0].tick < events[1].tick && events[1].tick <= events[2].tick);
    }
    assert_ne!(
        sessions[0].1[0].entity, sessions[3].1[0].entity,
        "the second Alice is a fresh entity"
    );
    assert_ne!(sessions[0].1[0].client_id, sessions[3].1[0].client_id);
    assert_eq!(sessions[3].1[0].uuid, Some(alice_uuid));
    assert!(online(&server).await.is_empty());

    server.stop().await;
}

/// The journal's lifecycle events grouped by entity, in order of joining.
fn sessions(journal: &Journal) -> Vec<(String, Vec<Seen>)> {
    let mut sessions: Vec<(String, Vec<Seen>)> = Vec::new();
    for seen in journal.all() {
        if !matches!(seen.kind, Join | Ready | Quit) {
            continue;
        }
        match sessions
            .iter_mut()
            .find(|(_, events)| events[0].entity == seen.entity)
        {
            Some((_, events)) => events.push(seen),
            None => sessions.push((name_of(&seen), vec![seen])),
        }
    }
    sessions
}

async fn wait_for_count(server: &TestServer, journal: &Journal, kind: Kind, count: usize) {
    let journal = journal.clone();
    server
        .wait_for(&format!("{count} {kind:?} events"), move |_| {
            let seen = journal
                .all()
                .iter()
                .filter(|seen| seen.kind == kind)
                .count();
            (seen >= count).then_some(())
        })
        .await;
}

fn chat_line(text: &str) -> impl FnMut(&ClientboundGamePacket) -> Option<()> + use<> {
    let text = text.to_string();
    move |packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => {
            chat.content.to_string().contains(&text).then_some(())
        }
        _ => None,
    }
}

fn disconnect_reason(packet: &ClientboundGamePacket) -> Option<String> {
    match packet {
        ClientboundGamePacket::Disconnect(disconnect) => Some(disconnect.reason.to_string()),
        _ => None,
    }
}

/// The quit event fires exactly once however a ready player leaves: kicked
/// with `/kick` (the server closes the connection itself, even though the
/// harness bot ignores the Disconnect packet like a misbehaving client), timed
/// out on keep-alive, or still online when the server shuts down.
#[tokio::test]
async fn atp_034_quit_fires_once_for_kick_timeout_and_shutdown() {
    let journal = Journal::default();
    let server = start(&journal).await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let carol = server.join("Carol").await;
    let dave = server.join("Dave").await;
    let bob_entity = wait_for_kind(&server, &journal, "Bob", Ready).await.entity;
    let carol_entity = wait_for_kind(&server, &journal, "Carol", Ready)
        .await
        .entity;

    // Kick: Bob reads the reason, then the server ends his session although
    // the bot never closes its side.
    alice.command("kick Bob Bye now").await;
    let reason = bob.expect("the kick", disconnect_reason).await;
    assert!(reason.contains("Bye now"), "{reason}");
    let quit = wait_for_kind(&server, &journal, "Bob", Quit).await;
    assert_identity(&quit, "Bob", bob.uuid());
    assert!(quit.ready);
    assert_eq!(quit.entity, bob_entity);
    alice
        .expect("Bob's leave announcement", chat_line("Bob left the game"))
        .await;
    alice
        .wait_until("Bob gone from the tab list", |view| {
            !view.players.contains_key(&offline_uuid("Bob"))
        })
        .await;

    // Keep-alive timeout: Carol's keep-alive is made 31 s overdue.
    server
        .with_world(move |world| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let mut state = world
                .get_mut::<KeepAliveState>(carol_entity)
                .expect("Carol has a keep-alive state");
            state.awaiting_response = true;
            state.last_sent_id = now - 31_000;
        })
        .await;
    let reason = carol.expect("the time-out", disconnect_reason).await;
    assert!(reason.contains("Timed out"), "{reason}");
    let quit = wait_for_kind(&server, &journal, "Carol", Quit).await;
    assert_identity(&quit, "Carol", carol.uuid());
    assert_eq!(quit.entity, carol_entity);
    for entity in [bob_entity, carol_entity] {
        server
            .wait_for("the kicked player to be despawned", move |world| {
                world.get_entity(entity).is_err().then_some(())
            })
            .await;
    }
    alice.sync().await;
    dave.sync().await;
    alice.assert_healthy();
    dave.assert_healthy();
    assert_eq!(journal.kinds("Bob"), [Join, Ready, Quit]);
    assert_eq!(journal.kinds("Carol"), [Join, Ready, Quit]);

    // Shutdown: Alice and Dave are still online and both quit, while their
    // identity can be read.
    assert_eq!(journal.kinds("Alice"), [Join, Ready]);
    server.stop().await;
    for bot in [&alice, &dave] {
        let events = journal.events(bot.name());
        assert_eq!(
            events.iter().map(|seen| seen.kind).collect::<Vec<_>>(),
            [Join, Ready, Quit],
            "{}",
            bot.name()
        );
        assert_identity(&events[2], bot.name(), bot.uuid());
        assert!(events[2].ready);
    }
}

/// Observers that kick or despawn the player from their ready handler: the
/// kicked client reads the reason and is disconnected, with its quit event;
/// the despawned one has its connection closed and is no longer dispatched
/// to. The server does not panic or log an error, and the other player stays
/// healthy.
#[tokio::test]
async fn atp_034_an_observer_that_kicks_or_despawns_the_player_does_not_break_the_server() {
    let journal = Journal::default();
    let server = TestServer::builder()
        .plugin(lifecycle_plugin(journal.clone()))
        .plugin(|app| {
            app.add_observer(
                |event: On<PlayerReadyEvent>,
                 names: Query<&PlayerName>,
                 players: Players,
                 mut commands: Commands| {
                    match names.get(event.entity).map(|name| name.0.as_str()) {
                        Ok("Mallory") => players.kick(event.entity, "Mallory is not welcome"),
                        Ok("Eve") => commands.entity(event.entity).despawn(),
                        _ => {}
                    }
                },
            );
        })
        .start()
        .await;
    let alice = server.join("Alice").await;

    let mut mallory = log_in(&server, "Mallory", offline_uuid("Mallory"))
        .await
        .finish()
        .await;
    mallory.send(ServerboundPlayerLoaded {}).await;
    let reason = mallory.expect("the kick", disconnect_reason).await;
    assert!(reason.contains("Mallory is not welcome"), "{reason}");
    mallory.closed().await;
    let quit = wait_for_kind(&server, &journal, "Mallory", Quit).await;
    assert_identity(&quit, "Mallory", offline_uuid("Mallory"));
    assert_eq!(journal.kinds("Mallory"), [Join, Ready, Quit]);

    let mut eve = log_in(&server, "Eve", offline_uuid("Eve"))
        .await
        .finish()
        .await;
    eve.send(ServerboundPlayerLoaded {}).await;
    let ready = wait_for_kind(&server, &journal, "Eve", Ready).await;
    server
        .wait_for("Eve's entity to be despawned", move |world| {
            world.get_entity(ready.entity).is_err().then_some(())
        })
        .await;
    // Packets from Eve after the despawn reach no entity.
    eve.send(ServerboundPong {
        id: BARRIER_BIT | 99,
    })
    .await;
    eve.closed().await;
    assert_eq!(journal.kinds("Eve"), [Join, Ready]);

    alice.sync().await;
    alice.assert_healthy();
    let _bob = server.join("Bob").await;
    alice
        .expect("Bob's join announcement", chat_line("Bob joined the game"))
        .await;
    server.stop().await;
}
