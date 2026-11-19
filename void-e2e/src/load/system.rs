//! Resource readings from Linux procfs. Each returns `None` elsewhere, and
//! the report then lists the reading as unavailable.

/// CPU time the calling thread has run, in milliseconds.
#[cfg(test)]
pub(crate) fn thread_cpu_ms() -> Option<f64> {
    let schedstat = std::fs::read_to_string("/proc/thread-self/schedstat").ok()?;
    let nanos: f64 = schedstat.split_whitespace().next()?.parse().ok()?;
    Some(nanos / 1e6)
}

/// Thread names (as the kernel truncates them, to 15 bytes) of an
/// in-process server: its game thread and network driver, which inherit the
/// harness's `void-e2e-server` name, its Tokio network runtime, and Bevy's
/// task pools, where the multi-threaded executor runs most systems. The load
/// clients never run on threads with these names: in a test they run on the
/// test thread, and the `voidmc-load` binary names its own runtime threads
/// `load-client`.
pub(crate) const SERVER_THREADS: [&str; 5] = [
    "void-e2e-server",
    "tokio-runtime-w",
    "Compute Task Po",
    "Async Compute T",
    "IO Task Pool",
];

/// Total CPU time of this process's live threads whose name starts with one
/// of `prefixes`, in milliseconds.
#[cfg(test)]
pub(crate) fn threads_cpu_ms(prefixes: &[&str]) -> Option<f64> {
    let mut nanos = 0.0;
    for task in std::fs::read_dir("/proc/self/task").ok()?.flatten() {
        let path = task.path();
        let Ok(name) = std::fs::read_to_string(path.join("comm")) else {
            continue;
        };
        if !prefixes.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        // A thread may exit between the two reads.
        let Ok(schedstat) = std::fs::read_to_string(path.join("schedstat")) else {
            continue;
        };
        nanos += schedstat
            .split_whitespace()
            .next()
            .and_then(|field| field.parse::<f64>().ok())
            .unwrap_or(0.0);
    }
    Some(nanos / 1e6)
}

/// The calling thread's CPU time and the total of this process's live
/// threads whose name starts with one of `prefixes`, in nanoseconds, from a
/// single reading of each thread. When the calling thread is one of those
/// threads, the total includes exactly the time returned for it. Integers, so
/// that the difference between two totals is never less than the calling
/// thread's own difference by rounding: [`ms_between`] turns them into the
/// milliseconds of a window.
pub(crate) fn thread_and_threads_cpu_ns(prefixes: &[&str]) -> Option<(u64, u64)> {
    let own = std::fs::read_link("/proc/thread-self").ok()?;
    let own = own.file_name()?;
    let mut own_nanos = None;
    let mut nanos = 0;
    for task in std::fs::read_dir("/proc/self/task").ok()?.flatten() {
        let path = task.path();
        let is_own = task.file_name() == own;
        let named = std::fs::read_to_string(path.join("comm"))
            .is_ok_and(|name| prefixes.iter().any(|prefix| name.starts_with(prefix)));
        if !is_own && !named {
            continue;
        }
        // A thread may exit between the two reads.
        let Some(cpu) = std::fs::read_to_string(path.join("schedstat"))
            .ok()
            .and_then(|schedstat| schedstat.split_whitespace().next()?.parse::<u64>().ok())
        else {
            continue;
        };
        if is_own {
            own_nanos = Some(cpu);
        }
        if named {
            nanos += cpu;
        }
    }
    Some((own_nanos?, nanos))
}

/// Milliseconds from the `first` to the `last` of two nanosecond readings.
/// Exact for equal differences: both readings stay far below 2^53.
pub(crate) fn ms_between(first: u64, last: u64) -> f64 {
    (last as f64 - first as f64) / 1e6
}

/// User plus system CPU time of the whole process, in milliseconds.
pub(crate) fn process_cpu_ms() -> Option<f64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // Fields after the command name, which is parenthesised and may contain
    // spaces: state is field 3, utime 14 and stime 15.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let utime: f64 = fields.get(11)?.parse().ok()?;
    let stime: f64 = fields.get(12)?.parse().ok()?;
    // procfs reports clock ticks in USER_HZ, which Linux fixes at 100.
    Some((utime + stime) * 10.0)
}

/// Resident set size of the whole process, in KiB.
pub(crate) fn process_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn procfs_readings_are_available_on_linux() {
        let before = thread_cpu_ms().unwrap();
        let mut x = 0u64;
        for i in 0..2_000_000u64 {
            x = x.wrapping_add(std::hint::black_box(i));
        }
        std::hint::black_box(x);
        assert!(thread_cpu_ms().unwrap() > before);
        assert!(process_cpu_ms().is_some());
        let own = std::fs::read_to_string("/proc/thread-self/comm").unwrap();
        assert!(threads_cpu_ms(&[own.trim_end()]).unwrap() > 0.0);
        assert_eq!(threads_cpu_ms(&["no thread has this name"]), Some(0.0));
        assert!(process_rss_kib().unwrap() > 0);
    }

    #[test]
    fn one_pass_counts_the_calling_thread_in_the_total() {
        let own = std::fs::read_to_string("/proc/thread-self/comm").unwrap();
        let (own_ns, total_ns) = thread_and_threads_cpu_ns(&[own.trim_end()]).unwrap();
        assert!(total_ns >= own_ns, "{total_ns} < {own_ns}");
        let (later_own_ns, total_ns) =
            thread_and_threads_cpu_ns(&["no thread has this name"]).unwrap();
        assert!(later_own_ns >= own_ns);
        assert_eq!(total_ns, 0);
        assert_eq!(ms_between(1_000_000, 3_500_000), 2.5);
    }
}
