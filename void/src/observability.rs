//! Opt-in health and Prometheus metrics endpoints.
//!
//! Enabled with [`ServerConfigBuilder::observability`]: `VoidServer::run` then
//! binds a second TCP listener, served by a small HTTP responder on a thread
//! of its own (never the game thread), so that a stalled game loop can still
//! be reported. The game thread and the network thread record into a shared
//! [`Telemetry`] made of atomics; the HTTP side only reads it.
//!
//! [`ServerConfigBuilder::observability`]: crate::ServerConfigBuilder::observability

mod exposition;
mod http;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bevy_app::{App, AppExit, Plugin};
use bevy_ecs::prelude::*;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use voidmc_net::socket::SocketError;

use crate::components::PlayerReady;
use crate::config::ServerConfigResource;
use crate::tick_time::{self, TickEnd, TickMeasured, TickWorkTime};
use crate::world::{ChunkLoadErrorKind, ChunkLoadFailedEvent};
use exposition::{MetricKind, TextEncoder};
use http::{Method, Request, Response};

/// Host the endpoints are documented to bind: loopback only.
pub const DEFAULT_OBSERVABILITY_HOST: &str = "127.0.0.1";

/// Without an explicit threshold, the game loop counts as stalled once no tick
/// has completed for this many tick intervals...
const DEFAULT_STALL_TICKS: u32 = 40;
/// ...or for this long, whichever is longer.
const MIN_DEFAULT_STALL_THRESHOLD: Duration = Duration::from_secs(1);

/// More concurrent HTTP connections than this are closed on accept.
const MAX_CONNECTIONS: usize = 32;
/// How long `VoidServer::run` waits, on its way out, for the listener to close.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);

/// Upper bounds, in seconds, of the `voidmc_tick_duration_seconds` buckets.
const TICK_BUCKETS: [f64; 11] = [
    0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5,
];

/// Where and how to serve `/health` and `/metrics`.
///
/// The endpoints have no authentication and no TLS: bind them to a loopback
/// or private interface only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservabilityConfig {
    /// TCP address to bind, such as `"127.0.0.1:9464"`. Port `0` picks a free
    /// port, published as the [`ObservabilityAddress`] resource.
    pub address: String,
    /// How long without a completed tick before `/health` reports `stalled`.
    /// `None` means 40 tick intervals, and never less than one second.
    pub stall_threshold: Option<Duration>,
}

impl ObservabilityConfig {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            stall_threshold: None,
        }
    }

    /// Binds [`DEFAULT_OBSERVABILITY_HOST`] on `port`.
    pub fn localhost(port: u16) -> Self {
        Self::new(format!("{DEFAULT_OBSERVABILITY_HOST}:{port}"))
    }

    pub fn stall_threshold(mut self, threshold: Duration) -> Self {
        self.stall_threshold = Some(threshold);
        self
    }

    /// The stall threshold in effect for a server ticking every `tick_duration`.
    pub fn effective_stall_threshold(&self, tick_duration: Duration) -> Duration {
        self.stall_threshold.unwrap_or_else(|| {
            (tick_duration * DEFAULT_STALL_TICKS).max(MIN_DEFAULT_STALL_THRESHOLD)
        })
    }
}

/// The address the observability endpoints listen on, inserted as a resource
/// when they are enabled. Differs from the configured address when binding
/// port 0.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservabilityAddress(pub SocketAddr);

/// What `/health` reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Health {
    /// No tick has completed yet. `503`.
    Starting,
    /// A tick completed within the stall threshold. `200`.
    Ok,
    /// No tick completed within the stall threshold. `503`.
    Stalled,
    /// The server is shutting down. `503`.
    Stopping,
}

impl Health {
    fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ok => "ok",
            Self::Stalled => "stalled",
            Self::Stopping => "stopping",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ConnectionErrorKind {
    Io,
    Frame,
    Decode,
}

impl ConnectionErrorKind {
    const ALL: [Self; 3] = [Self::Io, Self::Frame, Self::Decode];

    fn as_str(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Frame => "frame",
            Self::Decode => "decode",
        }
    }
}

const CHUNK_LOAD_ERROR_KINDS: [ChunkLoadErrorKind; 2] =
    [ChunkLoadErrorKind::Decode, ChunkLoadErrorKind::Io];

const NO_SAMPLE: u64 = u64::MAX;

/// Counters and gauges shared by the game thread, the network thread and the
/// HTTP responder.
#[derive(Debug)]
pub(crate) struct Telemetry {
    epoch: Instant,
    stall_threshold: Duration,
    target_tps: u64,
    stopping: AtomicBool,
    ticks: AtomicU64,
    /// Nanoseconds from `epoch` to the end of the last tick.
    last_tick_end: AtomicU64,
    /// Non-cumulative: index `i` counts ticks in `(TICK_BUCKETS[i-1], TICK_BUCKETS[i]]`,
    /// the last index the ticks above every bound.
    tick_buckets: [AtomicU64; TICK_BUCKETS.len() + 1],
    tick_nanos_sum: AtomicU64,
    /// `f64` bits of the ticks per second measured over the last full window.
    measured_tps: AtomicU64,
    players_online: AtomicU64,
    players_max: AtomicI64,
    players_joined: AtomicU64,
    connections_accepted: AtomicU64,
    connection_errors: [AtomicU64; 3],
    clients_kicked: AtomicU64,
    packets_unrecognized: AtomicU64,
    packet_errors: AtomicU64,
    /// Indexed like [`CHUNK_LOAD_ERROR_KINDS`].
    chunk_load_errors: [AtomicU64; 2],
    #[cfg(test)]
    probe: std::sync::OnceLock<Probe>,
}

/// A reader on another thread that a test lets read the telemetry after each
/// write of the game thread's end of tick, while that thread waits.
#[cfg(test)]
#[derive(Debug)]
struct Probe {
    /// Sends the name of the write that just completed.
    written: flume::Sender<Option<&'static str>>,
    /// Receives once the reader is done reading.
    read: flume::Receiver<()>,
}

impl Telemetry {
    pub(crate) fn new(stall_threshold: Duration, target_tps: u64, max_players: i32) -> Self {
        Self {
            epoch: Instant::now(),
            stall_threshold,
            target_tps,
            stopping: AtomicBool::new(false),
            ticks: AtomicU64::new(0),
            last_tick_end: AtomicU64::new(0),
            tick_buckets: Default::default(),
            tick_nanos_sum: AtomicU64::new(0),
            measured_tps: AtomicU64::new(NO_SAMPLE),
            players_online: AtomicU64::new(0),
            players_max: AtomicI64::new(max_players.into()),
            players_joined: AtomicU64::new(0),
            connections_accepted: AtomicU64::new(0),
            connection_errors: Default::default(),
            clients_kicked: AtomicU64::new(0),
            packets_unrecognized: AtomicU64::new(0),
            packet_errors: AtomicU64::new(0),
            chunk_load_errors: Default::default(),
            #[cfg(test)]
            probe: std::sync::OnceLock::new(),
        }
    }

    /// Under test, lets the probe, if one is set, read the telemetry before
    /// the game thread writes anything else. Does nothing otherwise.
    fn written(&self, _write: &'static str) {
        #[cfg(test)]
        if let Some(probe) = self.probe.get() {
            probe
                .written
                .send(Some(_write))
                .expect("the probe reads until the test ends");
            probe
                .read
                .recv()
                .expect("the probe reads until the test ends");
        }
    }

    fn nanos_since_epoch(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.epoch).as_nanos()).unwrap_or(u64::MAX)
    }

    pub(crate) fn record_tick(&self, duration: Duration, ended_at: Instant) {
        let seconds = duration.as_secs_f64();
        let bucket = TICK_BUCKETS
            .iter()
            .position(|bound| seconds <= *bound)
            .unwrap_or(TICK_BUCKETS.len());
        self.tick_buckets[bucket].fetch_add(1, Ordering::Relaxed);
        self.tick_nanos_sum.fetch_add(
            u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        self.last_tick_end
            .store(self.nanos_since_epoch(ended_at), Ordering::Relaxed);
        self.ticks.fetch_add(1, Ordering::Release);
        self.written("record_tick");
    }

    fn record_measured_tps(&self, tps: f64) {
        self.measured_tps.store(tps.to_bits(), Ordering::Relaxed);
        self.written("record_measured_tps");
    }

    fn set_players(&self, online: usize, max: i32) {
        self.players_online.store(online as u64, Ordering::Relaxed);
        self.players_max.store(max.into(), Ordering::Relaxed);
        self.written("set_players");
    }

    fn add_players_joined(&self, joined: usize) {
        if joined > 0 {
            self.players_joined
                .fetch_add(joined as u64, Ordering::Relaxed);
        }
        self.written("add_players_joined");
    }

    pub(crate) fn connection_accepted(&self) {
        self.connections_accepted.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts a connection that ended with `error`. A peer closing the
    /// connection is not an error.
    pub(crate) fn connection_ended_with(&self, error: &SocketError) {
        let kind = match error {
            SocketError::PeerClosed => return,
            SocketError::Io(_) => ConnectionErrorKind::Io,
            SocketError::Frame(_) => ConnectionErrorKind::Frame,
            SocketError::Decode(_) => ConnectionErrorKind::Decode,
        };
        self.connection_errors[kind as usize].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn client_kicked(&self) {
        self.clients_kicked.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn packet_unrecognized(&self) {
        self.packets_unrecognized.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn packet_failed(&self) {
        self.packet_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn chunk_load_failed(&self, kind: ChunkLoadErrorKind) {
        let index = CHUNK_LOAD_ERROR_KINDS
            .iter()
            .position(|known| *known == kind)
            .expect("every kind is listed");
        self.chunk_load_errors[index].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn begin_stopping(&self) {
        self.stopping.store(true, Ordering::Release);
        self.written("begin_stopping");
    }

    /// Time since the last tick completed, `None` before the first one.
    fn last_tick_age(&self, now: Instant) -> Option<Duration> {
        if self.ticks.load(Ordering::Acquire) == 0 {
            return None;
        }
        let ended = self.last_tick_end.load(Ordering::Relaxed);
        Some(Duration::from_nanos(
            self.nanos_since_epoch(now).saturating_sub(ended),
        ))
    }

    pub(crate) fn health(&self, now: Instant) -> Health {
        if self.stopping.load(Ordering::Acquire) {
            return Health::Stopping;
        }
        match self.last_tick_age(now) {
            None => Health::Starting,
            Some(age) if age > self.stall_threshold => Health::Stalled,
            Some(_) => Health::Ok,
        }
    }

    /// The `/health` response.
    pub(crate) fn health_response(&self, now: Instant) -> Response {
        let health = self.health(now);
        let status = if health == Health::Ok { 200 } else { 503 };
        let age = self
            .last_tick_age(now)
            .map_or_else(|| "null".to_string(), |age| age.as_millis().to_string());
        let body = format!(
            "{{\"status\":\"{}\",\"ticks\":{},\"last_tick_age_ms\":{},\"stall_threshold_ms\":{}}}\n",
            health.as_str(),
            self.ticks.load(Ordering::Acquire),
            age,
            self.stall_threshold.as_millis()
        );
        Response::new(status, "application/json", body)
    }

    /// The `/metrics` body, in Prometheus text format 0.0.4.
    pub(crate) fn render_metrics(&self, now: Instant) -> String {
        let mut out = TextEncoder::default();
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed) as f64;

        out.family(
            "voidmc_ticks_total",
            MetricKind::Counter,
            "Game ticks completed since the server started.",
        );
        out.sample(
            "voidmc_ticks_total",
            &[],
            self.ticks.load(Ordering::Acquire) as f64,
        );

        out.family(
            "voidmc_tick_duration_seconds",
            MetricKind::Histogram,
            "Time the game thread spent running one tick, from the start of First to the end of Last.",
        );
        let mut cumulative = 0u64;
        for (index, bound) in TICK_BUCKETS.iter().enumerate() {
            cumulative += self.tick_buckets[index].load(Ordering::Relaxed);
            let le = exposition::format_value(*bound);
            out.sample(
                "voidmc_tick_duration_seconds_bucket",
                &[("le", &le)],
                cumulative as f64,
            );
        }
        cumulative += self.tick_buckets[TICK_BUCKETS.len()].load(Ordering::Relaxed);
        out.sample(
            "voidmc_tick_duration_seconds_bucket",
            &[("le", "+Inf")],
            cumulative as f64,
        );
        out.sample(
            "voidmc_tick_duration_seconds_sum",
            &[],
            Duration::from_nanos(self.tick_nanos_sum.load(Ordering::Relaxed)).as_secs_f64(),
        );
        out.sample("voidmc_tick_duration_seconds_count", &[], cumulative as f64);

        out.family(
            "voidmc_target_ticks_per_second",
            MetricKind::Gauge,
            "Configured tick rate.",
        );
        out.sample(
            "voidmc_target_ticks_per_second",
            &[],
            self.target_tps as f64,
        );

        out.family(
            "voidmc_ticks_per_second",
            MetricKind::Gauge,
            "Ticks per second measured over the last full one-second window; absent until the first window ends.",
        );
        let tps = self.measured_tps.load(Ordering::Relaxed);
        if tps != NO_SAMPLE {
            out.sample("voidmc_ticks_per_second", &[], f64::from_bits(tps));
        }

        out.family(
            "voidmc_seconds_since_last_tick",
            MetricKind::Gauge,
            "Seconds since the last tick completed, at scrape time; absent before the first tick.",
        );
        if let Some(age) = self.last_tick_age(now) {
            out.sample("voidmc_seconds_since_last_tick", &[], age.as_secs_f64());
        }

        out.family(
            "voidmc_players_online",
            MetricKind::Gauge,
            "Players in the play state at the end of the last tick, as in the server list.",
        );
        out.sample("voidmc_players_online", &[], load(&self.players_online));

        out.family(
            "voidmc_players_max",
            MetricKind::Gauge,
            "Configured maximum player count, as in the server list.",
        );
        out.sample(
            "voidmc_players_max",
            &[],
            self.players_max.load(Ordering::Relaxed) as f64,
        );

        out.family(
            "voidmc_players_joined_total",
            MetricKind::Counter,
            "Players that reached the play state.",
        );
        out.sample(
            "voidmc_players_joined_total",
            &[],
            load(&self.players_joined),
        );

        out.family(
            "voidmc_connections_accepted_total",
            MetricKind::Counter,
            "TCP connections accepted on the game port, server-list pings included.",
        );
        out.sample(
            "voidmc_connections_accepted_total",
            &[],
            load(&self.connections_accepted),
        );

        out.family(
            "voidmc_connection_errors_total",
            MetricKind::Counter,
            "Game connections closed by an error: io (socket error), frame (invalid framing or compression) or decode (undecodable packet on the network thread).",
        );
        for kind in ConnectionErrorKind::ALL {
            out.sample(
                "voidmc_connection_errors_total",
                &[("kind", kind.as_str())],
                load(&self.connection_errors[kind as usize]),
            );
        }

        out.family(
            "voidmc_clients_kicked_total",
            MetricKind::Counter,
            "Live connections the server closed itself: kicks, keep-alive timeouts, outbound queues that overflowed and despawned client entities.",
        );
        out.sample(
            "voidmc_clients_kicked_total",
            &[],
            load(&self.clients_kicked),
        );

        out.family(
            "voidmc_packets_unrecognized_total",
            MetricKind::Counter,
            "Packets dropped because their id is unknown in the connection's state.",
        );
        out.sample(
            "voidmc_packets_unrecognized_total",
            &[],
            load(&self.packets_unrecognized),
        );

        out.family(
            "voidmc_packet_errors_total",
            MetricKind::Counter,
            "Packets with a known id that the game thread failed to decode.",
        );
        out.sample("voidmc_packet_errors_total", &[], load(&self.packet_errors));

        out.family(
            "voidmc_chunk_load_errors_total",
            MetricKind::Counter,
            "Stored chunks that could not be loaded and were generated instead: decode (stored bytes undecodable) or io (stored bytes unreadable).",
        );
        for (kind, count) in CHUNK_LOAD_ERROR_KINDS.iter().zip(&self.chunk_load_errors) {
            out.sample(
                "voidmc_chunk_load_errors_total",
                &[("kind", kind.as_str())],
                load(count),
            );
        }

        out.finish()
    }

    fn route(&self, request: &Request<'_>) -> Response {
        let known = matches!(request.path, "/health" | "/metrics");
        if !known {
            return Response::text(404, "not found; try /health or /metrics");
        }
        if request.method == Method::Other {
            let mut response = Response::text(405, "method not allowed");
            response.allow = Some("GET, HEAD");
            return response;
        }
        let now = Instant::now();
        if request.path == "/health" {
            self.health_response(now)
        } else {
            Response::new(200, exposition::CONTENT_TYPE, self.render_metrics(now))
        }
    }
}

/// The game-thread side, registered by `VoidServer::run`.
pub(crate) struct ObservabilityPlugin {
    telemetry: Arc<Telemetry>,
    address: SocketAddr,
}

#[derive(Resource, Clone)]
pub(crate) struct TelemetryHandle(pub(crate) Arc<Telemetry>);

/// The window the measured tick rate is counted over.
#[derive(Resource)]
struct TickClock {
    window_start: Instant,
    window_ticks: u32,
}

impl Plugin for ObservabilityPlugin {
    fn build(&self, app: &mut App) {
        tick_time::require(app);
        app.insert_resource(TelemetryHandle(self.telemetry.clone()))
            .insert_resource(ObservabilityAddress(self.address))
            .insert_resource(TickClock {
                window_start: Instant::now(),
                window_ticks: 0,
            })
            .add_systems(TickEnd, end_tick.after(TickMeasured))
            .add_observer(count_chunk_load_failure);
    }
}

fn count_chunk_load_failure(event: On<ChunkLoadFailedEvent>, telemetry: Res<TelemetryHandle>) {
    telemetry.0.chunk_load_failed(event.kind);
}

fn end_tick(
    telemetry: Res<TelemetryHandle>,
    work: Res<TickWorkTime>,
    mut clock: ResMut<TickClock>,
    config: Option<Res<ServerConfigResource>>,
    online: Query<(), With<PlayerReady>>,
    joined: Query<(), Added<PlayerReady>>,
    mut exits: MessageReader<AppExit>,
) {
    let telemetry = &telemetry.0;
    if !exits.is_empty() {
        exits.clear();
        telemetry.begin_stopping();
    }
    let max_players = config.map_or(0, |config| config.max_players);
    telemetry.set_players(online.iter().count(), max_players);
    telemetry.add_players_joined(joined.iter().count());

    let now = work.ended_at;
    telemetry.record_tick(work.last, now);

    clock.window_ticks += 1;
    let window = now - clock.window_start;
    if window >= Duration::from_secs(1) {
        telemetry.record_measured_tps(f64::from(clock.window_ticks) / window.as_secs_f64());
        clock.window_start = now;
        clock.window_ticks = 0;
    }
}

/// The HTTP side: a listener served on its own thread.
struct Endpoint {
    listener: std::net::TcpListener,
    telemetry: Arc<Telemetry>,
    stop: flume::Receiver<()>,
    closed: flume::Sender<()>,
}

/// Held by `VoidServer::run`: on drop, reports `stopping` and closes the
/// listener, waiting a bounded time for it to be closed.
pub(crate) struct EndpointGuard {
    telemetry: Arc<Telemetry>,
    stop: Option<flume::Sender<()>>,
    closed: flume::Receiver<()>,
}

impl Drop for EndpointGuard {
    fn drop(&mut self) {
        self.telemetry.begin_stopping();
        drop(self.stop.take());
        // The serving thread drops the listener before its `closed` sender,
        // so a disconnected channel means nothing listens any more.
        if let Err(flume::RecvTimeoutError::Timeout) = self.closed.recv_timeout(SHUTDOWN_WAIT) {
            tracing::warn!("Observability endpoint did not close in time");
        }
    }
}

/// Running endpoints, as `VoidServer::run` wires them.
pub(crate) struct Observability {
    pub(crate) guard: EndpointGuard,
    pub(crate) plugin: ObservabilityPlugin,
    pub(crate) telemetry: Arc<Telemetry>,
}

impl Observability {
    /// Serves `/health` and `/metrics` on `listener` from a thread of its own,
    /// so that the endpoints outlive the game loop and the network runtime
    /// until the returned guard is dropped.
    pub(crate) fn start(
        config: &ObservabilityConfig,
        listener: std::net::TcpListener,
        tick_duration: Duration,
        tick_rate: u64,
        max_players: i32,
    ) -> Self {
        let address = listener
            .local_addr()
            .expect("a bound listener has a local address");
        let telemetry = Arc::new(Telemetry::new(
            config.effective_stall_threshold(tick_duration),
            tick_rate,
            max_players,
        ));
        let (stop_tx, stop_rx) = flume::bounded(1);
        let (closed_tx, closed_rx) = flume::bounded(1);
        let endpoint = Endpoint {
            listener,
            telemetry: telemetry.clone(),
            stop: stop_rx,
            closed: closed_tx,
        };
        std::thread::Builder::new()
            .name("void-observability".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build the observability runtime");
                runtime.block_on(endpoint.serve());
            })
            .expect("Failed to spawn the observability thread");
        Self {
            guard: EndpointGuard {
                telemetry: telemetry.clone(),
                stop: Some(stop_tx),
                closed: closed_rx,
            },
            plugin: ObservabilityPlugin {
                telemetry: telemetry.clone(),
                address,
            },
            telemetry,
        }
    }
}

impl Endpoint {
    async fn serve(self) {
        struct Serving {
            listener: TcpListener,
            // Declared after the listener so that it is dropped after it,
            // even if the runtime cancels this future.
            _closed: flume::Sender<()>,
        }

        let Self {
            listener,
            telemetry,
            stop,
            closed,
        } = self;
        let registered = listener
            .set_nonblocking(true)
            .and_then(|()| TcpListener::from_std(listener));
        let serving = Serving {
            listener: match registered {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::error!(error = %error, "Failed to register the observability listener with Tokio");
                    return;
                }
            },
            _closed: closed,
        };
        if let Ok(address) = serving.listener.local_addr() {
            tracing::info!(listen_addr = %address, "Observability endpoints listening on /health and /metrics");
        }
        let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        loop {
            tokio::select! {
                _ = stop.recv_async() => break,
                accepted = serving.listener.accept() => {
                    let (stream, peer) = match accepted {
                        Ok(accepted) => accepted,
                        Err(error) => {
                            tracing::warn!(error = %error, "Failed to accept an observability connection");
                            continue;
                        }
                    };
                    let Ok(permit) = permits.clone().try_acquire_owned() else {
                        tracing::debug!(peer = %peer, "Too many observability connections; closing");
                        continue;
                    };
                    let telemetry = telemetry.clone();
                    tokio::spawn(async move {
                        http::serve_connection(stream, http::REQUEST_DEADLINE, |request| {
                            telemetry.route(request)
                        })
                        .await;
                        drop(permit);
                    });
                }
            }
        }
        drop(serving);
        tracing::info!("Observability endpoints closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn telemetry() -> Telemetry {
        Telemetry::new(Duration::from_secs(2), 20, 100)
    }

    #[test]
    fn default_stall_threshold_follows_the_tick_rate_with_a_floor() {
        let config = ObservabilityConfig::localhost(0);
        assert_eq!(config.address, "127.0.0.1:0");
        assert_eq!(
            config.effective_stall_threshold(Duration::from_millis(50)),
            Duration::from_secs(2)
        );
        assert_eq!(
            config.effective_stall_threshold(Duration::from_millis(10)),
            Duration::from_secs(1)
        );
        assert_eq!(
            config
                .stall_threshold(Duration::from_millis(300))
                .effective_stall_threshold(Duration::from_millis(50)),
            Duration::from_millis(300)
        );
    }

    #[test]
    fn health_moves_from_starting_to_ok_to_stalled_to_stopping() {
        let telemetry = telemetry();
        let start = telemetry.epoch;
        assert_eq!(telemetry.health(start), Health::Starting);

        let tick_end = start + Duration::from_millis(100);
        telemetry.record_tick(Duration::from_millis(3), tick_end);
        assert_eq!(telemetry.health(tick_end), Health::Ok);
        assert_eq!(
            telemetry.health(tick_end + Duration::from_secs(2)),
            Health::Ok
        );
        assert_eq!(
            telemetry.health(tick_end + Duration::from_millis(2001)),
            Health::Stalled
        );

        telemetry.begin_stopping();
        assert_eq!(telemetry.health(tick_end), Health::Stopping);
    }

    #[test]
    fn health_response_is_200_only_when_ok() {
        let telemetry = telemetry();
        let start = telemetry.epoch;
        let response = telemetry.health_response(start);
        assert_eq!(response.status, 503);
        assert_eq!(
            response.body,
            "{\"status\":\"starting\",\"ticks\":0,\"last_tick_age_ms\":null,\"stall_threshold_ms\":2000}\n"
        );

        telemetry.record_tick(Duration::from_millis(3), start);
        let response = telemetry.health_response(start + Duration::from_millis(5));
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(
            response.body,
            "{\"status\":\"ok\",\"ticks\":1,\"last_tick_age_ms\":5,\"stall_threshold_ms\":2000}\n"
        );

        let response = telemetry.health_response(start + Duration::from_secs(3));
        assert_eq!(response.status, 503);
        assert!(response.body.starts_with("{\"status\":\"stalled\""));
    }

    #[test]
    fn tick_durations_fill_cumulative_buckets() {
        let telemetry = telemetry();
        let now = telemetry.epoch;
        telemetry.record_tick(Duration::from_micros(500), now);
        telemetry.record_tick(Duration::from_millis(1), now);
        telemetry.record_tick(Duration::from_millis(30), now);
        telemetry.record_tick(Duration::from_secs(3), now);
        let text = telemetry.render_metrics(now);

        for line in [
            "voidmc_ticks_total 4\n",
            "voidmc_tick_duration_seconds_bucket{le=\"0.001\"} 2\n",
            "voidmc_tick_duration_seconds_bucket{le=\"0.025\"} 2\n",
            "voidmc_tick_duration_seconds_bucket{le=\"0.05\"} 3\n",
            "voidmc_tick_duration_seconds_bucket{le=\"2.5\"} 3\n",
            "voidmc_tick_duration_seconds_bucket{le=\"+Inf\"} 4\n",
            "voidmc_tick_duration_seconds_sum 3.0315\n",
            "voidmc_tick_duration_seconds_count 4\n",
            "voidmc_seconds_since_last_tick 0\n",
        ] {
            assert!(text.contains(line), "missing {line:?} in\n{text}");
        }
    }

    #[test]
    fn metrics_count_what_the_threads_record() {
        let telemetry = telemetry();
        let now = telemetry.epoch;
        let text = telemetry.render_metrics(now);
        assert!(!text.contains("\nvoidmc_ticks_per_second "));
        assert!(!text.contains("\nvoidmc_seconds_since_last_tick "));

        telemetry.set_players(3, 50);
        telemetry.add_players_joined(2);
        telemetry.add_players_joined(0);
        telemetry.connection_accepted();
        telemetry.connection_ended_with(&SocketError::PeerClosed);
        telemetry.connection_ended_with(&SocketError::Frame(
            voidmc_net::socket::FrameError::EmptyFrame,
        ));
        telemetry.connection_ended_with(&SocketError::Io(std::io::ErrorKind::Other.into()));
        telemetry.client_kicked();
        telemetry.packet_unrecognized();
        telemetry.packet_unrecognized();
        telemetry.packet_failed();
        telemetry.chunk_load_failed(ChunkLoadErrorKind::Io);
        telemetry.record_measured_tps(19.5);
        let text = telemetry.render_metrics(now);

        for line in [
            "# TYPE voidmc_players_online gauge\nvoidmc_players_online 3\n",
            "voidmc_players_max 50\n",
            "voidmc_players_joined_total 2\n",
            "voidmc_connections_accepted_total 1\n",
            "voidmc_connection_errors_total{kind=\"io\"} 1\n",
            "voidmc_connection_errors_total{kind=\"frame\"} 1\n",
            "voidmc_connection_errors_total{kind=\"decode\"} 0\n",
            "voidmc_clients_kicked_total 1\n",
            "voidmc_packets_unrecognized_total 2\n",
            "voidmc_packet_errors_total 1\n",
            "voidmc_chunk_load_errors_total{kind=\"decode\"} 0\n",
            "voidmc_chunk_load_errors_total{kind=\"io\"} 1\n",
            "voidmc_ticks_per_second 19.5\n",
            "voidmc_target_ticks_per_second 20\n",
        ] {
            assert!(text.contains(line), "missing {line:?} in\n{text}");
        }
    }

    #[test]
    fn plugin_measures_whole_ticks_players_and_shutdown() {
        let telemetry = Arc::new(telemetry());
        let mut app = App::new();
        app.add_plugins(ObservabilityPlugin {
            telemetry: telemetry.clone(),
            address: "127.0.0.1:1".parse().unwrap(),
        })
        .insert_resource(ServerConfigResource::from(&crate::ServerConfig {
            max_players: 7,
            ..Default::default()
        }))
        .add_systems(bevy_app::Update, || {
            std::thread::sleep(Duration::from_millis(2));
        });
        app.world_mut().spawn(PlayerReady);
        app.update();
        app.world_mut().spawn(PlayerReady);
        app.update();
        app.update();

        let now = Instant::now();
        assert_eq!(telemetry.health(now), Health::Ok);
        let text = telemetry.render_metrics(now);
        for line in [
            "voidmc_ticks_total 3\n",
            "voidmc_tick_duration_seconds_bucket{le=\"0.001\"} 0\n",
            "voidmc_tick_duration_seconds_count 3\n",
            "voidmc_players_online 2\n",
            "voidmc_players_max 7\n",
            "voidmc_players_joined_total 2\n",
        ] {
            assert!(text.contains(line), "missing {line:?} in\n{text}");
        }
        assert_eq!(
            app.world().resource::<ObservabilityAddress>().0,
            "127.0.0.1:1".parse().unwrap()
        );

        app.world_mut().write_message(AppExit::Success);
        app.update();
        assert_eq!(telemetry.health(Instant::now()), Health::Stopping);
    }

    /// Reads `/health` and `/metrics` from another thread after each write of
    /// the exit tick, the way a scrape can land between any two of them, and
    /// checks that every read reporting `stopping` also reports the gauges of
    /// that tick.
    #[test]
    fn a_reader_that_sees_stopping_sees_the_gauges_of_the_exit_tick() {
        let telemetry = Arc::new(telemetry());
        let mut app = App::new();
        app.add_plugins(ObservabilityPlugin {
            telemetry: telemetry.clone(),
            address: "127.0.0.1:1".parse().unwrap(),
        })
        .insert_resource(ServerConfigResource::from(&crate::ServerConfig {
            max_players: 7,
            ..Default::default()
        }));
        let alice = app.world_mut().spawn(PlayerReady).id();
        let bob = app.world_mut().spawn(PlayerReady).id();
        app.update();
        app.update();
        let before = telemetry.render_metrics(Instant::now());
        assert!(before.contains("voidmc_players_online 2\n"), "{before}");

        let (written_tx, written_rx) = flume::bounded(0);
        let (read_tx, read_rx) = flume::bounded(0);
        telemetry
            .probe
            .set(Probe {
                written: written_tx.clone(),
                read: read_rx,
            })
            .unwrap();
        let reader = std::thread::spawn({
            let telemetry = telemetry.clone();
            move || {
                let mut reads = Vec::new();
                while let Ok(Some(write)) = written_rx.recv() {
                    let now = Instant::now();
                    let health = telemetry.health_response(now).body;
                    let metrics = telemetry.render_metrics(now);
                    reads.push((write, health, metrics));
                    read_tx.send(()).unwrap();
                }
                reads
            }
        });

        // The exit tick: the players have left and the server stops.
        app.world_mut().despawn(alice);
        app.world_mut().despawn(bob);
        app.world_mut().write_message(AppExit::Success);
        app.update();
        written_tx.send(None).unwrap();
        let reads = reader.join().unwrap();

        let stopping: Vec<_> = reads
            .iter()
            .filter(|(_, health, _)| health.starts_with("{\"status\":\"stopping\""))
            .collect();
        assert!(!stopping.is_empty(), "never saw stopping in {reads:#?}");
        for (write, health, metrics) in stopping {
            for line in [
                "voidmc_players_online 0\n",
                "voidmc_players_max 7\n",
                "voidmc_players_joined_total 2\n",
                "voidmc_ticks_total 3\n",
            ] {
                assert!(
                    metrics.contains(line),
                    "after {write}, /health reports stopping but /metrics lacks {line:?}:\n{health}{metrics}"
                );
            }
            assert!(
                health.contains("\"ticks\":3,"),
                "after {write}, /health reports stopping before the exit tick is counted: {health}"
            );
        }
    }

    #[test]
    fn plugin_counts_chunk_load_failures_by_kind() {
        let telemetry = Arc::new(telemetry());
        let mut app = App::new();
        app.add_plugins(ObservabilityPlugin {
            telemetry: telemetry.clone(),
            address: "127.0.0.1:1".parse().unwrap(),
        });
        let failure = |kind| ChunkLoadFailedEvent {
            dimension: crate::world::DimensionId::Overworld,
            pos: crate::world::ChunkPos::new(1, 0),
            kind,
            error: "test".into(),
            action: crate::world::DamagedChunkAction::SavesSkipped,
        };
        app.world_mut().trigger(failure(ChunkLoadErrorKind::Decode));
        app.world_mut().trigger(failure(ChunkLoadErrorKind::Decode));
        app.world_mut().trigger(failure(ChunkLoadErrorKind::Io));

        let text = telemetry.render_metrics(Instant::now());
        for line in [
            "# TYPE voidmc_chunk_load_errors_total counter\n",
            "voidmc_chunk_load_errors_total{kind=\"decode\"} 2\n",
            "voidmc_chunk_load_errors_total{kind=\"io\"} 1\n",
        ] {
            assert!(text.contains(line), "missing {line:?} in\n{text}");
        }
    }

    #[test]
    fn dropping_the_guard_closes_the_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let observability = Observability::start(
            &ObservabilityConfig::localhost(0),
            listener,
            Duration::from_millis(50),
            20,
            100,
        );

        let mut stream = std::net::TcpStream::connect(address).unwrap();
        std::io::Write::write_all(&mut stream, b"GET /health HTTP/1.1\r\n\r\n").unwrap();
        let mut response = String::new();
        std::io::Read::read_to_string(&mut stream, &mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 503 "), "{response}");
        assert!(response.contains("\"status\":\"starting\""), "{response}");

        drop(observability.guard);
        assert_eq!(
            observability.telemetry.health(Instant::now()),
            Health::Stopping
        );
        assert_eq!(
            std::net::TcpStream::connect(address).unwrap_err().kind(),
            std::io::ErrorKind::ConnectionRefused
        );
    }

    #[test]
    fn routes_known_paths_and_rejects_the_rest() {
        let telemetry = telemetry();
        let request = |method, path| Request { method, path };

        let metrics = telemetry.route(&request(Method::Get, "/metrics"));
        assert_eq!(metrics.status, 200);
        assert_eq!(metrics.content_type, exposition::CONTENT_TYPE);
        assert!(metrics.body.starts_with("# HELP voidmc_ticks_total "));

        assert_eq!(
            telemetry.route(&request(Method::Head, "/health")).status,
            503
        );
        assert_eq!(telemetry.route(&request(Method::Get, "/")).status, 404);
        assert_eq!(
            telemetry.route(&request(Method::Other, "/nope")).status,
            404
        );
        let post = telemetry.route(&request(Method::Other, "/metrics"));
        assert_eq!(post.status, 405);
        assert_eq!(post.allow, Some("GET, HEAD"));
    }
}
