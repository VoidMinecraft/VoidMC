use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::Value;

use crate::condition::{self, DEFAULT_WAIT_MS};
use crate::{perf, registry, version};

/// Above the 10 s the probe gives the game thread: a stuck game answers with a failure, in time.
const STATE_LIMIT: Duration = Duration::from_secs(12);

/// What goes to a probe of any version, unchecked: a stale client stays visible and stoppable, and
/// gets its clock back if it knows the verb.
#[derive(Clone, Copy)]
pub enum Chore {
    State,
    Thumb,
    ShowFocus,
    Hide,
    Quit,
    ClockReal,
}

impl Chore {
    fn verb(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Thumb => "thumb",
            Self::ShowFocus => "show focus",
            Self::Hide => "hide",
            Self::Quit => "quit",
            Self::ClockReal => "clock real",
        }
    }
}

/// A connection to the probe of a running client, through which every verb passes.
pub struct Probe {
    name: String,
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

fn no_answer(name: &str, verb: &str, timeout: Duration, error: &std::io::Error) -> String {
    match error.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => {
            format!("client {name} does not answer: no reply to {verb} within {timeout:?}")
        }
        _ => format!("client {name}, on {verb}: {error}"),
    }
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
    pub fn open(name: &str, limits: Limits) -> Result<Self, String> {
        let mut probe = Self::connect(name)?;
        let state = probe.ask("state", limits.open)?;
        let state: Value = serde_json::from_str(&state).map_err(|e| e.to_string())?;
        version::check(name, &state)?;
        Ok(probe)
    }

    /// Sends one verb and returns its JSON line. After an error the connection may still get the
    /// late answer, which would read as the answer to the next verb: drop it.
    pub fn ask(&mut self, verb: &str, timeout: Duration) -> Result<String, String> {
        one_verb(verb)?;
        let closed = |e| format!("client {} closed the connection: {e}", self.name);
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(closed)?;
        writeln!(self.stream, "{verb}").map_err(closed)?;
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|e| no_answer(&self.name, verb, timeout, &e))?;
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
    Probe::open(name, Limits::REAL)?.ask(verb, timeout)
}

/// A chore needs no check of the version.
pub fn chore(name: &str, chore: Chore, timeout: Duration) -> Result<String, String> {
    Probe::connect(name)?.ask(chore.verb(), timeout)
}

/// How long a client gets. A test takes a smaller set, so that it does not wait for the real one.
#[derive(Clone, Copy)]
pub struct Limits {
    pub open: Duration,
    verb: Duration,
    past_a_wait: Duration,
}

impl Limits {
    pub const REAL: Self = Self {
        open: STATE_LIMIT,
        verb: Duration::from_secs(30),
        past_a_wait: Duration::from_secs(5),
    };

    /// The real ones divided by 20.
    #[cfg(test)]
    pub const TEST: Self = Self {
        open: Duration::from_millis(600),
        verb: Duration::from_millis(1500),
        past_a_wait: Duration::from_millis(250),
    };

    /// How long a verb may take: a `wait_until` carries its own limit.
    pub fn of(self, verb: &str) -> Duration {
        let words: Vec<&str> = verb.split_whitespace().collect();
        if words.first() == Some(&"perf") {
            return perf::timeout_of(&words);
        }
        let limit = match condition::split(verb) {
            ("wait_until", rest) => condition::wait(rest).map_or(DEFAULT_WAIT_MS, |(_, ms)| ms),
            _ => return self.verb,
        };
        Duration::from_millis(limit) + self.past_a_wait
    }
}

pub fn timeout_of(verb: &str) -> Duration {
    Limits::REAL.of(verb)
}

#[cfg(test)]
pub mod fake {
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use serde_json::json;

    use super::*;
    use crate::held::Held;
    use crate::registry::{Client, Level};

    pub const NAME: &str = "fake";

    /// What the probe sends back to a verb, and how long it takes to.
    pub struct Reply {
        pub after: Duration,
        pub line: String,
    }

    impl Reply {
        /// Stamped with the version of this voidc.
        pub fn current(after: Duration, mut fields: Value) -> Self {
            fields["version"] = json!(version::current());
            let line = fields.to_string();
            Self { after, line }
        }
    }

    type Script = Box<dyn Fn(&str) -> Reply + Send + Sync>;

    /// A running client whose probe answers from a script, with a real socket and a real pid.
    pub struct FakeClient {
        home: PathBuf,
        heard: Arc<Mutex<Vec<String>>>,
        process: Arc<Mutex<Held>>,
    }

    struct Serving {
        heard: Arc<Mutex<Vec<String>>>,
        process: Arc<Mutex<Held>>,
        script: Arc<Script>,
    }

    fn serve(stream: UnixStream, serving: &Serving) {
        let Ok(mut out) = stream.try_clone() else {
            return;
        };
        for verb in BufReader::new(stream).lines().map_while(Result::ok) {
            serving.heard.lock().unwrap().push(verb.clone());
            let reply = (serving.script)(&verb);
            thread::sleep(reply.after);
            if writeln!(out, "{}", reply.line).is_err() {
                return;
            }
            if verb == "quit" {
                serving.process.lock().unwrap().stop();
            }
        }
    }

    fn short_home_of(pid: u32, run: u32) -> PathBuf {
        Path::new("/tmp").join(format!("voidc-fake-{pid}-{run}"))
    }

    /// The one way a test gets a socket, under a home of its own for this thread. In `/tmp` and
    /// not `temp_dir`, and with two numbers for all that varies: a socket has 103 bytes, whatever
    /// `TMPDIR` holds, and neither a test nor another process shares it.
    pub fn test_socket(name: &str) -> (PathBuf, PathBuf) {
        static RUN: AtomicU32 = AtomicU32::new(0);
        let home = short_home_of(std::process::id(), RUN.fetch_add(1, Ordering::Relaxed));
        let _ = fs::remove_dir_all(&home);
        registry::set_test_home(Some(home.clone()));
        let socket = registry::socket(name).unwrap();
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        (home, socket)
    }

    impl FakeClient {
        /// A client whose probe answers every verb, from another version than this voidc.
        pub fn stale(level: Level) -> Self {
            Self::scripted(
                level,
                Box::new(|_| {
                    Reply {
                    after: Duration::ZERO,
                    line: r#"{"ok":true,"state":"connected","chunks":1000,"clock":"real","version":"0000000000000000"}"#.to_string(),
                }
                }),
            )
        }

        pub fn scripted(level: Level, script: Script) -> Self {
            let (home, socket) = test_socket(NAME);
            let listener = UnixListener::bind(&socket).unwrap();
            let process = Held::waiting(&format!("fake-client -Dvoid.client={NAME} x"));
            registry::save(&Client {
                name: NAME.to_string(),
                feature: "Fake".to_string(),
                port: 1,
                level,
                pid: process.id(),
                fps: None,
                size: None,
                jfr: false,
            })
            .unwrap();
            let heard = Arc::new(Mutex::new(Vec::new()));
            let process = Arc::new(Mutex::new(process));
            let (shared_heard, shared_process) = (heard.clone(), process.clone());
            let script = Arc::new(script);
            thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let serving = Serving {
                        heard: shared_heard.clone(),
                        process: shared_process.clone(),
                        script: script.clone(),
                    };
                    thread::spawn(move || serve(stream, &serving));
                }
            });
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

    #[test]
    fn a_test_socket_fits_whatever_the_numbers_and_is_never_shared() {
        let longest = short_home_of(u32::MAX, u32::MAX);
        registry::set_test_home(Some(longest));
        assert!(registry::socket(NAME).is_ok());
        let (first, _) = test_socket(NAME);
        let (second, _) = test_socket(NAME);
        registry::set_test_home(None);
        assert_ne!(first, second);
        assert!(first.starts_with("/tmp"));
        let mine = format!("voidc-fake-{}-", std::process::id());
        assert!(first.to_string_lossy().contains(&mine));
        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }

    impl Drop for FakeClient {
        fn drop(&mut self) {
            self.process.lock().unwrap().stop();
            registry::set_test_home(None);
            let _ = fs::remove_dir_all(&self.home);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

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
    fn a_state_outlives_what_the_probe_gives_the_game_thread() {
        assert!(include_str!("../probe/Probe.java").contains("onGame(task, 10_000)"));
        assert!(Limits::REAL.open > Duration::from_secs(10));
    }

    #[test]
    fn the_clock_given_back_has_thirty_seconds_for_real() {
        assert_eq!(Limits::REAL.of("clock real"), Duration::from_secs(30));
    }

    #[test]
    fn the_limits_of_the_tests_are_the_real_ones_divided_by_twenty() {
        assert_eq!(Limits::TEST.open * 20, Limits::REAL.open);
        assert_eq!(Limits::TEST.of("state") * 20, Limits::REAL.of("state"));
        assert_eq!(Limits::TEST.past_a_wait * 20, Limits::REAL.past_a_wait);
    }

    #[test]
    fn a_connection_the_client_hung_up_says_so() {
        use std::os::unix::net::UnixListener;
        let (home, socket) = fake::test_socket("gone");
        let listener = UnixListener::bind(socket).unwrap();
        let mut probe = Probe::connect("gone").unwrap();
        let (hung_up, _) = listener.accept().unwrap();
        let held_by_a_child = hung_up.try_clone().unwrap();
        hung_up.shutdown(std::net::Shutdown::Both).unwrap();
        let error = probe.ask("act map", Duration::from_secs(1)).unwrap_err();
        drop(held_by_a_child);
        registry::set_test_home(None);
        fs::remove_dir_all(home).unwrap();
        assert!(
            error.contains("client gone closed the connection"),
            "{error}"
        );
    }

    #[test]
    fn a_fake_client_leaves_nothing_behind() {
        let fake = fake::FakeClient::stale(registry::Level::Render);
        let client = registry::load(fake::NAME).unwrap();
        assert!(registry::alive(&client));
        drop(fake);
        assert!(!registry::alive(&client));
    }

    #[test]
    fn a_long_tmpdir_leaves_the_fake_client_standing() {
        let run = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::cmd_refuses_a_probe_of_another_version"])
            .env("TMPDIR", std::env::temp_dir().join("t".repeat(120)))
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&run.stdout);
        assert!(run.status.success() && out.contains("1 passed"), "{out}");
    }

    #[test]
    fn a_line_break_cannot_slip_a_second_verb_in() {
        let error = ask("nobody", "assert chat~ok\nquit", Duration::from_secs(1)).unwrap_err();
        assert!(error.contains("line break"), "{error}");
    }
}
