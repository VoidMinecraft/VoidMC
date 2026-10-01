use std::fmt;
use std::sync::{Arc, Mutex, Once};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::{EnvFilter, Registry};

static ACTIVE: Mutex<Option<LogCapture>> = Mutex::new(None);
static INSTALL: Once = Once::new();

/// A warning or error logged anywhere in the process while a server ran.
#[derive(Clone, Debug)]
pub struct LogRecord {
    pub level: Level,
    pub target: String,
    pub message: String,
}

impl LogRecord {
    pub fn is_failure(&self) -> bool {
        self.target.starts_with("voidmc")
            && (self.level == Level::ERROR
                || (self.level == Level::WARN && self.message.starts_with("Unrecognized packet")))
    }
}

impl fmt::Display for LogRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.level, self.target, self.message)
    }
}

#[derive(Clone, Default)]
pub(crate) struct LogCapture(Arc<Mutex<Vec<LogRecord>>>);

impl LogCapture {
    pub(crate) fn activate(&self) -> ActiveCapture {
        INSTALL.call_once(|| {
            let filter = EnvFilter::try_from_env("VOID_E2E_LOG").unwrap_or_else(|_| "off".into());
            let subscriber = Registry::default().with(Forward).with(
                tracing_subscriber::fmt::layer()
                    .with_test_writer()
                    .with_filter(filter),
            );
            if tracing::subscriber::set_global_default(subscriber).is_err() {
                eprintln!(
                    "void-e2e: a global tracing subscriber exists; server logs are not checked"
                );
            }
        });
        *ACTIVE.lock().unwrap() = Some(self.clone());
        ActiveCapture
    }

    pub(crate) fn records(&self) -> Vec<LogRecord> {
        self.0.lock().unwrap().clone()
    }
}

pub(crate) struct ActiveCapture;

impl Drop for ActiveCapture {
    fn drop(&mut self) {
        *ACTIVE.lock().unwrap() = None;
    }
}

struct Forward;

impl<S: Subscriber> Layer<S> for Forward {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let metadata = event.metadata();
        if *metadata.level() > Level::WARN {
            return;
        }
        let Some(capture) = ACTIVE.lock().unwrap().clone() else {
            return;
        };
        let mut message = MessageVisitor(String::new());
        event.record(&mut message);
        capture.0.lock().unwrap().push(LogRecord {
            level: *metadata.level(),
            target: metadata.target().to_string(),
            message: message.0,
        });
    }
}

struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}{}", self.0);
        } else {
            self.0.push_str(&format!(" {}={value:?}", field.name()));
        }
    }
}
