//! The proxy on real sockets, between a fake client and a fake server, all on ephemeral ports.

mod faults;
mod returns;
mod status;

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
use voidmc_keeper::frame::{self, FrameCodec};
use voidmc_keeper::proxy::{Events, run};
use voidmc_keeper::session::Settings;
use voidmc_protocol::PROTOCOL_VERSION;
use voidmc_protocol::clientbound::{Login, PlayPacket};

const PATIENCE: Duration = Duration::from_secs(10);
/// How long the proxy waits on a write before it gives the peer up.
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);

async fn within<T>(waiting: impl Future<Output = T>) -> T {
    timeout(PATIENCE, waiting)
        .await
        .expect("the test waited too long")
}

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
    frame::id_and_body(frame).expect("every frame has an id").0
}

fn empty(id: i32) -> Bytes {
    frame::build(id, |_| {})
}

fn login_packet(entity_id: i32) -> Bytes {
    let mut frame = Vec::new();
    PlayPacket::Login(Login {
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
    })
    .encode(&mut frame);
    frame.into()
}

fn registry(tag: u8) -> Bytes {
    frame::build(config_cb::REGISTRY_DATA, |body| {
        body.push(tag);
        body.extend_from_slice(&[7; 4000]);
    })
}

fn add_entity(id: i32) -> Bytes {
    frame::build(play_cb::ADD_ENTITY, |body| VarI32(id).encode(body))
}

fn chat(content: u8) -> Bytes {
    frame::build(play_cb::SYSTEM_CHAT, |body| body.push(content))
}

struct Peer {
    read: OwnedReadHalf,
    write: OwnedWriteHalf,
    inbound: BytesMut,
    codec: FrameCodec,
}

impl Peer {
    fn new(stream: TcpStream) -> Self {
        let (read, write) = stream.into_split();
        Self {
            read,
            write,
            inbound: BytesMut::new(),
            codec: FrameCodec::default(),
        }
    }

    async fn next(&mut self) -> Option<Bytes> {
        loop {
            if let Some(frame) = self.codec.decode(&mut self.inbound).ok()? {
                return Some(frame);
            }
            if self.read.read_buf(&mut self.inbound).await.ok()? == 0 {
                return None;
            }
        }
    }

    async fn send(&mut self, frame: &[u8]) {
        let mut out = Vec::new();
        self.codec.encode(frame, &mut out);
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
    probes: Arc<AtomicUsize>,
    received: mpsc::UnboundedReceiver<Bytes>,
    broadcast: broadcast::Sender<Bytes>,
    stop: watch::Sender<bool>,
}

impl FakeServer {
    async fn start(address: SocketAddr, plan: Plan) -> Self {
        let listener = TcpListener::bind(address).await.unwrap();
        let address = listener.local_addr().unwrap();
        let logins = Arc::new(AtomicUsize::new(0));
        let probes = Arc::new(AtomicUsize::new(0));
        let (received_tx, received) = mpsc::unbounded_channel();
        let (broadcast, _) = broadcast::channel(64);
        let (stop, stopping) = watch::channel(false);
        let task = tokio::spawn({
            let logins = logins.clone();
            let probes = probes.clone();
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
                                (logins.clone(), probes.clone()),
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
            probes,
            received,
            broadcast,
            stop,
        }
    }

    fn logins(&self) -> usize {
        self.logins.load(Ordering::SeqCst)
    }

    fn probes(&self) -> usize {
        self.probes.load(Ordering::SeqCst)
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
        within(self.task).await.unwrap();
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
    (logins, probes): (Arc<AtomicUsize>, Arc<AtomicUsize>),
    received: mpsc::UnboundedSender<Bytes>,
    (mut broadcast, mut stopping): (broadcast::Receiver<Bytes>, watch::Receiver<bool>),
) {
    let Some(handshake) = peer.next().await else {
        return;
    };
    let is_status = handshake.last() == Some(&1);
    if is_status {
        probes.fetch_add(1, Ordering::SeqCst);
        while let Some(frame_in) = peer.next().await {
            if id_of(&frame_in) == status_sb::STATUS_REQUEST {
                peer.send(&frame::build(status_cb::STATUS_RESPONSE, |body| {
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
        peer.send(&frame::build(login_cb::LOGIN_COMPRESSION, |body| {
            VarI32(threshold).encode(body);
        }))
        .await;
        peer.codec.set_threshold(threshold);
    }
    peer.send(&login_finished()).await;
    while let Some(frame_in) = peer.next().await {
        if id_of(&frame_in) == login_sb::LOGIN_ACKNOWLEDGED {
            break;
        }
    }
    peer.send(&frame::build(config_cb::SELECT_KNOWN_PACKS, |body| {
        body.push(0)
    }))
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
        peer.send(&frame::build(0, |body| {
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
            .send(&frame::build(login_sb::HELLO, |body| {
                name.to_string().encode(body);
                Uuid::from_u128(name.len() as u128).encode(body);
            }))
            .await;
        client.expect(login_cb::LOGIN_FINISHED).await;
        client.peer.send(&empty(login_sb::LOGIN_ACKNOWLEDGED)).await;
        client
            .peer
            .send(&frame::build(config_sb::CLIENT_INFORMATION, |body| {
                body.push(1)
            }))
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
                        .send(&frame::build(config_sb::SELECT_KNOWN_PACKS, |body| {
                            body.push(0)
                        }))
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
        self.expect_event_within(name, PATIENCE).await
    }

    async fn expect_event_within(&mut self, name: &str, limit: Duration) -> Value {
        timeout(limit, async {
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

fn login_finished() -> Bytes {
    frame::build(login_cb::LOGIN_FINISHED, |body| {
        Uuid::from_u128(77).encode(body);
    })
}

/// The next connection that logs in, past the status probes of the proxy.
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

/// The server side of a join, by hand, up to the `login`.
async fn log_in_by_hand(peer: &mut Peer) {
    peer.next().await;
    peer.send(&login_finished()).await;
    peer.next().await;
    peer.next().await;
    peer.send(&frame::build(config_cb::SELECT_KNOWN_PACKS, |body| {
        body.push(0)
    }))
    .await;
    peer.next().await;
    peer.send(&empty(config_cb::FINISH_CONFIGURATION)).await;
    peer.next().await;
    peer.send(&login_packet(1)).await;
}

async fn rig_with(plan: Plan) -> (Rig, FakeServer) {
    let server = FakeServer::start("[::1]:0".parse().unwrap(), plan).await;
    let rig = Rig::start(server.address).await;
    (rig, server)
}
