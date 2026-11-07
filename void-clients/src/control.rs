use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::Value;

use crate::condition::{self, DEFAULT_WAIT_MS};
use crate::{perf, registry, version};

const STATE_LIMIT: Duration = Duration::from_secs(5);

/// What every probe answers alike, whatever its version: a stale client stays visible and stoppable.
#[derive(Clone, Copy)]
pub enum Chore {
    State,
    Thumb,
    ShowFocus,
    Hide,
    Quit,
}

impl Chore {
    fn verb(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Thumb => "thumb",
            Self::ShowFocus => "show focus",
            Self::Hide => "hide",
            Self::Quit => "quit",
        }
    }
}

/// A connection to the probe of a running client, through which every verb passes.
pub struct Probe {
    name: String,
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

fn one_verb(verb: &str) -> Result<(), String> {
    if verb.contains(['\n', '\r']) {
        return Err(format!("one verb at a time: {verb:?} holds a line break"));
    }
    Ok(())
}

impl Probe {
    fn connect(name: &str) -> Result<Self, String> {
        let stream = UnixStream::connect(registry::socket(name)?)
            .map_err(|e| format!("client {name} does not answer: {e}"))?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        Ok(Self {
            name: name.to_string(),
            stream,
            reader,
        })
    }

    /// The only way to drive a client: a probe of another version than this voidc is refused.
    pub fn open(name: &str) -> Result<Self, String> {
        let mut probe = Self::connect(name)?;
        let state = probe.ask("state", STATE_LIMIT)?;
        let state: Value = serde_json::from_str(&state).map_err(|e| e.to_string())?;
        version::check(name, &state)?;
        Ok(probe)
    }

    /// Sends one verb and returns its JSON line.
    pub fn ask(&mut self, verb: &str, timeout: Duration) -> Result<String, String> {
        one_verb(verb)?;
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        writeln!(self.stream, "{verb}").map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|e| format!("{verb}: {e}"))?;
        if line.is_empty() {
            return Err(format!("client {} closed the connection", self.name));
        }
        Ok(line.trim_end().to_string())
    }

    pub fn into_stream(self) -> UnixStream {
        self.stream
    }
}

/// Sends one verb to a client whose probe is of this version, and returns its JSON line.
pub fn ask(name: &str, verb: &str, timeout: Duration) -> Result<String, String> {
    one_verb(verb)?;
    Probe::open(name)?.ask(verb, timeout)
}

/// A chore needs no check of the version.
pub fn chore(name: &str, chore: Chore, timeout: Duration) -> Result<String, String> {
    Probe::connect(name)?.ask(chore.verb(), timeout)
}

/// How long a verb may take: a `wait_until` carries its own limit.
pub fn timeout_of(verb: &str) -> Duration {
    let words: Vec<&str> = verb.split_whitespace().collect();
    if words.first() == Some(&"perf") {
        return perf::timeout_of(&words);
    }
    let limit = match condition::split(verb) {
        ("wait_until", rest) => condition::wait(rest).map_or(DEFAULT_WAIT_MS, |(_, ms)| ms),
        _ => return Duration::from_secs(30),
    };
    Duration::from_millis(limit + 5_000)
}

#[cfg(test)]
pub mod stale {
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;
    use crate::registry::{Client, Level};

    pub const NAME: &str = "stale";

    /// A running client whose probe answers every verb, from another version than this voidc.
    pub struct Stale {
        home: PathBuf,
        heard: Arc<Mutex<Vec<String>>>,
        process: Child,
    }

    fn serve(stream: UnixStream, heard: &Mutex<Vec<String>>) {
        let Ok(mut out) = stream.try_clone() else {
            return;
        };
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            heard.lock().unwrap().push(line);
            let reply = r#"{"ok":true,"state":"connected","chunks":1000,"clock":"real","version":"0000000000000000"}"#;
            if writeln!(out, "{reply}").is_err() {
                return;
            }
        }
    }

    impl Stale {
        pub fn start(level: Level) -> Self {
            static RUN: AtomicU32 = AtomicU32::new(0);
            let home = std::env::temp_dir().join(format!(
                "vs{}-{}",
                std::process::id(),
                RUN.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&home);
            registry::set_test_home(Some(home.clone()));
            let socket = registry::socket(NAME).unwrap();
            fs::create_dir_all(socket.parent().unwrap()).unwrap();
            let listener = UnixListener::bind(&socket).unwrap();
            let heard = Arc::new(Mutex::new(Vec::new()));
            let shared = heard.clone();
            thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let heard = shared.clone();
                    thread::spawn(move || serve(stream, &heard));
                }
            });
            let process = Command::new("sh")
                .args(["-c", "sleep 60; :", &format!("-Dvoid.client={NAME}"), "x"])
                .spawn()
                .unwrap();
            registry::save(&Client {
                name: NAME.to_string(),
                feature: "Stale".to_string(),
                port: 1,
                level,
                pid: process.id(),
                fps: None,
                size: None,
                jfr: false,
            })
            .unwrap();
            Self {
                home,
                heard,
                process,
            }
        }

        /// What the probe was sent: a refused command sends nothing past its check.
        pub fn heard(&self) -> Vec<String> {
            self.heard.lock().unwrap().clone()
        }

        pub fn assert_refused(&self, result: Result<(), String>) {
            let error = result.unwrap_err();
            assert!(!self.heard().is_empty(), "the probe was never asked");
            assert!(
                error.contains("runs probe version 0000000000000000"),
                "{error}"
            );
            assert!(
                self.heard().iter().all(|verb| verb == "state"),
                "{:?}",
                self.heard()
            );
        }
    }

    impl Drop for Stale {
        fn drop(&mut self) {
            let _ = self.process.kill();
            let _ = self.process.wait();
            registry::set_test_home(None);
            let _ = fs::remove_dir_all(&self.home);
        }
    }
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
        assert_eq!(
            timeout_of("wait_until\t40000\tchunks>=1"),
            Duration::from_secs(45)
        );
        assert_eq!(timeout_of("state"), Duration::from_secs(30));
    }

    #[test]
    fn a_line_break_cannot_slip_a_second_verb_in() {
        let error = ask("nobody", "assert chat~ok\nquit", Duration::from_secs(1)).unwrap_err();
        assert!(error.contains("line break"), "{error}");
    }
}
