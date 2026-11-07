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
