//! What the proxy says happens, one JSON object per line.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, channel, sync_channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::session::Event;

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

    pub(super) fn emit(&self, name: &str, mut fields: Value) {
        let at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        fields["event"] = json!(name);
        fields["at_ms"] = json!(at_ms);
        (self.sink)(fields);
    }

    pub(super) fn emit_session(&self, session: u64, event: Event) {
        match event {
            Event::Joined { name } => {
                self.emit("client_joined", json!({ "session": session, "name": name }));
            }
            Event::BackendLost => self.emit("backend_lost", json!({ "session": session })),
            Event::Resumed { outage } => self.emit(
                "resumed",
                json!({ "session": session, "outage_ms": outage.as_millis() as u64 }),
            ),
            Event::GaveUp { reason } => {
                self.emit("gave_up", json!({ "session": session, "reason": reason }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

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

    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<u8>>>);

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
}
