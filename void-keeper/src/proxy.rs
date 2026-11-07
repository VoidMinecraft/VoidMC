//! Runs one [`Session`] per client on real sockets, and says what happens as JSON lines.

mod events;
mod peer;
mod status;

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{MissedTickBehavior, interval, sleep, timeout};
use voidmc_codec::{Decode, Encode};
use voidmc_data::v26_1_2::packets::login::clientbound as login_cb;
use voidmc_protocol::serverbound::Handshake;
use voidmc_protocol::{MINECRAFT_VERSION, PROTOCOL_VERSION, State};

pub use self::events::Events;
use self::peer::{Peer, connect, next_of};
pub use self::status::probe;
use self::status::{relay_status, watch_backend};
use crate::frame::{self, FrameCodecError};
use crate::session::{Action, Session, Settings};

const TICK: Duration = Duration::from_millis(50);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

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
            Ok(Some(first)) => self.handle_backend_frames(first),
            Ok(None) => {}
            Err(error) => self.lose_backend(&error),
        }
    }

    /// Takes in `bytes` read from the client, and every frame they complete.
    pub fn received_from_client(&mut self, bytes: &[u8]) -> Result<(), FrameCodecError> {
        self.client.inbound.extend_from_slice(bytes);
        match self.client.buffered()? {
            Some(first) => self.handle_client_frames(first),
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
                Action::Event(event) => self.events.emit_session(self.id, event),
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
    fn handle_backend_frames(&mut self, first: Bytes) {
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

    fn handle_client_frames(&mut self, first: Bytes) -> Result<(), FrameCodecError> {
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

struct Keeper {
    relay: Relay<TcpStream>,
    backend_address: SocketAddr,
}

impl Relay<TcpStream> {
    /// Writes what is queued for the backend. `false` when that lost it.
    async fn flush_backend(&mut self) -> bool {
        let Some(backend) = &mut self.backend else {
            return true;
        };
        match backend.flush().await {
            Ok(()) => true,
            Err(error) => {
                self.lose_backend(&error);
                false
            }
        }
    }
}

impl Keeper {
    /// Writes what is queued and opens a backend if one is wanted. `false` once the client
    /// is gone, or was sent away.
    async fn settle(&mut self) -> bool {
        let relay = &mut self.relay;
        if relay.client.flush().await.is_err() || relay.closing {
            return false;
        }
        if !relay.flush_backend().await {
            return relay.client.flush().await.is_ok();
        }
        if !std::mem::take(&mut relay.wants_backend) {
            return true;
        }
        match connect(self.backend_address).await {
            Some(stream) => relay.backend_connected(stream),
            None => {
                relay.session.on_connect_failed(Instant::now());
                relay.absorb();
            }
        }
        relay.flush_backend().await;
        relay.client.flush().await.is_ok()
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
                        if let Err(error) = relay.handle_client_frames(frame) {
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
                    Ok(Some(frame)) => relay.handle_backend_frames(frame),
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
    client.queue(&frame::encode(login_cb::LOGIN_DISCONNECT, |body| {
        let json = json!({ "text": format!("[keeper] {message}") });
        json.to_string().encode(body);
    }));
    let _ = client.flush().await;
    let _ = client.stream.shutdown().await;
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
    let Some((_, mut body)) = frame::id_and_body(&handshake) else {
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
    let mut id = 0;
    loop {
        let (stream, _) = next_connection(|| listener.accept()).await;
        id += 1;
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
