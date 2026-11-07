//! The proxy on real sockets, between a fake client and a fake server, all on ephemeral ports.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, sleep, timeout};
use uuid::Uuid;
use voidmc_codec::{Encode, VarI32};
use voidmc_data::v26_1_2::packets::configuration::{
    clientbound as config_cb, serverbound as config_sb,
};
use voidmc_data::v26_1_2::packets::login::{clientbound as login_cb, serverbound as login_sb};
use voidmc_data::v26_1_2::packets::play::{clientbound as play_cb, serverbound as play_sb};
use voidmc_data::v26_1_2::packets::status::{clientbound as status_cb, serverbound as status_sb};
use voidmc_keeper::proxy::{Events, run};
use voidmc_keeper::session::Settings;
use voidmc_keeper::wire::{Framing, frame, split, tagged};
use voidmc_protocol::PROTOCOL_VERSION;
use voidmc_protocol::clientbound::{Login, PlayPacket};

const PATIENCE: Duration = Duration::from_secs(10);

fn settings() -> Settings {
    Settings {
        return_timeout: Duration::from_secs(8),
        retry_every: Duration::from_millis(20),
        keep_alive_every: Duration::from_millis(100),
        settle: Duration::from_millis(200),
        backend_silence: Duration::from_secs(60),
    }
}

fn id_of(frame: &[u8]) -> i32 {
    split(frame).expect("every frame has an id").0
}

fn empty(id: i32) -> Bytes {
    frame(id, |_| {})
}

fn login_packet(entity_id: i32) -> Bytes {
    tagged(&PlayPacket::Login(Login {
        entity_id,
        is_hardcore: false,
        dimension_names: vec!["minecraft:overworld".to_string()],
        max_players: 10,
        view_distance: 10,
        simulation_distance: 10,
        reduced_debug_info: false,
        enable_respawn_screen: true,
        do_limited_crafting: false,
        dimension_type: 0,
        dimension_name: "minecraft:overworld".to_string(),
        hashed_seed: 0,
        game_mode: 1,
        previous_game_mode: -1,
        is_debug: false,
        is_flat: false,
        last_death_location: None,
        portal_cooldown: 0,
        sea_level: 63,
        enforces_secure_chat: false,
    }))
}

fn registry(tag: u8) -> Bytes {
    frame(config_cb::REGISTRY_DATA, |body| {
        body.push(tag);
        body.extend_from_slice(&[7; 4000]);
    })
}

fn add_entity(id: i32) -> Bytes {
    frame(play_cb::ADD_ENTITY, |body| VarI32(id).encode(body))
}

fn chat(content: u8) -> Bytes {
    frame(play_cb::SYSTEM_CHAT, |body| body.push(content))
}

struct Peer {
    read: OwnedReadHalf,
    write: OwnedWriteHalf,
    inbound: BytesMut,
    framing: Framing,
}

impl Peer {
    fn new(stream: TcpStream) -> Self {
        let (read, write) = stream.into_split();
        Self {
            read,
            write,
            inbound: BytesMut::new(),
            framing: Framing::default(),
        }
    }

    async fn next(&mut self) -> Option<Bytes> {
        loop {
            if let Some(frame) = self.framing.decode(&mut self.inbound).ok()? {
                return Some(frame);
            }
            if self.read.read_buf(&mut self.inbound).await.ok()? == 0 {
                return None;
            }
        }
    }

    async fn send(&mut self, frame: &[u8]) {
        let mut out = Vec::new();
        self.framing.encode(frame, &mut out);
        let _ = self.write.write_all(&out).await;
    }
}

#[derive(Clone)]
struct Plan {
    player: i32,
    registry: u8,
    compression: Option<i32>,
    after_login: Vec<Bytes>,
}

impl Plan {
    fn player(player: i32) -> Self {
        Self {
            player,
            registry: 1,
            compression: None,
            after_login: Vec::new(),
        }
    }
}

struct FakeServer {
    address: SocketAddr,
    task: JoinHandle<()>,
    logins: Arc<AtomicUsize>,
    received: mpsc::UnboundedReceiver<Bytes>,
    broadcast: broadcast::Sender<Bytes>,
    stop: watch::Sender<bool>,
}

impl FakeServer {
    async fn start(address: SocketAddr, plan: Plan) -> Self {
        let listener = TcpListener::bind(address).await.unwrap();
        let address = listener.local_addr().unwrap();
        let logins = Arc::new(AtomicUsize::new(0));
        let (received_tx, received) = mpsc::unbounded_channel();
        let (broadcast, _) = broadcast::channel(64);
        let (stop, stopping) = watch::channel(false);
        let task = tokio::spawn({
            let logins = logins.clone();
            let broadcast = broadcast.clone();
            let mut stopping = stopping.clone();
            async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        accepted = listener.accept() => {
                            let (stream, _) = accepted.unwrap();
                            connections.spawn(serve_fake(
                                Peer::new(stream),
                                plan.clone(),
                                logins.clone(),
                                received_tx.clone(),
                                (broadcast.subscribe(), stopping.clone()),
                            ));
                        }
                        _ = stopping.changed() => {
                            drop(listener);
                            while connections.join_next().await.is_some() {}
                            return;
                        }
                    }
                }
            }
        });
        Self {
            address,
            task,
            logins,
            received,
            broadcast,
            stop,
        }
    }

    fn logins(&self) -> usize {
        self.logins.load(Ordering::SeqCst)
    }

    fn send(&self, frame: Bytes) {
        self.broadcast.send(frame).unwrap();
    }

    async fn crash(self) -> SocketAddr {
        self.task.abort();
        let _ = self.task.await;
        self.address
    }

    /// What the engine does on a clean stop: a last word in the chat, then every connection
    /// closes without a disconnect.
    async fn stop(self) -> SocketAddr {
        self.stop.send(true).unwrap();
        self.task.await.unwrap();
        self.address
    }

    async fn expect(&mut self, id: i32) -> Bytes {
        timeout(PATIENCE, async {
            loop {
                let frame = self.received.recv().await.expect("server is up");
                if id_of(&frame) == id {
                    return frame;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("server never received packet {id:#x}"))
    }
}

async fn serve_fake(
    mut peer: Peer,
    plan: Plan,
    logins: Arc<AtomicUsize>,
    received: mpsc::UnboundedSender<Bytes>,
    (mut broadcast, mut stopping): (broadcast::Receiver<Bytes>, watch::Receiver<bool>),
) {
    let Some(handshake) = peer.next().await else {
        return;
    };
    let is_status = handshake.last() == Some(&1);
    if is_status {
        while let Some(frame_in) = peer.next().await {
            if id_of(&frame_in) == status_sb::STATUS_REQUEST {
                peer.send(&frame(status_cb::STATUS_RESPONSE, |body| {
                    "{}".to_string().encode(body);
                }))
                .await;
            }
        }
        return;
    }
    let Some(_hello) = peer.next().await else {
        return;
    };
    logins.fetch_add(1, Ordering::SeqCst);
    if let Some(threshold) = plan.compression {
        peer.send(&frame(login_cb::LOGIN_COMPRESSION, |body| {
            VarI32(threshold).encode(body);
        }))
        .await;
        peer.framing.set_threshold(threshold);
    }
    peer.send(&login_finished()).await;
    while let Some(frame_in) = peer.next().await {
        if id_of(&frame_in) == login_sb::LOGIN_ACKNOWLEDGED {
            break;
        }
    }
    peer.send(&frame(config_cb::SELECT_KNOWN_PACKS, |body| body.push(0)))
        .await;
    while let Some(frame_in) = peer.next().await {
        if id_of(&frame_in) == config_sb::SELECT_KNOWN_PACKS {
            break;
        }
    }
    peer.send(&registry(plan.registry)).await;
    peer.send(&empty(config_cb::FINISH_CONFIGURATION)).await;
    while let Some(frame_in) = peer.next().await {
        if id_of(&frame_in) == config_sb::FINISH_CONFIGURATION {
            break;
        }
    }
    peer.send(&login_packet(plan.player)).await;
    for frame_out in &plan.after_login {
        peer.send(frame_out).await;
    }
    loop {
        tokio::select! {
            frame_in = peer.next() => match frame_in {
                Some(frame_in) => { let _ = received.send(frame_in); }
                None => return,
            },
            frame_out = broadcast.recv() => match frame_out {
                Ok(frame_out) => peer.send(&frame_out).await,
                Err(_) => return,
            },
            _ = stopping.changed() => {
                peer.send(&chat(0)).await;
                let _ = peer.write.shutdown().await;
                return;
            }
        }
    }
}

struct FakeClient {
    peer: Peer,
}

impl FakeClient {
    async fn connect(proxy: SocketAddr, next_state: i32, protocol: i32) -> Self {
        let stream = TcpStream::connect(proxy).await.unwrap();
        let mut peer = Peer::new(stream);
        peer.send(&frame(0, |body| {
            VarI32(protocol).encode(body);
            "localhost".to_string().encode(body);
            25565u16.encode(body);
            VarI32(next_state).encode(body);
        }))
        .await;
        Self { peer }
    }

    /// Goes through login and configuration, as a vanilla client does, up to the `login`.
    async fn join(proxy: SocketAddr, name: &str) -> (Self, Bytes) {
        let mut client = Self::connect(proxy, 2, PROTOCOL_VERSION).await;
        client
            .peer
            .send(&frame(login_sb::HELLO, |body| {
                name.to_string().encode(body);
                Uuid::from_u128(name.len() as u128).encode(body);
            }))
            .await;
        client.expect(login_cb::LOGIN_FINISHED).await;
        client.peer.send(&empty(login_sb::LOGIN_ACKNOWLEDGED)).await;
        client
            .peer
            .send(&frame(config_sb::CLIENT_INFORMATION, |body| body.push(1)))
            .await;
        let login = client.configure().await;
        client.peer.send(&empty(play_sb::PLAYER_LOADED)).await;
        (client, login)
    }

    /// Answers a configuration up to the `login` that ends it.
    async fn configure(&mut self) -> Bytes {
        let mut registries = HashSet::new();
        loop {
            let frame_in = self.next().await;
            match id_of(&frame_in) {
                config_cb::REGISTRY_DATA => {
                    assert!(
                        registries.insert(frame_in),
                        "a registry reached the client twice"
                    );
                }
                config_cb::SELECT_KNOWN_PACKS => {
                    self.peer
                        .send(&frame(config_sb::SELECT_KNOWN_PACKS, |body| body.push(0)))
                        .await;
                }
                config_cb::FINISH_CONFIGURATION => {
                    self.peer
                        .send(&empty(config_sb::FINISH_CONFIGURATION))
                        .await;
                    return self.expect(play_cb::LOGIN).await;
                }
                _ => {}
            }
        }
    }

    /// Goes through the configuration the keeper starts once a server is back, up to its `login`.
    async fn come_back(&mut self) -> Bytes {
        self.expect(play_cb::START_CONFIGURATION).await;
        self.peer
            .send(&empty(play_sb::CONFIGURATION_ACKNOWLEDGED))
            .await;
        let login = self.configure().await;
        self.peer.send(&empty(play_sb::PLAYER_LOADED)).await;
        login
    }

    async fn next(&mut self) -> Bytes {
        timeout(PATIENCE, self.peer.next())
            .await
            .expect("client waited too long")
            .expect("client was disconnected")
    }

    async fn expect(&mut self, id: i32) -> Bytes {
        timeout(PATIENCE, async {
            loop {
                let frame = self.peer.next().await.expect("client was disconnected");
                if id_of(&frame) == id {
                    return frame;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("client never received packet {id:#x}"))
    }

    /// Everything the client receives until `id`, which is included.
    async fn collect_until(&mut self, id: i32) -> Vec<Bytes> {
        let mut seen = Vec::new();
        loop {
            let frame = self.next().await;
            let done = id_of(&frame) == id;
            seen.push(frame);
            if done {
                return seen;
            }
        }
    }
}

struct Rig {
    proxy: SocketAddr,
    events: mpsc::UnboundedReceiver<Value>,
}

impl Rig {
    async fn start(backend: SocketAddr) -> Self {
        Self::start_with(backend, settings()).await
    }

    async fn start_with(backend: SocketAddr, settings: Settings) -> Self {
        let listener = TcpListener::bind("[::1]:0").await.unwrap();
        let proxy = listener.local_addr().unwrap();
        let (sender, events) = mpsc::unbounded_channel();
        let sink = Events::new(move |event| {
            let _ = sender.send(event);
        });
        tokio::spawn(run(listener, backend, settings, sink));
        Self { proxy, events }
    }

    async fn expect_event(&mut self, name: &str) -> Value {
        timeout(PATIENCE, async {
            loop {
                let event = self.events.recv().await.expect("proxy is up");
                if event["event"] == name {
                    return event;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("event {name} never came"))
    }
}

/// The next connection that logs in, past the status probes of the proxy.
fn login_finished() -> Bytes {
    frame(login_cb::LOGIN_FINISHED, |body| {
        uuid::Uuid::from_u128(77).encode(body);
    })
}

async fn accept_login(listener: &TcpListener) -> Peer {
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let mut peer = Peer::new(stream);
        let handshake = peer.next().await.unwrap();
        if handshake.last() == Some(&2) {
            return peer;
        }
    }
}

async fn rig_with(plan: Plan) -> (Rig, FakeServer) {
    let server = FakeServer::start("[::1]:0".parse().unwrap(), plan).await;
    let rig = Rig::start(server.address).await;
    (rig, server)
}

#[tokio::test]
async fn a_client_survives_a_crash_through_a_configuration() {
    let mut plan = Plan::player(1);
    plan.after_login = vec![add_entity(50)];
    let (mut rig, mut server) = rig_with(plan).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    client.expect(play_cb::ADD_ENTITY).await;
    server.expect(play_sb::PLAYER_LOADED).await;

    let address = server.crash().await;
    client.expect(play_cb::KEEP_ALIVE).await;
    client.expect(play_cb::SET_ACTION_BAR_TEXT).await;
    rig.expect_event("backend_lost").await;

    let mut plan = Plan::player(9);
    plan.registry = 2;
    let mut server = FakeServer::start(address, plan).await;
    assert_eq!(client.come_back().await, login_packet(9));
    let resumed = rig.expect_event("resumed").await;
    assert!(resumed["outage_ms"].as_u64().is_some());
    assert_eq!(
        client.next().await,
        frame(play_cb::SET_ACTION_BAR_TEXT, |body| {
            voidmc_keeper::wire::text("").encode(body);
        })
    );

    server.expect(play_sb::PLAYER_LOADED).await;
    server.send(add_entity(60));
    assert_eq!(client.expect(play_cb::ADD_ENTITY).await, add_entity(60));
    client.peer.send(&empty(play_sb::CLIENT_TICK_END)).await;
    server.expect(play_sb::CLIENT_TICK_END).await;
}

#[tokio::test]
async fn a_server_that_stops_cleanly_keeps_the_client_as_a_crash_does() {
    let (mut rig, mut server) = rig_with(Plan::player(1)).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    server.expect(play_sb::PLAYER_LOADED).await;

    let address = server.stop().await;
    assert_eq!(client.expect(play_cb::SYSTEM_CHAT).await, chat(0));
    rig.expect_event("backend_lost").await;
    let kept_alive = client.collect_until(play_cb::KEEP_ALIVE).await;
    assert!(
        kept_alive
            .iter()
            .all(|frame| id_of(frame) != play_cb::DISCONNECT)
    );

    let mut server = FakeServer::start(address, Plan::player(1)).await;
    assert_eq!(client.come_back().await, login_packet(1));
    rig.expect_event("resumed").await;
    server.expect(play_sb::PLAYER_LOADED).await;
}

#[tokio::test]
async fn three_restarts_in_a_row_each_go_through_a_configuration() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let mut server = server;
    for _ in 0..3 {
        server.expect(play_sb::PLAYER_LOADED).await;
        let address = server.crash().await;
        rig.expect_event("backend_lost").await;
        server = FakeServer::start(address, Plan::player(1)).await;
        assert_eq!(client.come_back().await, login_packet(1));
        rig.expect_event("resumed").await;
    }
}

#[tokio::test]
async fn two_clients_come_back_on_their_own() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    let (mut first, _) = FakeClient::join(rig.proxy, "Bot").await;
    let (mut second, _) = FakeClient::join(rig.proxy, "Other").await;
    assert_eq!(server.logins(), 2);
    let address = server.crash().await;

    first.expect(play_cb::KEEP_ALIVE).await;
    second.expect(play_cb::KEEP_ALIVE).await;
    let server = FakeServer::start(address, Plan::player(1)).await;
    first.come_back().await;
    second.come_back().await;
    for _ in 0..2 {
        rig.expect_event("resumed").await;
    }
    assert_eq!(server.logins(), 2);
    server.send(add_entity(5));
    assert_eq!(first.expect(play_cb::ADD_ENTITY).await, add_entity(5));
    assert_eq!(second.expect(play_cb::ADD_ENTITY).await, add_entity(5));
}

#[tokio::test]
async fn a_client_that_leaves_while_the_server_is_away_is_forgotten() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    let (leaving, _) = FakeClient::join(rig.proxy, "Bot").await;
    let (mut staying, _) = FakeClient::join(rig.proxy, "Other").await;
    let address = server.crash().await;
    rig.expect_event("backend_lost").await;

    drop(leaving);
    rig.expect_event("client_left").await;

    let server = FakeServer::start(address, Plan::player(1)).await;
    staying.come_back().await;
    assert_eq!(rig.expect_event("resumed").await["session"], 2);
    sleep(Duration::from_millis(100)).await;
    assert_eq!(server.logins(), 1);
}

#[tokio::test]
async fn a_server_that_never_returns_ends_with_a_clear_message() {
    let server = FakeServer::start("[::1]:0".parse().unwrap(), Plan::player(1)).await;
    let patience = Duration::from_millis(600);
    let mut settings = settings();
    settings.return_timeout = patience;
    let mut rig = Rig::start_with(server.address, settings).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let started = Instant::now();
    server.crash().await;

    let goodbye = client.expect(play_cb::DISCONNECT).await;
    assert!(started.elapsed() >= patience);
    let text = String::from_utf8_lossy(&goodbye);
    assert!(text.contains("did not come back"), "{text}");
    assert!(client.peer.next().await.is_none());
    rig.expect_event("gave_up").await;
}

#[tokio::test]
async fn a_packet_the_proxy_cannot_read_still_reaches_the_client() {
    let (rig, server) = rig_with(Plan::player(1)).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let broken = frame(play_cb::RESOURCE_PACK_PUSH, |body| body.push(1));
    server.send(broken.clone());
    assert_eq!(client.expect(play_cb::RESOURCE_PACK_PUSH).await, broken);
}

#[tokio::test]
async fn garbage_from_the_server_is_a_lost_server_not_a_crash() {
    let listener = TcpListener::bind("[::1]:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut rig = Rig::start(address).await;
    let accepting = tokio::spawn(async move {
        let mut peer = accept_login(&listener).await;
        peer.next().await;
        peer.write.write_all(&[0xFF; 16]).await.unwrap();
        peer
    });
    let mut client = FakeClient::connect(rig.proxy, 2, PROTOCOL_VERSION).await;
    client
        .peer
        .send(&frame(login_sb::HELLO, |body| {
            "Bot".to_string().encode(body);
            Uuid::nil().encode(body);
        }))
        .await;
    let _stuck = accepting.await.unwrap();
    let server = FakeServer::start(address, Plan::player(1)).await;
    client.expect(login_cb::LOGIN_FINISHED).await;
    assert_eq!(server.logins(), 1);
    let (_healthy, _) = FakeClient::join(rig.proxy, "Other").await;
    rig.expect_event("client_joined").await;
}

#[tokio::test]
async fn a_frame_cut_by_the_crash_never_reaches_the_client() {
    let listener = TcpListener::bind("[::1]:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let rig = Rig::start(address).await;
    let server = tokio::spawn(async move {
        let mut peer = accept_login(&listener).await;
        peer.next().await;
        peer.send(&login_finished()).await;
        peer.next().await;
        peer.next().await;
        peer.send(&frame(config_cb::SELECT_KNOWN_PACKS, |body| body.push(0)))
            .await;
        peer.next().await;
        peer.send(&empty(config_cb::FINISH_CONFIGURATION)).await;
        peer.next().await;
        peer.send(&login_packet(1)).await;
        peer.write.write_all(&[200, 1, 5, 5, 5]).await.unwrap();
    });
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    server.await.unwrap();
    assert_eq!(id_of(&client.next().await), play_cb::KEEP_ALIVE);
}

#[tokio::test]
async fn a_server_lost_after_a_registry_does_not_hand_it_to_the_client_twice() {
    let listener = TcpListener::bind("[::1]:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut rig = Rig::start(address).await;
    let first = tokio::spawn(async move {
        let mut peer = accept_login(&listener).await;
        peer.next().await;
        peer.send(&login_finished()).await;
        peer.next().await;
        peer.next().await;
        peer.send(&frame(config_cb::SELECT_KNOWN_PACKS, |body| body.push(0)))
            .await;
        peer.next().await;
        peer.send(&registry(1)).await;
        sleep(Duration::from_millis(100)).await;
    });
    let proxy = rig.proxy;
    let joining = tokio::spawn(async move { FakeClient::join(proxy, "Bot").await });
    first.await.unwrap();

    let _server = FakeServer::start(address, Plan::player(1)).await;
    let (_client, login) = joining.await.unwrap();
    assert_eq!(login, login_packet(1));
    rig.expect_event("resumed").await;
}

#[tokio::test]
async fn a_compressing_server_stays_invisible_to_the_client() {
    let mut plan = Plan::player(1);
    plan.compression = Some(64);
    plan.after_login = vec![add_entity(50)];
    let (mut rig, mut server) = rig_with(plan.clone()).await;
    let (mut client, login) = FakeClient::join(rig.proxy, "Bot").await;
    assert_eq!(login, login_packet(1));
    client.expect(play_cb::ADD_ENTITY).await;
    server.expect(play_sb::PLAYER_LOADED).await;

    let large = frame(play_cb::SYSTEM_CHAT, |body| {
        body.extend_from_slice(&[9; 5000])
    });
    server.send(large.clone());
    assert_eq!(client.expect(play_cb::SYSTEM_CHAT).await, large);

    let address = server.crash().await;
    let mut server = FakeServer::start(address, plan).await;
    client.come_back().await;
    rig.expect_event("resumed").await;
    server.expect(play_sb::PLAYER_LOADED).await;
    client.peer.send(&vec![0u8; 5000]).await;
    server.expect(0).await;
}

#[tokio::test]
async fn a_change_of_compression_between_two_runs_is_followed() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    let (mut client, _) = FakeClient::join(rig.proxy, "Bot").await;
    let address = server.crash().await;
    let mut plan = Plan::player(1);
    plan.compression = Some(64);
    let server = FakeServer::start(address, plan).await;
    client.come_back().await;
    rig.expect_event("resumed").await;
    let large = frame(play_cb::SYSTEM_CHAT, |body| {
        body.extend_from_slice(&[9; 5000])
    });
    server.send(large.clone());
    assert_eq!(client.expect(play_cb::SYSTEM_CHAT).await, large);
}

#[tokio::test]
async fn the_return_of_a_server_is_announced_without_any_client() {
    let (mut rig, server) = rig_with(Plan::player(1)).await;
    rig.expect_event("backend_up").await;
    let address = server.crash().await;
    rig.expect_event("backend_down").await;
    let _server = FakeServer::start(address, Plan::player(1)).await;
    rig.expect_event("backend_up").await;
}

#[tokio::test]
async fn a_status_request_is_answered_while_the_server_is_away() {
    let (rig, server) = rig_with(Plan::player(1)).await;
    server.crash().await;
    let mut client = FakeClient::connect(rig.proxy, 1, PROTOCOL_VERSION).await;
    client.peer.send(&empty(status_sb::STATUS_REQUEST)).await;
    let status = client.expect(status_cb::STATUS_RESPONSE).await;
    assert!(String::from_utf8_lossy(&status).contains("away"));
    client
        .peer
        .send(&frame(status_sb::PING_REQUEST, |body| 3i64.encode(body)))
        .await;
    client.expect(status_cb::PONG_RESPONSE).await;
}

#[tokio::test]
async fn a_status_request_reaches_a_live_server() {
    let (rig, _server) = rig_with(Plan::player(1)).await;
    let mut client = FakeClient::connect(rig.proxy, 1, PROTOCOL_VERSION).await;
    client.peer.send(&empty(status_sb::STATUS_REQUEST)).await;
    let status = client.expect(status_cb::STATUS_RESPONSE).await;
    assert!(!String::from_utf8_lossy(&status).contains("away"));
}

#[tokio::test]
async fn another_protocol_is_refused_with_a_reason() {
    let (rig, _server) = rig_with(Plan::player(1)).await;
    let mut client = FakeClient::connect(rig.proxy, 2, PROTOCOL_VERSION - 1).await;
    let refusal = client.expect(login_cb::LOGIN_DISCONNECT).await;
    assert!(String::from_utf8_lossy(&refusal).contains("protocol"));
}

#[tokio::test]
async fn a_client_arriving_before_its_server_waits_for_it() {
    let port_holder = TcpListener::bind("[::1]:0").await.unwrap();
    let address = port_holder.local_addr().unwrap();
    drop(port_holder);
    let rig = Rig::start(address).await;
    let joining = tokio::spawn(async move { FakeClient::join(rig.proxy, "Bot").await });
    sleep(Duration::from_millis(100)).await;
    let _server = FakeServer::start(address, Plan::player(1)).await;
    let (_client, login) = joining.await.unwrap();
    assert_eq!(login, login_packet(1));
}
