//! Runs one [`Session`] per client on real sockets, and says what happens as JSON lines.

use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, channel, sync_channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::{Bytes, BytesMut};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{MissedTickBehavior, interval, sleep, timeout, timeout_at};
use voidmc_codec::{Decode, Encode, VarI32};
use voidmc_data::v26_1_2::packets::login::clientbound as login_cb;
use voidmc_data::v26_1_2::packets::status::clientbound as status_cb;
use voidmc_data::v26_1_2::packets::status::serverbound as status_sb;
use voidmc_protocol::serverbound::Handshake;
use voidmc_protocol::{MINECRAFT_VERSION, PROTOCOL_VERSION, State};

use crate::frame::{Frame, FrameCodec, FrameError};
use crate::session::{Action, Event, Session, Settings};

const TICK: Duration = Duration::from_millis(50);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_EVERY: Duration = Duration::from_millis(500);
const READ_CHUNK: usize = 64 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
const STATUS_TIMEOUT: Duration = Duration::from_secs(10);
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);
const EVENT_BACKLOG: usize = 1024;

enum Line {
    Event(Value),
    Drained(std::sync::mpsc::Sender<()>),
}

#[derive(Clone)]
struct Writer {
    lines: SyncSender<Line>,
    lost: Arc<AtomicU64>,
}

impl Writer {
    fn lose(&self) {
        let lost = self.lost.fetch_add(1, Ordering::Relaxed) + 1;
        if lost.is_power_of_two() {
            tracing::warn!(lost, "events lost: their reader is gone or too slow");
        }
    }
}

/// Where events go, one JSON object each.
#[derive(Clone)]
pub struct Events {
    sink: Arc<dyn Fn(Value) + Send + Sync>,
    writer: Option<Writer>,
}

impl Events {
    pub fn new(sink: impl Fn(Value) + Send + Sync + 'static) -> Self {
        Self {
            sink: Arc::new(sink),
            writer: None,
        }
    }

    pub fn stdout() -> Self {
        Self::writing_to(io::stdout())
    }

    /// Writes on a thread of its own: a reader that stalls or dies costs events, never a session.
    pub fn writing_to(mut out: impl Write + Send + 'static) -> Self {
        let (lines, receiver) = sync_channel::<Line>(EVENT_BACKLOG);
        let writer = Writer {
            lines,
            lost: Arc::default(),
        };
        let thread_writer = writer.clone();
        std::thread::spawn(move || {
            let mut broken = false;
            for line in receiver {
                match line {
                    Line::Event(event) => {
                        broken =
                            broken || writeln!(out, "{event}").and_then(|()| out.flush()).is_err();
                        if broken {
                            thread_writer.lose();
                        }
                    }
                    Line::Drained(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        let sink_writer = writer.clone();
        Self {
            sink: Arc::new(move |event| {
                if sink_writer.lines.try_send(Line::Event(event)).is_err() {
                    sink_writer.lose();
                }
            }),
            writer: Some(writer),
        }
    }

    /// Events that never reached their reader.
    pub fn lost(&self) -> u64 {
        self.writer
            .as_ref()
            .map_or(0, |writer| writer.lost.load(Ordering::Relaxed))
    }

    /// Waits until every event emitted so far is written, for at most `limit`. `false` when the
    /// reader did not take them in time.
    pub fn drain(&self, limit: Duration) -> bool {
        let Some(writer) = &self.writer else {
            return true;
        };
        let deadline = Instant::now() + limit;
        let (done, drained) = channel();
        let mut line = Line::Drained(done);
        let written = loop {
            match writer.lines.try_send(line) {
                Ok(()) => {
                    break drained
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .is_ok();
                }
                Err(TrySendError::Full(back)) if Instant::now() < deadline => {
                    line = back;
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break false,
            }
        };
        let lost = self.lost();
        if lost > 0 {
            tracing::warn!(lost, "events lost in all");
        }
        written
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
    Frame(FrameError),
}

impl std::fmt::Display for PeerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Frame(error) => error.fmt(f),
        }
    }
}

struct Peer<S = TcpStream> {
    stream: S,
    codec: FrameCodec,
    inbound: BytesMut,
    outbound: Vec<u8>,
    stalled_since: Option<tokio::time::Instant>,
}

impl<S> Peer<S> {
    fn new(stream: S) -> Self {
        Self {
            stream,
            codec: FrameCodec::default(),
            inbound: BytesMut::with_capacity(READ_CHUNK),
            outbound: Vec::new(),
            stalled_since: None,
        }
    }

    fn buffered(&mut self) -> Result<Option<Bytes>, FrameError> {
        self.codec.decode(&mut self.inbound)
    }

    fn queue(&mut self, frame: &[u8]) {
        self.codec.encode(frame, &mut self.outbound);
    }
}

impl Peer {
    /// `None` when the peer closed. Safe to cancel: bytes only ever land in `inbound`.
    async fn next(&mut self) -> Result<Option<Bytes>, PeerError> {
        loop {
            if let Some(frame) = self.buffered().map_err(PeerError::Frame)? {
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

    async fn flush(&mut self) -> io::Result<()> {
        self.flush_within(WRITE_TIMEOUT).await
    }

    /// Fails once the peer's socket has stayed full for `limit`, counted from the first write
    /// that had to wait: a peer that takes a few bytes now and then is still stuck. Only a
    /// queue written whole without waiting ends the stall.
    async fn flush_within(&mut self, limit: Duration) -> io::Result<()> {
        let mut written = 0;
        while written < self.outbound.len() {
            match self.stream.try_write(&self.outbound[written..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => written += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let since = *self
                        .stalled_since
                        .get_or_insert_with(tokio::time::Instant::now);
                    timeout_at(
                        since + limit,
                        self.stream.write_all(&self.outbound[written..]),
                    )
                    .await
                    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
                    self.outbound.clear();
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
        if written > 0 {
            self.outbound.clear();
            self.stalled_since = None;
        }
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

/// What the keeper does with the bytes it reads, sockets aside: frames in, the session,
/// frames queued out. The proxy and the relay bench both go through it.
pub struct Relay<S> {
    id: u64,
    session: Session,
    client: Peer<S>,
    backend: Option<Peer<S>>,
    events: Events,
    wants_backend: bool,
    closing: bool,
}

impl<S> Relay<S> {
    /// A session whose client is `client`, given its first tick.
    pub fn new(id: u64, session: Session, client: S, events: Events) -> Self {
        Self::with_client(id, session, Peer::new(client), events)
    }

    fn with_client(id: u64, session: Session, client: Peer<S>, events: Events) -> Self {
        let mut relay = Self {
            id,
            session,
            client,
            backend: None,
            events,
            wants_backend: false,
            closing: false,
        };
        relay.session.on_tick(Instant::now());
        relay.absorb();
        relay
    }

    pub fn backend_connected(&mut self, stream: S) {
        self.backend = Some(Peer::new(stream));
        self.session.on_backend_connected(Instant::now());
        self.absorb();
    }

    /// Takes in `bytes` read from the backend, and every frame they complete.
    pub fn received_from_backend(&mut self, bytes: &[u8]) {
        let Some(backend) = &mut self.backend else {
            return;
        };
        backend.inbound.extend_from_slice(bytes);
        match backend.buffered() {
            Ok(Some(first)) => self.receive_from_backend(first),
            Ok(None) => {}
            Err(error) => self.lose_backend(&error),
        }
    }

    /// Takes in `bytes` read from the client, and every frame they complete.
    pub fn received_from_client(&mut self, bytes: &[u8]) -> Result<(), FrameError> {
        self.client.inbound.extend_from_slice(bytes);
        match self.client.buffered()? {
            Some(first) => self.receive_from_client(first),
            None => Ok(()),
        }
    }

    /// Forgets what is queued for both sides, as if it was written. The bytes it forgot.
    pub fn discard_queued(&mut self) -> usize {
        let mut queued = self.client.outbound.len();
        self.client.outbound.clear();
        if let Some(backend) = &mut self.backend {
            queued += backend.outbound.len();
            backend.outbound.clear();
        }
        queued
    }

    /// Takes in what the session decided. Queued frames stay queued: a burst leaves as one write.
    fn absorb(&mut self) {
        for action in self.session.actions() {
            match action {
                Action::ToClient(frame) => self.client.queue(&frame),
                Action::ToBackend(frame) => {
                    if let Some(backend) = &mut self.backend {
                        backend.queue(&frame);
                    }
                }
                Action::Compress(threshold) => {
                    if let Some(backend) = &mut self.backend {
                        backend.codec.set_threshold(threshold);
                    }
                }
                Action::Connect => self.wants_backend = true,
                Action::DropBackend => self.backend = None,
                Action::Close => self.closing = true,
                Action::Event(event) => emit(&self.events, self.id, event),
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
    /// The frames of one read arrived at once, so they share one reading of the clock.
    fn receive_from_backend(&mut self, first: Bytes) {
        let now = Instant::now();
        let mut next = Some(first);
        while let Some(frame) = next.take() {
            self.session.on_backend(frame, now);
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

    fn receive_from_client(&mut self, first: Bytes) -> Result<(), FrameError> {
        let now = Instant::now();
        let mut next = Some(first);
        while let Some(frame) = next.take() {
            self.session.on_client(frame, now);
            self.absorb();
            next = self.client.buffered()?;
        }
        Ok(())
    }

    fn tick(&mut self) {
        self.session.on_tick(Instant::now());
        self.absorb();
    }
}

fn emit(events: &Events, id: u64, event: Event) {
    match event {
        Event::Joined { name } => {
            events.emit("client_joined", json!({ "session": id, "name": name }));
        }
        Event::BackendLost => events.emit("backend_lost", json!({ "session": id })),
        Event::Resumed { outage } => events.emit(
            "resumed",
            json!({ "session": id, "outage_ms": outage.as_millis() as u64 }),
        ),
        Event::GaveUp { reason } => {
            events.emit("gave_up", json!({ "session": id, "reason": reason }));
        }
    }
}

struct Keeper {
    relay: Relay<TcpStream>,
    backend_address: SocketAddr,
}

impl Keeper {
    /// Writes what is queued and opens a backend if one is wanted. `false` once the client
    /// is gone, or was sent away.
    async fn settle(&mut self) -> bool {
        let relay = &mut self.relay;
        if relay.client.flush().await.is_err() {
            return false;
        }
        if relay.closing {
            return false;
        }
        if let Some(backend) = &mut relay.backend
            && let Err(error) = backend.flush().await
        {
            relay.lose_backend(&error);
            return relay.client.flush().await.is_ok();
        }
        if std::mem::take(&mut relay.wants_backend) {
            match connect(self.backend_address).await {
                Some(stream) => relay.backend_connected(stream),
                None => {
                    relay.session.on_connect_failed(Instant::now());
                    relay.absorb();
                }
            }
            if let Some(backend) = &mut relay.backend
                && let Err(error) = backend.flush().await
            {
                relay.lose_backend(&error);
            }
            return relay.client.flush().await.is_ok();
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
            let relay = &mut self.relay;
            tokio::select! {
                read = relay.client.next() => match read {
                    Ok(Some(frame)) => {
                        if let Err(error) = relay.receive_from_client(frame) {
                            tracing::warn!(session = relay.id, %error, "client sent garbage");
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::info!(session = relay.id, %error, "client gone");
                        break;
                    }
                },
                read = next_of(&mut relay.backend) => match read {
                    Ok(Some(frame)) => relay.receive_from_backend(frame),
                    Ok(None) => relay.lose_backend(&"closed"),
                    Err(error) => relay.lose_backend(&error),
                },
                _ = tick.tick() => relay.tick(),
            }
        }
        let mut relay = self.relay;
        let _ = relay.client.stream.shutdown().await;
        relay
            .events
            .emit("client_left", json!({ "session": relay.id }));
    }
}

async fn refuse_login(mut client: Peer, message: &str) {
    client.queue(&Frame::encode(login_cb::LOGIN_DISCONNECT, |body| {
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

async fn relay_for(limit: Duration, client: &mut TcpStream, backend: &mut TcpStream) {
    let _ = timeout(limit, copy_bidirectional(client, backend)).await;
}

/// The server answers its own pings; while it is away, the keeper does.
async fn relay_status(mut client: Peer, handshake: Bytes, backend_address: SocketAddr) {
    if let Some(stream) = connect(backend_address).await {
        let mut backend = Peer::new(stream);
        backend.queue(&handshake);
        backend.outbound.extend_from_slice(&client.inbound);
        if backend.flush().await.is_ok() {
            relay_for(STATUS_TIMEOUT, &mut client.stream, &mut backend.stream).await;
        }
        return;
    }
    while let Ok(Ok(Some(frame_in))) = timeout(STATUS_TIMEOUT, client.next()).await {
        match Frame::id_and_body(&frame_in) {
            Some((status_sb::STATUS_REQUEST, _)) => {
                client.queue(&Frame::encode(status_cb::STATUS_RESPONSE, status_response));
            }
            Some((status_sb::PING_REQUEST, body)) => {
                client.queue(&Frame::encode(status_cb::PONG_RESPONSE, |out| {
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

/// Counts a session among the open ones for as long as it runs.
struct Open(Arc<watch::Sender<usize>>);

impl Open {
    fn new(open: &Arc<watch::Sender<usize>>) -> Self {
        open.send_modify(|count| *count += 1);
        Self(open.clone())
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}

async fn serve(
    stream: TcpStream,
    id: u64,
    backend_address: SocketAddr,
    settings: Settings,
    events: Events,
    open: Arc<watch::Sender<usize>>,
) {
    let _ = stream.set_nodelay(true);
    let mut client = Peer::new(stream);
    let Ok(Ok(Some(handshake))) = timeout(HANDSHAKE_TIMEOUT, client.next()).await else {
        return;
    };
    let Some((_, mut body)) = Frame::id_and_body(&handshake) else {
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
            let _open = Open::new(&open);
            let session = Session::new(settings, Instant::now(), handshake);
            let keeper = Keeper {
                relay: Relay::with_client(id, session, client, events),
                backend_address,
            };
            keeper.run().await;
        }
    }
}

/// Answers whether a server is up: a status request, which logs nobody in.
pub async fn probe(address: SocketAddr) -> bool {
    let attempt = async {
        let mut peer = Peer::new(connect(address).await?);
        let port = address.port();
        peer.queue(&Frame::encode(0, |body| {
            VarI32(PROTOCOL_VERSION).encode(body);
            address.ip().to_string().encode(body);
            port.encode(body);
            VarI32(1).encode(body);
        }));
        peer.queue(&Frame::encode(status_sb::STATUS_REQUEST, |_| {}));
        peer.flush().await.ok()?;
        peer.next().await.ok()?
    };
    timeout(CONNECT_TIMEOUT, attempt)
        .await
        .is_ok_and(|answer| answer.is_some())
}

/// Probes the server while no session is open: a session says itself when it is lost.
async fn watch_backend(address: SocketAddr, events: Events, mut open: watch::Receiver<usize>) {
    let mut up = None;
    loop {
        if open.wait_for(|count| *count == 0).await.is_err() {
            return;
        }
        let now_up = probe(address).await;
        if up != Some(now_up) {
            up = Some(now_up);
            let name = if now_up { "backend_up" } else { "backend_down" };
            events.emit(name, json!({ "backend": address.to_string() }));
        }
        sleep(PROBE_EVERY).await;
    }
}

/// A failed `accept`, such as out of file descriptors, says nothing of the next one.
async fn next_connection<T, F>(mut accept: impl FnMut() -> F) -> T
where
    F: Future<Output = io::Result<T>>,
{
    loop {
        match accept().await {
            Ok(connection) => return connection,
            Err(error) => {
                tracing::warn!(%error, "accept failed, trying again");
                sleep(ACCEPT_BACKOFF).await;
            }
        }
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
    let open = Arc::new(watch::Sender::new(0));
    tokio::spawn(watch_backend(
        backend_address,
        events.clone(),
        open.subscribe(),
    ));
    let sessions = AtomicU64::new(1);
    loop {
        let (stream, _) = next_connection(|| listener.accept()).await;
        let id = sessions.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(serve(
            stream,
            id,
            backend_address,
            settings,
            events.clone(),
            open.clone(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct Stuck;

    impl Write for Stuck {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            loop {
                std::thread::park();
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    async fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("[::1]:0").await.unwrap();
        let near = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (far, _) = listener.accept().await.unwrap();
        (near, far)
    }

    #[derive(Clone, Default)]
    struct Kept(Arc<std::sync::Mutex<Vec<u8>>>);

    impl Write for Kept {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            std::thread::sleep(Duration::from_micros(200));
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_dead_reader_of_events_costs_them_all_and_they_are_counted() {
        let events = Events::writing_to(Broken);
        for _ in 0..3 {
            events.emit("listening", json!({}));
        }
        assert!(events.drain(Duration::from_secs(5)));
        assert_eq!(events.lost(), 3);
    }

    #[test]
    fn a_stuck_reader_of_events_blocks_nobody() {
        let events = Events::writing_to(Stuck);
        for _ in 0..EVENT_BACKLOG * 3 {
            events.emit("listening", json!({}));
        }
        assert!(events.lost() >= 2 * EVENT_BACKLOG as u64 - 1);
        assert!(!events.drain(Duration::from_millis(50)));
    }

    #[test]
    fn events_are_all_written_once_drained() {
        let kept = Kept::default();
        let events = Events::writing_to(kept.clone());
        for _ in 0..200 {
            events.emit("listening", json!({}));
        }
        assert!(events.drain(Duration::from_secs(5)));
        let written = kept
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|&&b| b == b'\n')
            .count();
        assert_eq!(written, 200);
        assert_eq!(events.lost(), 0);
    }

    #[tokio::test]
    async fn a_peer_that_stops_reading_fails_the_flush() {
        let (near, _far) = pair().await;
        let mut peer = Peer::new(near);
        peer.outbound = vec![0; 64 * 1024 * 1024];
        let error = peer
            .flush_within(Duration::from_millis(100))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn a_peer_that_swallows_a_little_now_and_then_is_lost_from_its_first_stall() {
        let (near, mut far) = pair().await;
        let swallowing = tokio::spawn(async move {
            let mut buffer = vec![0; 64 * 1024];
            loop {
                sleep(Duration::from_millis(20)).await;
                if far.read(&mut buffer).await.map_or(true, |read| read == 0) {
                    break;
                }
            }
        });
        let mut peer = Peer::new(near);
        let limit = Duration::from_secs(1);
        let started = tokio::time::Instant::now();
        let error = loop {
            peer.outbound.resize(1024 * 1024, 0);
            if let Err(error) = peer.flush_within(limit).await {
                break error;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "every flush made a little progress, and the peer was never lost"
            );
        };
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        let stalled_for = peer.stalled_since.expect("the peer stalled").elapsed();
        assert!(stalled_for >= limit && stalled_for < limit * 3);
        swallowing.abort();
    }

    #[tokio::test]
    async fn a_status_relay_ends_on_its_own() {
        let (mut client, _held) = pair().await;
        let (mut backend, _also_held) = pair().await;
        let relay = relay_for(Duration::from_millis(50), &mut client, &mut backend);
        timeout(Duration::from_secs(5), relay).await.unwrap();
    }

    #[tokio::test]
    async fn a_failed_accept_is_tried_again() {
        let mut failures = 2;
        let connection = next_connection(|| {
            let failed = failures > 0;
            failures -= i32::from(failed);
            async move {
                if failed {
                    Err(io::Error::from_raw_os_error(24))
                } else {
                    Ok(7)
                }
            }
        })
        .await;
        assert_eq!(connection, 7);
    }
}
