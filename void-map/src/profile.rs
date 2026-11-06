//! Where the time of a tick goes, system by system. Only with the `profile` feature: it
//! reads the spans `bevy_ecs` opens around each system when built with `trace`.
//!
//! `VOID_MAP_SLOW_TICK` sets, in milliseconds, what counts as a slow update (25 by default).
//! Each one logs its largest systems at `info`, and a summary follows every 200 updates.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tracing::span::{Attributes, Id};
use tracing::{Subscriber, field};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

const SUMMARY_UPDATES: u32 = 200;
const SHOWN: usize = 6;

/// The name of a span, as `bevy_ecs` gives it in a field.
struct Named(String);

/// When a span was last entered.
struct Entered(Instant);

#[derive(Default)]
struct Tally {
    update: HashMap<String, Duration>,
    stretch: HashMap<String, (Duration, Duration)>,
    updates: u32,
    last_end: Option<Instant>,
    longest_gap: Duration,
}

/// Times the systems of each update of the `Main` schedule.
pub struct Profile {
    tally: Mutex<Tally>,
    slow: Duration,
}

impl Profile {
    pub fn new() -> Self {
        let millis = std::env::var("VOID_MAP_SLOW_TICK")
            .ok()
            .and_then(|millis| millis.parse().ok())
            .unwrap_or(25);
        Self {
            tally: Mutex::default(),
            slow: Duration::from_millis(millis),
        }
    }
}

impl Default for Profile {
    fn default() -> Self {
        Self::new()
    }
}

struct NameVisitor(Option<String>);

impl field::Visit for NameVisitor {
    fn record_debug(&mut self, field: &field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "name" {
            self.0 = Some(format!("{value:?}").trim_matches('"').to_string());
        }
    }

    fn record_str(&mut self, field: &field::Field, value: &str) {
        if field.name() == "name" {
            self.0 = Some(value.to_string());
        }
    }
}

fn largest(of: impl Iterator<Item = (String, Duration)>) -> String {
    let mut all: Vec<_> = of.collect();
    all.sort_by_key(|(_, took)| std::cmp::Reverse(*took));
    let shown = all.iter().take(SHOWN);
    let line = shown.map(|(name, took)| format!("{}={:.1}", short(name), took.as_secs_f64() * 1e3));
    line.collect::<Vec<_>>().join(" ")
}

/// The last part of a system's path, with what tells it from others of that name.
fn short(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Profile {
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let mut name = NameVisitor(None);
        attributes.record(&mut name);
        if let (Some(name), Some(span)) = (name.0, context.span(id)) {
            span.extensions_mut().insert(Named(name));
        }
    }

    fn on_enter(&self, id: &Id, context: Context<'_, S>) {
        if let Some(span) = context.span(id) {
            span.extensions_mut().replace(Entered(Instant::now()));
        }
    }

    fn on_exit(&self, id: &Id, context: Context<'_, S>) {
        let Some(span) = context.span(id) else {
            return;
        };
        let extensions = span.extensions();
        let (Some(Named(name)), Some(Entered(at))) =
            (extensions.get::<Named>(), extensions.get::<Entered>())
        else {
            return;
        };
        let (took, now) = (at.elapsed(), Instant::now());
        let Ok(mut tally) = self.tally.lock() else {
            return;
        };
        let tally = &mut *tally;
        match span.metadata().name() {
            "system" | "system_commands" | "observer" | "observer_trigger" => {
                *tally.update.entry(name.clone()).or_default() += took;
            }
            "schedule" if short(name) == "Main" => {
                if took >= self.slow {
                    tracing::info!(
                        took_ms = took.as_millis() as u64,
                        systems = largest(tally.update.iter().map(|(n, t)| (n.clone(), *t))),
                        "slow update"
                    );
                }
                for (name, spent) in tally.update.drain() {
                    let (total, most) = tally.stretch.entry(name).or_default();
                    *total += spent;
                    *most = (*most).max(spent);
                }
                if let Some(end) = tally.last_end.replace(now) {
                    tally.longest_gap = tally.longest_gap.max(now - end);
                }
                tally.updates += 1;
                if tally.updates >= SUMMARY_UPDATES {
                    let totals = tally.stretch.iter().map(|(n, (t, _))| (n.clone(), *t));
                    let mosts = tally.stretch.iter().map(|(n, (_, m))| (n.clone(), *m));
                    tracing::info!(
                        longest_gap_ms = tally.longest_gap.as_millis() as u64,
                        total = largest(totals),
                        most = largest(mosts),
                        "update summary"
                    );
                    tally.stretch.clear();
                    tally.updates = 0;
                    tally.longest_gap = Duration::ZERO;
                }
            }
            _ => {}
        }
    }
}
