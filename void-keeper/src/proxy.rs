//! Runs one [`Session`] per client on real sockets, and says what happens as JSON lines.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::{Bytes, BytesMut};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{MissedTickBehavior, interval, sleep, timeout};
use voidmc_codec::{Decode, Encode, VarI32};
use voidmc_data::v26_1_2::packets::login::clientbound as login_cb;
use voidmc_data::v26_1_2::packets::status::clientbound as status_cb;
use voidmc_data::v26_1_2::packets::status::serverbound as status_sb;
use voidmc_protocol::serverbound::Handshake;
use voidmc_protocol::{MINECRAFT_VERSION, PROTOCOL_VERSION, State};

use crate::ledger::Tally;
use crate::session::{Action, Event, Session, Settings};
use crate::wire::{Framing, WireError, frame, split};

const TICK: Duration = Duration::from_millis(50);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_EVERY: Duration = Duration::from_millis(500);
const READ_CHUNK: usize = 64 * 1024;

/// Where events go: the lines `dev-tests` and the dev tool read.
#[derive(Clone)]
pub struct Events {
    sink: Arc<dyn Fn(Value) + Send + Sync>,
}

impl Events {
    pub fn new(sink: impl Fn(Value) + Send + Sync + 'static) -> Self {
        Self {
            sink: Arc::new(sink),
        }
    }

    pub fn stdout() -> Self {
        Self::new(|event| println!("{event}"))
    }

    fn emit(&self, name: &str, mut fields: Value) {
        let at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        fields["event"] = json!(name);
        fields["at_ms"] = json!(at_ms);
        (self.sink)(fields);
    }
}

#[derive(Debug)]
enum PeerError {
    Io(io::Error),
    Wire(WireError),
}

impl std::fmt::Display for PeerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Wire(error) => error.fmt(f),
        }
    }
}

struct Peer {
    stream: TcpStream,
    framing: Framing,
    inbound: BytesMut,
    outbound: Vec<u8>,
}

impl Peer {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            framing: Framing::default(),
            inbound: BytesMut::with_capacity(READ_CHUNK),
            outbound: Vec::new(),
        }
    }

    fn buffered(&mut self) -> Result<Option<Bytes>, WireError> {
        self.framing.decode(&mut self.inbound)
    }

    /// `None` when the peer closed. Safe to cancel: bytes only ever land in `inbound`.
    async fn next(&mut self) -> Result<Option<Bytes>, PeerError> {
        loop {
            if let Some(frame) = self.buffered().map_err(PeerError::Wire)? {
                return Ok(Some(frame));
            }
            self.inbound.reserve(READ_CHUNK);
            let read = self
                .stream
                .read_buf(&mut self.inbound)
                .await
                .map_err(PeerError::Io)?;
            if read == 0 {
                return Ok(None);
            }
        }
    }

    fn queue(&mut self, frame: &[u8]) {
        self.framing.encode(frame, &mut self.outbound);
    }

    async fn flush(&mut self) -> io::Result<()> {
        if self.outbound.is_empty() {
            return Ok(());
        }
        self.stream.write_all(&self.outbound).await?;
        self.outbound.clear();
        Ok(())
    }
}

async fn next_of(peer: &mut Option<Peer>) -> Result<Option<Bytes>, PeerError> {
    match peer {
        Some(peer) => peer.next().await,
        None => std::future::pending().await,
    }
}

async fn connect(address: SocketAddr) -> Option<TcpStream> {
    let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
        .await
        .ok()?
        .ok()?;
    stream.set_nodelay(true).ok()?;
    Some(stream)
}

struct Keeper {
    id: u64,
    backend_address: SocketAddr,
    session: Session,
    client: Peer,
    backend: Option<Peer>,
    events: Events,
    wants_backend: bool,
    closing: bool,
}

impl Keeper {
    fn emit(&self, event: Event) {
        let bytes = self.session.ledger().heap_bytes();
        let tallied = |mut fields: Value, tally: Tally| {
            fields["session"] = json!(self.id);
            fields["ledger_bytes"] = json!(bytes);
            fields["entities"] = json!(tally.entities);
            fields["chunks"] = json!(tally.chunks);
            fields["boss_bars"] = json!(tally.boss_bars);
            fields["objectives"] = json!(tally.objectives);
            fields["teams"] = json!(tally.teams);
            fields["tab_entries"] = json!(tally.tab_entries);
            fields["effects"] = json!(tally.effects);
            fields
        };
        match event {
            Event::Joined { name } => self
                .events
                .emit("client_joined", json!({ "session": self.id, "name": name })),
            Event::BackendLost { tally } => {
                self.events.emit("backend_lost", tallied(json!({}), tally));
            }
            Event::Resumed {
                path,
                reason,
                outage,
                tally,
            } => self.events.emit(
                "resumed",
                tallied(
                    json!({
                        "path": path.to_string(),
                        "reason": reason,
                        "outage_ms": outage.as_millis() as u64,
                    }),
                    tally,
                ),
            ),
            Event::GaveUp { reason } => self
                .events
                .emit("gave_up", json!({ "session": self.id, "reason": reason })),
        }
    }

    /// Takes in what the session decided. Queued frames stay queued: a burst leaves as one write.
    fn absorb(&mut self) {
        for action in self.session.take_actions() {
            match action {
                Action::ToClient(frame) => self.client.queue(&frame),
                Action::ToBackend(frame) => {
                    if let Some(backend) = &mut self.backend {
                        backend.queue(&frame);
                    }
                }
                Action::Compress(threshold) => {
                    if let Some(backend) = &mut self.backend {
                        backend.framing.set_threshold(threshold);
                    }
                }
                Action::Connect => self.wants_backend = true,
                Action::DropBackend => self.backend = None,
                Action::Close => self.closing = true,
                Action::Event(event) => self.emit(event),
            }
        }
    }

    fn lose_backend(&mut self, why: &dyn std::fmt::Display) {
        tracing::info!(session = self.id, %why, "backend gone");
        self.backend = None;
        self.session.on_backend_lost(Instant::now());
        self.absorb();
    }

    /// Handles a frame and every other frame already read, then leaves the rest to `settle`.
    fn receive_from_backend(&mut self, first: Bytes) {
        let mut next = Some(first);
        while let Some(frame) = next.take() {
            self.session.on_backend(frame, Instant::now());
            self.absorb();
            let Some(backend) = &mut self.backend else {
                return;
            };
            match backend.buffered() {
                Ok(following) => next = following,
                Err(error) => self.lose_backend(&error),
            }
        }
    }

    fn receive_from_client(&mut self, first: Bytes) -> Result<(), WireError> {
        let mut next = Some(first);
        while let Some(frame) = next.take() {
            self.session.on_client(frame, Instant::now());
            self.absorb();
            next = self.client.buffered()?;
        }
        Ok(())
    }

    /// Writes what is queued and opens a backend if one is wanted. `false` once the client
    /// is gone, or was sent away.
    async fn settle(&mut self) -> bool {
        if self.client.flush().await.is_err() {
            return false;
        }
        if self.closing {
            return false;
        }
        if let Some(backend) = &mut self.backend
            && let Err(error) = backend.flush().await
        {
            self.lose_backend(&error);
            return self.client.flush().await.is_ok();
        }
        if std::mem::take(&mut self.wants_backend) {
            match connect(self.backend_address).await {
                Some(stream) => {
                    self.backend = Some(Peer::new(stream));
                    self.session.on_backend_connected();
                }
                None => self.session.on_connect_failed(Instant::now()),
            }
            self.absorb();
            if let Some(backend) = &mut self.backend
                && let Err(error) = backend.flush().await
            {
                self.lose_backend(&error);
            }
            return self.client.flush().await.is_ok();
        }
        true
    }

    async fn run(mut self) {
        let mut tick = interval(TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            if !self.settle().await {
                break;
            }
            tokio::select! {
                read = self.client.next() => match read {
                    Ok(Some(frame)) => {
                        if let Err(error) = self.receive_from_client(frame) {
                            tracing::warn!(session = self.id, %error, "client sent garbage");
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::info!(session = self.id, %error, "client gone");
                        break;
                    }
                },
                read = next_of(&mut self.backend) => match read {
                    Ok(Some(frame)) => self.receive_from_backend(frame),
                    Ok(None) => self.lose_backend(&"closed"),
                    Err(error) => self.lose_backend(&error),
                },
                _ = tick.tick() => {
                    self.session.on_tick(Instant::now());
                    self.absorb();
                }
            }
        }
        let _ = self.client.stream.shutdown().await;
        self.events
            .emit("client_left", json!({ "session": self.id }));
    }
}

async fn refuse_login(mut client: Peer, message: &str) {
    client.queue(&frame(login_cb::LOGIN_DISCONNECT, |body| {
        let json = json!({ "text": format!("[keeper] {message}") });
        json.to_string().encode(body);
    }));
    let _ = client.flush().await;
    let _ = client.stream.shutdown().await;
}

fn status_response(body: &mut Vec<u8>) {
    let status = json!({
        "version": { "name": MINECRAFT_VERSION, "protocol": PROTOCOL_VERSION },
        "players": { "max": 0, "online": 0 },
        "description": { "text": "The server is away, the keeper is waiting for it" },
    });
    status.to_string().encode(body);
}

/// The server answers its own pings; while it is away, the keeper does.
async fn relay_status(mut client: Peer, handshake: Bytes, backend_address: SocketAddr) {
    if let Some(stream) = connect(backend_address).await {
        let mut backend = Peer::new(stream);
        backend.queue(&handshake);
        backend.outbound.extend_from_slice(&client.inbound);
        if backend.flush().await.is_ok() {
            let _ = copy_bidirectional(&mut client.stream, &mut backend.stream).await;
        }
        return;
    }
    while let Ok(Some(frame_in)) = client.next().await {
        match split(&frame_in) {
            Some((status_sb::STATUS_REQUEST, _)) => {
                client.queue(&frame(status_cb::STATUS_RESPONSE, status_response));
            }
            Some((status_sb::PING_REQUEST, body)) => {
                client.queue(&frame(status_cb::PONG_RESPONSE, |out| {
                    out.extend_from_slice(body);
                }));
            }
            _ => return,
        }
        if client.flush().await.is_err() {
            return;
        }
    }
}

async fn serve(
    stream: TcpStream,
    id: u64,
    backend_address: SocketAddr,
    settings: Settings,
    events: Events,
) {
    let _ = stream.set_nodelay(true);
    let mut client = Peer::new(stream);
    let Ok(Ok(Some(handshake))) = timeout(HANDSHAKE_TIMEOUT, client.next()).await else {
        return;
    };
    let Some((_, mut body)) = split(&handshake) else {
        return;
    };
    let Ok(intent) = Handshake::decode(&mut body) else {
        tracing::warn!(session = id, "unreadable handshake");
        return;
    };
    match intent.next_state {
        State::Status => relay_status(client, handshake, backend_address).await,
        _ if intent.protocol_version != PROTOCOL_VERSION => {
            let message = format!(
                "This keeper speaks protocol {PROTOCOL_VERSION} (Minecraft {MINECRAFT_VERSION}), \
                 the client speaks {}.",
                intent.protocol_version
            );
            refuse_login(client, &message).await;
        }
        _ => {
            let session = Session::new(settings, Instant::now(), handshake);
            let mut keeper = Keeper {
                id,
                backend_address,
                session,
                client,
                backend: None,
                events,
                wants_backend: false,
                closing: false,
            };
            keeper.session.on_tick(Instant::now());
            keeper.absorb();
            keeper.run().await;
        }
    }
}

/// Answers whether a server is up: a status request, which logs nobody in.
pub async fn probe(address: SocketAddr) -> bool {
    let attempt = async {
        let mut peer = Peer::new(connect(address).await?);
        let port = address.port();
        peer.queue(&frame(0, |body| {
            VarI32(PROTOCOL_VERSION).encode(body);
            address.ip().to_string().encode(body);
            port.encode(body);
            VarI32(1).encode(body);
        }));
        peer.queue(&frame(status_sb::STATUS_REQUEST, |_| {}));
        peer.flush().await.ok()?;
        peer.next().await.ok()?
    };
    timeout(CONNECT_TIMEOUT, attempt)
        .await
        .is_ok_and(|answer| answer.is_some())
}

async fn watch_backend(address: SocketAddr, events: Events) {
    let mut up = None;
    loop {
        let now_up = probe(address).await;
        if up != Some(now_up) {
            up = Some(now_up);
            let name = if now_up { "backend_up" } else { "backend_down" };
            events.emit(name, json!({ "backend": address.to_string() }));
        }
        sleep(PROBE_EVERY).await;
    }
}

/// Serves clients on `listener` until the process ends.
pub async fn run(
    listener: TcpListener,
    backend_address: SocketAddr,
    settings: Settings,
    events: Events,
) -> io::Result<()> {
    events.emit(
        "listening",
        json!({
            "address": listener.local_addr()?.to_string(),
            "backend": backend_address.to_string(),
        }),
    );
    tokio::spawn(watch_backend(backend_address, events.clone()));
    let sessions = AtomicU64::new(1);
    loop {
        let (stream, _) = listener.accept().await?;
        let id = sessions.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(serve(stream, id, backend_address, settings, events.clone()));
    }
}
