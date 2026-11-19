//! Unit tests for the example's diagnostics settings (`VOID_METRICS_*`,
//! `VOID_TPS_OUTPUT`, `VOID_FLAME_*`). `tests/diagnostics.rs` runs the built
//! binary with them.

use std::collections::HashMap;
use std::path::PathBuf;

use tracing_subscriber::prelude::*;

use super::{MetricsEnv, flame_layer};

fn read(vars: &[(&str, &str)]) -> MetricsEnv {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    MetricsEnv::from_lookup(|name| vars.get(name).cloned())
}

#[test]
fn nothing_is_enabled_without_variables() {
    let env = read(&[]);
    assert!(!env.metrics_debug);
    assert!(!env.flame_enabled);
    assert!(!env.flame_include_idle);
    assert!(!env.packet_debug);
    assert_eq!(env.tps_output, None);
    assert_eq!(env.flame_output, None);
}

#[test]
fn flags_accept_one_true_and_yes() {
    for on in ["1", "true", "TRUE", "yes", "YES"] {
        assert!(read(&[("VOID_METRICS_DEBUG", on)]).metrics_debug, "{on}");
        assert!(read(&[("VOID_FLAME_INCLUDE_IDLE", on)]).flame_include_idle);
        assert!(read(&[("VOID_PACKET_DEBUG", on)]).packet_debug);
    }
    for off in ["0", "false", "no", "", "on"] {
        assert!(!read(&[("VOID_METRICS_DEBUG", off)]).metrics_debug, "{off}");
    }
}

#[test]
fn output_paths_are_trimmed_and_blank_means_unset() {
    let env = read(&[
        ("VOID_TPS_OUTPUT", "  logs/tps-demo.csv "),
        ("VOID_FLAME_OUTPUT", "   "),
    ]);
    assert_eq!(env.tps_output.as_deref(), Some("logs/tps-demo.csv"));
    assert_eq!(env.flame_output, None);
    assert!(!env.flame_enabled);
}

#[test]
fn tps_output_alone_does_not_enable_tps_metrics() {
    let env = read(&[("VOID_TPS_OUTPUT", "tps.csv")]);
    assert!(!env.metrics_debug);
    assert!(!env.flame_enabled);
}

#[test]
fn flame_mode_or_a_flame_output_enables_flame_and_not_tps() {
    for vars in [
        &[("VOID_METRICS_MODE", "flame")][..],
        &[("VOID_FLAME_OUTPUT", "trace.folded")][..],
    ] {
        let env = read(vars);
        assert!(env.flame_enabled, "{vars:?}");
        assert!(!env.metrics_debug, "{vars:?}");
    }
    assert!(!read(&[("VOID_METRICS_MODE", "tps")]).flame_enabled);
    assert!(!read(&[("VOID_METRICS_DEBUG", "1")]).flame_enabled);
}

/// A directory under the system temp dir, removed when dropped (also when
/// the test panics).
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("voidmc-example-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Records two nested spans three times into a folded trace and returns its
/// stacks (each line without its sample count).
fn traced_stacks(include_idle: bool) -> Vec<String> {
    let dir = TempDir::new(&format!("flame-{include_idle}"));
    let path = dir.0.join("missing/parent/trace.folded");
    let (layer, guard) = flame_layer(&path, include_idle).expect("the trace file opens");
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        for _ in 0..3 {
            let _tick = tracing::info_span!("tick").entered();
            let _work = tracing::info_span!("work").entered();
        }
    });
    drop(guard);

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.ends_with('\n'), "{content:?}");
    content
        .lines()
        .map(|line| {
            let (stack, count) = line
                .rsplit_once(' ')
                .unwrap_or_else(|| panic!("not `stack count`: {line:?}"));
            count
                .parse::<u64>()
                .unwrap_or_else(|_| panic!("bad sample count: {line:?}"));
            stack.to_string()
        })
        .collect()
}

#[test]
fn flame_trace_holds_folded_span_stacks_without_idle_time() {
    let stacks = traced_stacks(false);
    assert!(
        stacks
            .iter()
            .any(|stack| stack == "all-threads; tick; work"),
        "{stacks:?}"
    );
    assert!(
        stacks.iter().all(|stack| stack != "all-threads"),
        "idle samples are hidden by default: {stacks:?}"
    );
}

#[test]
fn include_idle_adds_samples_outside_any_span() {
    let stacks = traced_stacks(true);
    assert!(
        stacks
            .iter()
            .any(|stack| stack == "all-threads; tick; work"),
        "{stacks:?}"
    );
    assert!(
        stacks.iter().any(|stack| stack == "all-threads"),
        "idle samples are included: {stacks:?}"
    );
}

#[test]
fn an_unusable_flame_path_is_an_error_not_a_panic() {
    let dir = TempDir::new("flame-unusable");
    let file = dir.0.join("file");
    std::fs::write(&file, "x").unwrap();
    for path in [dir.0.clone(), file.join("trace.folded")] {
        let result = flame_layer::<tracing_subscriber::Registry>(&path, false);
        assert!(result.is_err(), "{}", path.display());
    }
}
