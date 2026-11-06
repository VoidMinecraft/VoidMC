mod bot;
mod condition;
mod control;
mod dash;
mod frame;
mod launch;
mod lifecycle;
mod registry;

use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant};

use registry::{Client, Level, Registry};
use serde_json::Value;

const USAGE: &str = "voidc: the clients of the agents

  voidc port \"Feature\"                     the pair of ports of a feature
  voidc up \"Feature\" [--port N] [--level render|logic|bot] [--fps N] [--size WxH] [--jfr]
                                            starts a client and waits until it is in game
  voidc play \"Feature\" --port N            a visible client for Léo, never driven
  voidc cmd <name> <verb> [args]            act, wait_until, assert, state, shot, ...
  voidc down <name>                         stops a client
  voidc forget <name>                       stops it and deletes its folder
  voidc list                                every client and its state
  voidc dash                                the address of the mosaic";

const JOIN_LIMIT: Duration = Duration::from_secs(90);

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|at| args.get(at + 1))
        .cloned()
}

fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (width, height) = text.split_once('x').ok_or("--size is WxH")?;
    Ok((
        width.parse().map_err(|_| "bad width")?,
        height.parse().map_err(|_| "bad height")?,
    ))
}

fn wait_in_game(name: &str) -> Result<(), String> {
    let client = registry::load(name).ok_or_else(|| format!("unknown client {name}"))?;
    let log = registry::client_dir(name).join("client.log");
    let start = Instant::now();
    while start.elapsed() < JOIN_LIMIT {
        if !registry::alive(&client) {
            return Err(format!(
                "{name} stopped before it was in game, see {}",
                log.display()
            ));
        }
        if let Ok(line) = control::ask(name, "state", Duration::from_secs(5)) {
            let reply: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if reply["state"] == "connected" {
                println!(
                    "{name}: in game after {:.1} s",
                    start.elapsed().as_secs_f32()
                );
                return Ok(());
            }
        }
        sleep(Duration::from_millis(250));
    }
    Err(format!(
        "{name} is not in game after {} s, see {}",
        JOIN_LIMIT.as_secs(),
        log.display()
    ))
}

fn up(registry: &Registry, args: &[String], level: Level) -> Result<(), String> {
    let feature = args.first().ok_or("which feature?")?.clone();
    let name = registry::slug(&feature)?;
    let port = match flag(args, "--port") {
        Some(port) => port.parse().map_err(|_| "bad port")?,
        None => registry::lease_of(&feature)?
            .ok_or("no port: pass --port, or take one with voidc port")?,
    };
    let client = Client {
        name: name.clone(),
        feature,
        port,
        level,
        pid: 0,
        fps: flag(args, "--fps")
            .map(|fps| fps.parse().map_err(|_| "bad fps"))
            .transpose()?,
        size: flag(args, "--size")
            .as_deref()
            .map(parse_size)
            .transpose()?,
        jfr: args.iter().any(|arg| arg == "--jfr"),
    };
    lifecycle::start(registry, client)?;
    println!("mosaic: {}", dash::ensure(registry)?);
    if level == Level::Play {
        println!("{name}: started");
        return Ok(());
    }
    wait_in_game(&name).map_err(|error| match lifecycle::down(&name) {
        Ok(()) => format!("{error}\n{name} is stopped, so it does not stay behind"),
        Err(stop) => format!("{error}\n{name} could not be stopped: {stop}"),
    })
}

fn list() {
    for client in registry::all() {
        let running = registry::alive(&client);
        let state = if !running {
            "stopped".to_string()
        } else if client.level == Level::Play {
            "playing".to_string()
        } else {
            control::ask(&client.name, "state", Duration::from_secs(3))
                .ok()
                .and_then(|line| serde_json::from_str::<Value>(&line).ok())
                .and_then(|reply| reply["state"].as_str().map(str::to_string))
                .unwrap_or_else(|| "starting".to_string())
        };
        println!(
            "{:<24} {:<7} {:<6} {state}",
            client.name,
            client.level.name(),
            client.port
        );
    }
}

fn cmd(args: &[String]) -> Result<(), String> {
    let name = args.first().ok_or("which client?")?;
    let verb = args[1..].join(" ");
    let client = registry::load(name).ok_or_else(|| format!("unknown client {name}"))?;
    if client.level == Level::Play {
        return Err(format!("{name} is played by Léo and takes no command"));
    }
    condition::check(&verb)?;
    let reply = control::ask(name, &verb, control::timeout_of(&verb))?;
    println!("{reply}");
    let parsed: Value = serde_json::from_str(&reply).map_err(|e| e.to_string())?;
    if parsed["ok"] == true {
        Ok(())
    } else {
        Err(String::new())
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let registry = Registry::load();
    let (verb, rest) = args.split_first().ok_or(USAGE)?;
    match verb.as_str() {
        "port" => {
            let port = registry::lease(&registry, rest.first().ok_or("which feature?")?)?;
            println!("{port} {}", port + 1);
            Ok(())
        }
        "up" => {
            let level = flag(rest, "--level")
                .as_deref()
                .map_or(Ok(Level::Render), Level::parse)?;
            if level == Level::Play {
                return Err("use voidc play".into());
            }
            up(&registry, rest, level)
        }
        "play" => up(&registry, rest, Level::Play),
        "cmd" => cmd(rest),
        "down" => lifecycle::down(rest.first().ok_or("which client?")?),
        "forget" => lifecycle::forget(rest.first().ok_or("which client?")?),
        "list" => {
            list();
            Ok(())
        }
        "dash" => {
            println!("{}", dash::ensure(&registry)?);
            Ok(())
        }
        "dash-run" => dash::run(registry),
        "bot-run" => {
            let name = rest.first().ok_or("name")?;
            let port = rest.get(1).ok_or("port")?.parse().map_err(|_| "bad port")?;
            let idle: u64 = rest
                .get(2)
                .ok_or("idle minutes")?
                .parse()
                .map_err(|_| "bad idle minutes")?;
            bot::run(name, port, Duration::from_secs(idle * 60))
        }
        _ => Err(USAGE.to_string()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if !error.is_empty() {
                eprintln!("{error}");
            }
            ExitCode::FAILURE
        }
    }
}
