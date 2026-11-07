use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::condition::{self, DEFAULT_WAIT_MS};
use crate::registry;

/// Sends one verb to a client and returns its JSON line.
pub fn ask(name: &str, verb: &str, timeout: Duration) -> Result<String, String> {
    let mut stream = UnixStream::connect(registry::socket(name)?)
        .map_err(|e| format!("client {name} does not answer: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    writeln!(stream, "{verb}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| format!("{verb}: {e}"))?;
    if line.is_empty() {
        return Err(format!("client {name} closed the connection"));
    }
    Ok(line.trim_end().to_string())
}

/// How long a verb may take: a `wait_until` carries its own limit.
pub fn timeout_of(verb: &str) -> Duration {
    let limit = match verb.trim().split_once(' ') {
        Some(("wait_until", rest)) => condition::wait(rest).map_or(DEFAULT_WAIT_MS, |(_, ms)| ms),
        _ => return Duration::from_secs(30),
    };
    Duration::from_millis(limit + 5_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wait_outlives_its_own_limit() {
        assert_eq!(
            timeout_of("wait_until 60000 chunks>=1"),
            Duration::from_secs(65)
        );
        assert_eq!(timeout_of("wait_until chunks>=1"), Duration::from_secs(25));
        assert_eq!(
            timeout_of("wait_until chunks >= 100"),
            Duration::from_secs(25)
        );
        assert_eq!(timeout_of("state"), Duration::from_secs(30));
    }
}
