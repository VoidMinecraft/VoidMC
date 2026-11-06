use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::registry::{self, Client, Level, Registry};
use crate::{control, lifecycle};

const PAGE: &str = include_str!("dash.html");
const ASK_TIMEOUT: Duration = Duration::from_secs(3);

/// Starts the page if nobody serves it yet, and says where it is.
pub fn ensure(registry: &Registry) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{}", registry.dashboard_port);
    if TcpStream::connect(("127.0.0.1", registry.dashboard_port)).is_ok() {
        return Ok(url);
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
    let mut first = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut first)?;
    let mut words = first.split_whitespace();
    let (method, target) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let path = target.split('?').next().unwrap_or("");
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let safe = |name: &str| registry::load(name).is_some_and(|client| client.name == name);
    match (method, parts.as_slice()) {
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
