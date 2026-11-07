//! One JSON line per packet, in and out, with its tick and the system behind it.
//!
//! Needs the `packet-trace` feature, and `VOID_TRACE=<file>` at run time:
//!
//! ```json
//! {"tick":661,"dir":"out","client":1,"player":"sonde","packet":"Play/KeepAlive","bytes":9,"hash":123,"system":"voidmc::systems::keep_alive::tick"}
//! ```
//!
//! `bytes` counts the packet id and the payload: no frame length, and before any compression.
//! `hash` is the FNV-1a of those bytes.
//! `system` is absent when no system was running. It only appears at all with [`trace_layer`]
//! in the subscriber of the binary.
//!
//! `dropped` says why an outgoing packet never reached the queue of its client: `full`,
//! `kicked` or `closed`. The network thread answers status connections by itself, and those
//! do not pass here.
//!
//! Lines are flushed at the end of each tick and when a thread panics. A write that fails ends
//! the trace, with one error in the logs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};

use bevy_app::{App, First, Last, Plugin};
use bevy_ecs::prelude::*;
use serde::Serialize;
use tracing::span::{Attributes, Id};
use tracing::{Subscriber, field};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use voidmc_codec::{Encode, PacketName};
use voidmc_protocol::clientbound::ClientboundPacket;

use crate::pace::{self, Pace};

const PATH_VARIABLE: &str = "VOID_TRACE";
const PANIC_LOCK_ATTEMPTS: u32 = 1000;
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

static TRACER: OnceLock<Tracer> = OnceLock::new();
static LAYER_BUILT: AtomicBool = AtomicBool::new(false);

thread_local! {
    static SYSTEMS: RefCell<Vec<Arc<str>>> = const { RefCell::new(Vec::new()) };
    static ENCODED: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum Direction {
    In,
    Out,
}

#[derive(Serialize)]
struct Line<'a> {
    tick: u64,
    dir: Direction,
    client: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    player: Option<&'a str>,
    packet: &'a str,
    bytes: usize,
    hash: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dropped: Option<&'a str>,
}

/// A line waiting to learn what became of its packet.
pub(crate) struct Pending {
    tick: u64,
    dir: Direction,
    client: u32,
    packet: String,
    bytes: usize,
    hash: u64,
    system: Option<Arc<str>>,
}

struct Sink {
    writer: Box<dyn Write + Send>,
    players: HashMap<u32, String>,
    line: Vec<u8>,
    stopped: bool,
}

impl Sink {
    fn write(&mut self, pending: &Pending, dropped: Option<&str>) {
        if self.stopped {
            return;
        }
        let line = Line {
            tick: pending.tick,
            dir: pending.dir,
            client: pending.client,
            player: self.players.get(&pending.client).map(String::as_str),
            packet: &pending.packet,
            bytes: pending.bytes,
            hash: pending.hash,
            system: pending.system.as_deref(),
            dropped,
        };
        let mut text = std::mem::take(&mut self.line);
        text.clear();
        let written = serde_json::to_writer(&mut text, &line)
            .map_err(std::io::Error::from)
            .and_then(|()| {
                text.push(b'\n');
                self.writer.write_all(&text)
            });
        self.line = text;
        self.check(written);
    }

    fn flush(&mut self) {
        if !self.stopped {
            let flushed = self.writer.flush();
            self.check(flushed);
        }
    }

    fn check(&mut self, result: std::io::Result<()>) {
        if let Err(error) = result {
            self.stopped = true;
            tracing::error!(
                %error,
                "Packet trace stopped: the file ends here and its last line may be cut"
            );
        }
    }
}

struct Tracer {
    sink: Mutex<Sink>,
    tick: AtomicU64,
}

impl Tracer {
    fn new(writer: impl Write + Send + 'static) -> Self {
        Self {
            sink: Mutex::new(Sink {
                writer: Box::new(writer),
                players: HashMap::new(),
                line: Vec::new(),
                stopped: false,
            }),
            tick: AtomicU64::new(0),
        }
    }

    fn prepare(&self, dir: Direction, client: u32, packet: String, body: &[u8]) -> Pending {
        Pending {
            tick: self.tick.load(Ordering::Relaxed),
            dir,
            client,
            packet,
            bytes: body.len(),
            hash: fnv1a(body),
            system: current_system(),
        }
    }

    fn write(&self, pending: &Pending, dropped: Option<&str>) {
        if let Ok(mut sink) = self.sink.lock() {
            sink.write(pending, dropped);
        }
    }

    fn out(&self, client: u32, packet: &ClientboundPacket) -> Pending {
        let name = format!("{}/{}", packet.state_name(), packet.packet_name());
        ENCODED.with_borrow_mut(|encoded| {
            encoded.clear();
            packet.encode(encoded);
            self.prepare(Direction::Out, client, name, encoded)
        })
    }

    fn incoming(&self, client: u32, state: &str, name: Option<&str>, body: &[u8]) {
        let name = format!("{state}/{}", name.unwrap_or("?"));
        self.write(&self.prepare(Direction::In, client, name, body), None);
    }

    fn name_player(&self, client: u32, name: &str) {
        if let Ok(mut sink) = self.sink.lock() {
            sink.players.insert(client, name.to_string());
        }
    }

    fn flush(&self) {
        if let Ok(mut sink) = self.sink.lock() {
            sink.flush();
        }
    }

    /// Never waits for long: the panicking thread may hold the lock itself.
    fn flush_in_panic(&self) {
        for _ in 0..PANIC_LOCK_ATTEMPTS {
            match self.sink.try_lock() {
                Ok(mut sink) => return sink.flush(),
                Err(TryLockError::Poisoned(sink)) => return sink.into_inner().flush(),
                Err(TryLockError::WouldBlock) => std::thread::yield_now(),
            }
        }
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

fn current_system() -> Option<Arc<str>> {
    SYSTEMS.with_borrow(|systems| systems.last().cloned())
}

/// Encodes the packet a second time, the network thread encoding its own copy: `hash` needs
/// the bytes here, where the system is known. Only a running trace pays for it.
pub(crate) fn packet_out(client_id: u32, packet: &ClientboundPacket) -> Option<Pending> {
    TRACER.get().map(|tracer| tracer.out(client_id, packet))
}

pub(crate) fn packet_out_done(pending: Option<Pending>, dropped: Option<&str>) {
    if let (Some(tracer), Some(pending)) = (TRACER.get(), pending) {
        tracer.write(&pending, dropped);
    }
}

/// `name` is `None` for a packet that did not decode.
pub(crate) fn packet_in(client_id: u32, state: &str, name: Option<&str>, body: &[u8]) {
    if let Some(tracer) = TRACER.get() {
        tracer.incoming(client_id, state, name, body);
    }
}

pub(crate) fn player_named(client_id: u32, name: &str) {
    if let Some(tracer) = TRACER.get() {
        tracer.name_player(client_id, name);
    }
}

/// Opens the trace when `VOID_TRACE` names a file. [`crate::VoidServer`] adds it by itself.
pub struct PacketTracePlugin;

impl Plugin for PacketTracePlugin {
    fn build(&self, app: &mut App) {
        let Some(path) = std::env::var_os(PATH_VARIABLE) else {
            return;
        };
        let file = match File::create(&path) {
            Ok(file) => file,
            Err(error) => panic!("{PATH_VARIABLE}: cannot create {path:?}: {error}"),
        };
        if TRACER.set(Tracer::new(BufWriter::new(file))).is_err() {
            tracing::warn!("A packet trace is already open, keeping that one");
        }
        if !LAYER_BUILT.load(Ordering::Relaxed) {
            tracing::warn!(
                "Packet trace without trace_layer() in the subscriber: no line will name a system"
            );
        }
        flush_on_panic();
        pace::count_ticks(app);
        app.add_systems(First, note_tick.after(pace::advance))
            .add_systems(Last, flush_trace);
    }
}

/// The tracer lives in a static, so nothing else would write what a dead tick left in the buffer.
fn flush_on_panic() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(tracer) = TRACER.get() {
            tracer.flush_in_panic();
        }
        previous(info);
    }));
}

/// The lines are written where no world is at hand: the tracer keeps the tick of [`Pace`].
fn note_tick(pace: Res<Pace>) {
    if let Some(tracer) = TRACER.get() {
        tracer.tick.store(pace.ticks_started(), Ordering::Relaxed);
    }
}

fn flush_trace() {
    if let Some(tracer) = TRACER.get() {
        tracer.flush();
    }
}

/// Add this to the subscriber of a binary so that lines name their system. `None` when no
/// trace was asked for, which `tracing_subscriber` takes as no layer at all.
pub fn trace_layer() -> Option<SystemLayer> {
    std::env::var_os(PATH_VARIABLE).map(|_| SystemLayer::new())
}

/// Follows, on each thread, the innermost system running there.
pub struct SystemLayer(());

impl SystemLayer {
    fn new() -> Self {
        LAYER_BUILT.store(true, Ordering::Relaxed);
        Self(())
    }
}

struct SystemName(Arc<str>);

struct NameVisitor(Option<Arc<str>>);

impl field::Visit for NameVisitor {
    fn record_str(&mut self, field: &field::Field, value: &str) {
        if field.name() == "name" {
            self.0 = Some(value.into());
        }
    }

    fn record_debug(&mut self, field: &field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "name" {
            self.0 = Some(format!("{value:?}").trim_matches('"').into());
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for SystemLayer {
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        if !matches!(attributes.metadata().name(), "system" | "system_commands") {
            return;
        }
        let mut name = NameVisitor(None);
        attributes.record(&mut name);
        if let (Some(name), Some(span)) = (name.0, context.span(id)) {
            span.extensions_mut().insert(SystemName(name));
        }
    }

    fn on_enter(&self, id: &Id, context: Context<'_, S>) {
        let Some(span) = context.span(id) else {
            return;
        };
        if let Some(SystemName(name)) = span.extensions().get::<SystemName>() {
            SYSTEMS.with_borrow_mut(|systems| systems.push(name.clone()));
        }
    }

    fn on_exit(&self, id: &Id, context: Context<'_, S>) {
        let Some(span) = context.span(id) else {
            return;
        };
        if span.extensions().get::<SystemName>().is_some() {
            SYSTEMS.with_borrow_mut(|systems| {
                systems.pop();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tracing_subscriber::layer::SubscriberExt;
    use voidmc_protocol::clientbound::KeepAlive;

    use super::*;

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Shared {
        fn lines(&self) -> Vec<Value> {
            let text = self.0.lock().unwrap().clone();
            let text = String::from_utf8(text).unwrap();
            let lines = text.lines().map(|line| serde_json::from_str(line).unwrap());
            lines.collect()
        }
    }

    fn with_layer<T>(run: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::registry().with(SystemLayer::new());
        tracing::subscriber::with_default(subscriber, run)
    }

    #[test]
    fn a_line_holds_the_fixed_format() {
        let out = Shared::default();
        let tracer = Tracer::new(out.clone());
        let packet = ClientboundPacket::from(KeepAlive { keep_alive_id: 1 });
        tracer.write(&tracer.out(3, &packet), None);
        let line = &out.lines()[0];
        assert_eq!(line["dir"], "out");
        assert_eq!(line["client"], 3);
        assert_eq!(line["packet"], "Play/KeepAlive");
        let mut body = Vec::new();
        packet.encode(&mut body);
        assert_eq!(line["bytes"], body.len());
        assert_eq!(line["hash"], json!(fnv1a(&body)));
        assert!(line["tick"].is_u64());
        for absent in ["system", "player", "dropped"] {
            assert!(line.get(absent).is_none(), "{absent}");
        }
    }

    #[test]
    fn an_incoming_packet_is_named_by_its_state_or_marked_undecoded() {
        let out = Shared::default();
        let tracer = Tracer::new(out.clone());
        tracer.incoming(1, "Login", Some("LoginStart"), &[1, 2, 3]);
        tracer.incoming(1, "Play", None, &[9]);
        let lines = out.lines();
        assert_eq!(lines[0]["dir"], "in");
        assert_eq!(lines[0]["packet"], "Login/LoginStart");
        assert_eq!(lines[0]["bytes"], 3);
        assert_eq!(lines[1]["packet"], "Play/?");
    }

    /// Takes `room` bytes, then fails, counting every attempt after that.
    struct FullDisk {
        room: usize,
        attempts_after: Arc<Mutex<usize>>,
    }

    impl Write for FullDisk {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.room == 0 {
                *self.attempts_after.lock().unwrap() += 1;
                return Err(std::io::Error::other("no space left"));
            }
            let taken = self.room.min(bytes.len());
            self.room -= taken;
            Ok(taken)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct CountErrors(Arc<Mutex<usize>>);

    impl<S: Subscriber> Layer<S> for CountErrors {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            if *event.metadata().level() == tracing::Level::ERROR {
                *self.0.lock().unwrap() += 1;
            }
        }
    }

    #[test]
    fn a_failed_write_stops_the_trace_and_says_so_once() {
        let attempts = Arc::new(Mutex::new(0));
        let disk = FullDisk {
            room: 10,
            attempts_after: attempts.clone(),
        };
        let tracer = Tracer::new(disk);
        let errors = Arc::new(Mutex::new(0));
        let counter = CountErrors(errors.clone());
        let subscriber = tracing_subscriber::registry().with(counter);
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..5 {
                tracer.incoming(1, "Play", None, &[1]);
                tracer.flush();
            }
        });
        assert_eq!(
            *attempts.lock().unwrap(),
            1,
            "nothing written after the failure"
        );
        assert_eq!(*errors.lock().unwrap(), 1, "said once");
    }

    #[test]
    fn a_panic_flush_gets_through_a_poisoned_lock() {
        let out = Shared::default();
        let tracer = Arc::new(Tracer::new(BufWriter::new(out.clone())));
        tracer.incoming(1, "Play", None, &[1]);
        let holder = tracer.clone();
        let _ = std::thread::spawn(move || {
            let _sink = holder.sink.lock().unwrap();
            panic!("poisons the lock");
        })
        .join();
        assert!(out.lines().is_empty());
        tracer.flush_in_panic();
        assert_eq!(out.lines().len(), 1);
    }

    #[test]
    fn a_panic_flush_does_not_wait_for_a_lock_this_thread_holds() {
        let tracer = Tracer::new(Vec::new());
        let _sink = tracer.sink.lock().unwrap();
        tracer.flush_in_panic();
    }

    #[test]
    fn fnv1a_follows_the_published_vector() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn a_named_client_stays_named_and_others_do_not_inherit_it() {
        let out = Shared::default();
        let tracer = Tracer::new(out.clone());
        tracer.name_player(4, "sonde");
        tracer.incoming(4, "Play", None, &[]);
        tracer.incoming(5, "Play", None, &[]);
        let lines = out.lines();
        assert_eq!(lines[0]["player"], "sonde");
        assert!(lines[1].get("player").is_none());
    }

    #[test]
    fn a_system_name_ends_with_its_span() {
        with_layer(|| {
            assert_eq!(current_system(), None);
            tracing::info_span!("system", name = "game::send").in_scope(|| {
                assert_eq!(current_system().as_deref(), Some("game::send"));
            });
            assert_eq!(current_system(), None);
        });
    }

    #[test]
    fn a_span_that_is_not_a_system_names_nothing() {
        with_layer(|| {
            tracing::info_span!("schedule", name = "Main").in_scope(|| {
                assert_eq!(current_system(), None);
            });
        });
    }

    #[test]
    fn nothing_is_traced_or_encoded_when_no_trace_is_open() {
        assert!(TRACER.get().is_none());
        assert!(packet_out(1, &KeepAlive { keep_alive_id: 1 }.into()).is_none());
        ENCODED.with_borrow(|encoded| assert!(encoded.is_empty()));
    }
}
