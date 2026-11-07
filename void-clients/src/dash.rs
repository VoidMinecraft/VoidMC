use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::registry::{self, Client, Level, Registry};
use crate::{control, lifecycle, version};

const PAGE: &str = include_str!("dash.html");
const ASK_TIMEOUT: Duration = Duration::from_secs(3);
const HEAD_LIMIT: u64 = 16 * 1024;
const READ_LIMIT: Duration = Duration::from_secs(5);

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
    let whole = Deadline {
        stream: stream.try_clone()?,
        until: Instant::now() + READ_LIMIT,
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

/// Starts the page if nobody serves it yet, and says where it is.
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
}

fn served(port: u16) -> Served {
    if TcpStream::connect(("127.0.0.1", port)).is_err() {
        return Served::Nothing;
    }
    match call(port, "GET", "/api/version") {
        Some((200, version)) if version == version::current() => Served::Ours,
        Some((200, _)) => Served::Other,
        _ => Served::Older,
    }
}

pub fn ensure(registry: &Registry) -> Result<String, String> {
    let port = registry.dashboard_port;
    let url = format!("http://127.0.0.1:{port}");
    match served(port) {
        Served::Ours => return Ok(url),
        Served::Older => {
            eprintln!(
                "the mosaic on {port} comes from an older voidc and starts clients this one refuses: stop it, and voidc starts its own"
            );
            return Ok(url);
        }
        Served::Other => {
            let _ = call(port, "POST", "/api/quit");
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while served(port) != Served::Nothing && std::time::Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
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
    let reply = control::ask(&client.name, "state", ASK_TIMEOUT).ok();
    if let Some(Value::Object(fields)) = reply.and_then(|line| serde_json::from_str(&line).ok()) {
        for (key, value) in fields {
            if key != "level" {
                tile[key] = value;
            }
        }
    }
    tile
}

fn respond(stream: &mut TcpStream, code: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn thumb(name: &str) -> Option<Vec<u8>> {
    let line = control::ask(name, "thumb", ASK_TIMEOUT).ok()?;
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
        "take" => control::ask(name, "show focus", ASK_TIMEOUT).map(|_| ()),
        "give" => control::ask(name, "hide", ASK_TIMEOUT).map(|_| ()),
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

pub fn run(registry: Registry) -> Result<(), String> {
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
    use super::*;

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
            for _ in 0..20 {
                if client.write_all(b"G").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(400));
            }
        });
        let started = Instant::now();
        assert!(super::head(&server).is_err());
        assert!(started.elapsed() < READ_LIMIT + Duration::from_secs(1));
        drop(server);
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
                let _ = request(&registry, stream);
            }
        });
        assert_eq!(served(port), Served::Ours);
        let other = TcpListener::bind("127.0.0.1:0").unwrap();
        let old_port = other.local_addr().unwrap().port();
        thread::spawn(move || {
            for mut stream in other.incoming().flatten() {
                let _ = super::head(&stream);
                let _ = respond(&mut stream, "404 Not Found", "text/plain", b"");
            }
        });
        assert_eq!(served(old_port), Served::Older);
    }
}
