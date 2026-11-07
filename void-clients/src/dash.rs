use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::control::{self, Chore};
use crate::registry::{self, Client, Level, Registry, home_name};
use crate::{lifecycle, version};

const PAGE: &str = include_str!("dash.html");
const ASK_TIMEOUT: Duration = Duration::from_secs(3);
const HEAD_LIMIT: u64 = 16 * 1024;
const READ_LIMIT: Duration = Duration::from_secs(5);
const QUIT_LIMIT: Duration = Duration::from_secs(5);

struct Head {
    method: String,
    target: String,
    host: Option<String>,
    origin: Option<String>,
}

struct Deadline {
    stream: TcpStream,
    until: Instant,
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

fn head(stream: &TcpStream) -> std::io::Result<Head> {
    head_within(stream, READ_LIMIT)
}

fn head_within(stream: &TcpStream, limit: Duration) -> std::io::Result<Head> {
    let whole = Deadline {
        stream: stream.try_clone()?,
        until: Instant::now() + limit,
    };
    let mut lines = BufReader::new(whole.take(HEAD_LIMIT)).lines();
    let first = lines.next().transpose()?.unwrap_or_default();
    let mut words = first.split_whitespace();
    let mut head = Head {
        method: words.next().unwrap_or("").to_string(),
        target: words.next().unwrap_or("").to_string(),
        host: None,
        origin: None,
    };
    for line in lines {
        let line = line?;
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let value = Some(value.trim().to_string());
            match name.trim().to_ascii_lowercase().as_str() {
                "host" => head.host = value,
                "origin" => head.origin = value,
                _ => {}
            }
        }
    }
    Ok(head)
}

/// Only this page may act: another site, or a name rebound to 127.0.0.1, is refused.
fn trusted(head: &Head, port: u16) -> bool {
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let host_ok = head.host.as_ref().is_some_and(|host| hosts.contains(host));
    let origin_ok = match &head.origin {
        Some(origin) => hosts.iter().any(|host| *origin == format!("http://{host}")),
        None => head.method != "POST",
    };
    host_ok && origin_ok
}

fn call(port: u16, method: &str, path: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(ASK_TIMEOUT)).ok()?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;
    let code = answer.split_whitespace().nth(1)?.parse().ok()?;
    let body = answer.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    Some((code, body.to_string()))
}

#[derive(Debug, PartialEq)]
enum Served {
    Nothing,
    Ours,
    Other,
    Older,
    Foreign,
}

fn served(port: u16) -> Served {
    if TcpStream::connect(("127.0.0.1", port)).is_err() {
        return Served::Nothing;
    }
    let Some((200, version)) = call(port, "GET", "/api/version") else {
        return Served::Older;
    };
    match call(port, "GET", "/api/home") {
        Some((200, home)) if home != home_name() => Served::Foreign,
        _ if version == version::current() => Served::Ours,
        _ => Served::Other,
    }
}

/// Starts the page if nobody serves it yet, and says where it is.
pub fn ensure(registry: &Registry) -> Result<String, String> {
    ensure_on(registry::mosaic_port(registry)?)
}

fn ensure_on(port: u16) -> Result<String, String> {
    ensure_within(port, QUIT_LIMIT)
}

fn ensure_within(port: u16, quit_limit: Duration) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{port}");
    match served(port) {
        Served::Ours => return Ok(url),
        Served::Older => {
            eprintln!(
                "the mosaic on {port} comes from an older voidc and starts clients this one refuses: stop it, and voidc starts its own"
            );
            return Ok(url);
        }
        Served::Foreign => {
            return Err(format!(
                "port {port} serves the mosaic of another VOIDC_HOME, left alone: stop that mosaic, then run this again from {}",
                home_name()
            ));
        }
        Served::Other => {
            let _ = call(port, "POST", "/api/quit");
            let deadline = Instant::now() + quit_limit;
            let mut now = served(port);
            while !matches!(now, Served::Nothing | Served::Ours) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
                now = served(port);
            }
            if now == Served::Ours {
                return Ok(url);
            }
            if now != Served::Nothing {
                return Err(format!(
                    "the mosaic on {port} comes from another voidc and did not quit: stop it, then run this again"
                ));
            }
        }
        Served::Nothing => {}
    }
    use std::os::unix::process::CommandExt;
    fs::create_dir_all(registry::home()).map_err(|e| e.to_string())?;
    let log = fs::File::create(registry::home().join("dash.log")).map_err(|e| e.to_string())?;
    let error = log.try_clone().map_err(|e| e.to_string())?;
    Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("dash-run")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(error)
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(url)
}

fn status(client: &Client) -> Value {
    let running = registry::alive(client);
    let mut tile = json!({
        "name": client.name,
        "feature": client.feature,
        "level": client.level.name(),
        "port": client.port,
        "running": running,
        "state": if running { "starting" } else { "stopped" },
    });
    if !running {
        return tile;
    }
    if client.level == Level::Play {
        tile["state"] = json!("connected");
        return tile;
    }
    let reply = control::chore(&client.name, Chore::State, ASK_TIMEOUT).ok();
    if let Some(Value::Object(fields)) = reply.and_then(|line| serde_json::from_str(&line).ok()) {
        for (key, value) in fields {
            if key != "level" {
                tile[key] = value;
            }
        }
    }
    tile
}

/// Ends with a shutdown, not with the drop: a child forked meanwhile may hold this socket, and the
/// peer would wait for an end that a close does not send.
fn respond(stream: &mut TcpStream, code: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}

fn thumb(name: &str) -> Option<Vec<u8>> {
    let line = control::chore(name, Chore::Thumb, ASK_TIMEOUT).ok()?;
    let reply: Value = serde_json::from_str(&line).ok()?;
    fs::read(reply["path"].as_str()?).ok()
}

fn action(registry: &Registry, name: &str, verb: &str) -> Result<(), String> {
    match verb {
        "stop" => lifecycle::down(name),
        "start" => {
            let client = registry::load(name).ok_or("unknown client")?;
            lifecycle::start(registry, client)
        }
        "take" => control::chore(name, Chore::ShowFocus, ASK_TIMEOUT).map(|_| ()),
        "give" => control::chore(name, Chore::Hide, ASK_TIMEOUT).map(|_| ()),
        other => Err(format!("unknown action {other}")),
    }
}

fn request(registry: &Registry, mut stream: TcpStream) -> std::io::Result<()> {
    let head = head(&stream)?;
    if !trusted(&head, registry.dashboard_port) {
        return respond(&mut stream, "403 Forbidden", "text/plain", b"");
    }
    let (method, target) = (head.method.as_str(), head.target.as_str());
    let path = target.split('?').next().unwrap_or("");
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let safe = |name: &str| registry::load(name).is_some_and(|client| client.name == name);
    match (method, parts.as_slice()) {
        ("GET", ["api", "version"]) => respond(
            &mut stream,
            "200 OK",
            "text/plain",
            version::current().as_bytes(),
        ),
        ("GET", ["api", "home"]) => {
            respond(&mut stream, "200 OK", "text/plain", home_name().as_bytes())
        }
        ("POST", ["api", "quit"]) => {
            respond(&mut stream, "200 OK", "text/plain", b"")?;
            std::process::exit(0)
        }
        ("GET", [""]) => respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
        ),
        ("GET", ["api", "clients"]) => {
            let tiles: Vec<Value> = registry::all().iter().map(status).collect();
            respond(
                &mut stream,
                "200 OK",
                "application/json",
                &serde_json::to_vec(&tiles)?,
            )
        }
        ("GET", ["thumb", name]) if safe(name) => match thumb(name) {
            Some(png) => respond(&mut stream, "200 OK", "image/png", &png),
            None => respond(&mut stream, "404 Not Found", "text/plain", b""),
        },
        ("POST", ["api", name, verb]) if safe(name) => match action(registry, name, verb) {
            Ok(()) => respond(&mut stream, "200 OK", "application/json", b"{}"),
            Err(error) => respond(&mut stream, "500 Error", "text/plain", error.as_bytes()),
        },
        _ => respond(&mut stream, "404 Not Found", "text/plain", b""),
    }
}

pub fn run(mut registry: Registry) -> Result<(), String> {
    registry.dashboard_port = registry::mosaic_port(&registry)?;
    let listener =
        TcpListener::bind(("127.0.0.1", registry.dashboard_port)).map_err(|e| e.to_string())?;
    for stream in listener.incoming().flatten() {
        let registry = registry.clone();
        thread::spawn(move || request(&registry, stream));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::control::fake::{FakeClient, NAME};
    use crate::hangup::{hang_up, held_by_a_child};

    #[test]
    fn a_stale_client_stays_visible_and_stoppable() {
        let fake = FakeClient::stale(Level::Render);
        let registry = Registry::load();
        let client = registry::load(NAME).unwrap();
        assert_eq!(status(&client)["state"], "connected");
        thumb(NAME);
        action(&registry, NAME, "take").unwrap();
        action(&registry, NAME, "give").unwrap();
        action(&registry, NAME, "stop").unwrap();
        assert_eq!(
            fake.heard(),
            ["state", "thumb", "show focus", "hide", "quit"]
        );
        assert!(!registry::alive(&client));
    }

    fn head(method: &str, host: Option<&str>, origin: Option<&str>) -> Head {
        Head {
            method: method.into(),
            target: "/".into(),
            host: host.map(str::to_string),
            origin: origin.map(str::to_string),
        }
    }

    #[test]
    fn only_the_page_itself_may_act() {
        let page = Some("http://127.0.0.1:25685");
        assert!(trusted(&head("GET", Some("127.0.0.1:25685"), None), 25685));
        assert!(trusted(&head("POST", Some("127.0.0.1:25685"), page), 25685));
        assert!(trusted(
            &head(
                "POST",
                Some("localhost:25685"),
                Some("http://localhost:25685")
            ),
            25685
        ));
        assert!(!trusted(
            &head(
                "POST",
                Some("127.0.0.1:25685"),
                Some("https://evil.example")
            ),
            25685
        ));
        assert!(!trusted(
            &head("POST", Some("127.0.0.1:25685"), None),
            25685
        ));
        assert!(!trusted(
            &head("GET", Some("evil.example:25685"), None),
            25685
        ));
        assert!(!trusted(&head("GET", None, None), 25685));
    }

    #[test]
    fn a_request_sent_drop_by_drop_is_cut_at_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let dripping = thread::spawn(move || {
            for _ in 0..200 {
                if client.write_all(b"G").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        assert!(head_within(&server, Duration::from_millis(200)).is_err());
        assert!(started.elapsed() < Duration::from_secs(4));
        let _child = held_by_a_child(&server);
        hang_up(server);
        dripping.join().unwrap();
    }

    #[test]
    fn a_mosaic_tells_its_version_and_another_is_told_apart() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let registry = Registry {
            dashboard_port: port,
            ..crate::registry::Registry::load()
        };
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let registry = registry.clone();
                thread::spawn(move || request(&registry, stream));
            }
        });
        assert_eq!(served(port), Served::Ours);
        let other = TcpListener::bind("127.0.0.1:0").unwrap();
        let old_port = other.local_addr().unwrap().port();
        thread::spawn(move || {
            for mut stream in other.incoming().flatten() {
                thread::spawn(move || {
                    let _ = super::head(&stream);
                    let _ = respond(&mut stream, "404 Not Found", "text/plain", b"");
                });
            }
        });
        assert_eq!(served(old_port), Served::Older);
    }

    /// One thread per connection, as `run` does, and every socket stays held after its answer, as
    /// by a child forked at the wrong moment: only a shutdown ends a request here.
    fn fake(answer: impl Fn(&str, bool) -> Option<String> + Send + Sync + 'static) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let answer = Arc::new(answer);
        let quit = Arc::new(AtomicBool::new(false));
        let held = Arc::new(Mutex::new(Vec::new()));
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let (answer, quit, held) = (answer.clone(), quit.clone(), held.clone());
                held.lock().unwrap().push(held_by_a_child(&stream));
                thread::spawn(move || {
                    let Ok(head) = super::head(&stream) else {
                        return;
                    };
                    if head.target.is_empty() {
                        return;
                    }
                    if head.target == "/api/quit" {
                        quit.store(true, Ordering::SeqCst);
                    }
                    let _ = match answer(&head.target, quit.load(Ordering::SeqCst)) {
                        Some(body) => respond(&mut stream, "200 OK", "text/plain", body.as_bytes()),
                        None => respond(&mut stream, "404 Not Found", "text/plain", b""),
                    };
                });
            }
        });
        port
    }

    const OTHER: &str = "0000000000000000";

    #[test]
    fn a_mosaic_of_another_version_that_stays_is_an_error_not_a_success() {
        let port = fake(|target, _| match target {
            "/api/home" => Some(home_name()),
            _ => Some(OTHER.into()),
        });
        let error = ensure_within(port, Duration::from_millis(200)).unwrap_err();
        assert!(error.contains("did not quit"), "{error}");
    }

    #[test]
    fn a_mosaic_of_our_version_that_takes_the_port_meanwhile_is_kept() {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let heard = asked.clone();
        let port = fake(move |target, quit| {
            heard.lock().unwrap().push(target.to_string());
            match target {
                "/api/home" => Some(home_name()),
                _ if quit => Some(version::current()),
                _ => Some(OTHER.into()),
            }
        });
        assert_eq!(ensure_on(port).unwrap(), format!("http://127.0.0.1:{port}"));
        assert_eq!(
            *asked.lock().unwrap(),
            [
                "/api/version",
                "/api/home",
                "/api/quit",
                "/api/version",
                "/api/home"
            ]
        );
    }

    #[test]
    fn the_mosaic_of_another_home_is_never_asked_to_quit() {
        let asked = Arc::new(AtomicBool::new(false));
        let seen = asked.clone();
        let port = fake(move |target, quit| {
            seen.store(quit, Ordering::SeqCst);
            match target {
                "/api/home" => Some("/somewhere/else".into()),
                _ => Some(OTHER.into()),
            }
        });
        let error = ensure_on(port).unwrap_err();
        assert!(error.contains("another VOIDC_HOME"), "{error}");
        assert!(!asked.load(Ordering::SeqCst));
        let ours = fake(|target, _| match target {
            "/api/home" => Some("/somewhere/else".into()),
            _ => Some(version::current()),
        });
        assert!(ensure_on(ours).is_err());
    }

    #[test]
    fn a_mosaic_from_before_homes_is_replaced_like_one_of_our_home() {
        let port = fake(|target, quit| match target {
            "/api/home" => None,
            _ if quit => Some(version::current()),
            _ => Some(OTHER.into()),
        });
        assert_eq!(served(port), Served::Other);
        assert_eq!(ensure_on(port).unwrap(), format!("http://127.0.0.1:{port}"));
    }
}
