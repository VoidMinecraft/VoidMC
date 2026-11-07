mod bot;
mod check;
mod compare;
mod condition;
mod control;
mod dash;
mod frame;
#[cfg(test)]
mod hangup;
#[cfg(test)]
mod held;
mod launch;
mod lifecycle;
mod registry;
mod scenario;
mod step;
mod trace;
mod version;

use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant};

use control::{Chore, Failure};
use registry::{Client, Level, Registry};

const USAGE: &str = "voidc: the clients of the agents

  voidc port \"Feature\"                     the pair of ports of a feature
  voidc up \"Feature\" [--port N] [--level render|logic|bot] [--fps N] [--size WxH] [--jfr]
                                            starts a client and waits until it is in game
  voidc play \"Feature\" --port N            a visible client for Léo, never driven
  voidc cmd <name> <verb> [args]            act, wait_until, assert, state, shot, ...
  voidc step \"Feature\" --server <socket> --out <dir> [--ticks N] [--frames K] [--lock-at T] [--act <command>]...
             [--every N] [--at i,j] [--min-chunks N] [--size WxH] [--level render|logic] [--port N]
             [--hold T:S] [--stop-server]
                                            plays a passage frame by frame on a stepped server
  voidc run <file> (--on <name> | --feature \"Feature\") [--keep-going] [--keep-up] [--out DIR]
                                            plays a scenario, refused before it starts if it cannot run
  voidc compare <a.png> <b.png> [max=N] [tile=N] [ignore=x,y,w,h] [require=x,y,w,h:max:delta]
                                            compares two pictures by zones, writes the difference
  voidc check <frames> [cut=T] [jump=N] [turn=N] [flash=N] [images=dir]
                                            looks for jumps, half turns and flashes in a recording
  voidc down <name>                         stops a client
  voidc forget <name>                       stops it and deletes its folder
  voidc list                                every client and its state
  voidc dash                                the address of the mosaic
  voidc trace <file> | diff <a> <b>         reads a packet trace, see trace --help";

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
        let state = control::answered(control::chore(name, Chore::State, Duration::from_secs(5)));
        if state.is_ok_and(|reply| reply["state"] == "connected") {
            println!(
                "{name}: in game after {:.1} s",
                start.elapsed().as_secs_f32()
            );
            return Ok(());
        }
        sleep(Duration::from_millis(250));
    }
    Err(format!(
        "{name} is not in game after {} s, see {}",
        JOIN_LIMIT.as_secs(),
        log.display()
    ))
}

fn client_of(args: &[String], level: Level) -> Result<Client, String> {
    let feature = args.first().ok_or("which feature?")?.clone();
    let name = registry::slug(&feature)?;
    let port = match flag(args, "--port") {
        Some(port) => port.parse().map_err(|_| "bad port")?,
        None => registry::lease_of(&feature)?
            .ok_or("no port: pass --port, or take one with voidc port")?,
    };
    Ok(Client {
        name,
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
    })
}

fn up(registry: &Registry, args: &[String], level: Level) -> Result<(), String> {
    let client = client_of(args, level)?;
    let name = client.name.clone();
    lifecycle::start(registry, client)?;
    let started = || {
        println!("mosaic: {}", dash::ensure(registry)?);
        if level == Level::Play {
            println!("{name}: started");
            return Ok(());
        }
        wait_in_game(&name)
    };
    stop_on_failure(&name, started(), lifecycle::down)
}

fn stop_on_failure(
    name: &str,
    result: Result<(), String>,
    stop: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    result.map_err(|error| match stop(name) {
        Ok(()) => format!("{error}\n{name} is stopped, so it does not stay behind"),
        Err(stop) => format!("{error}\n{name} could not be stopped: {stop}"),
    })
}

fn line_of(client: &Client) -> String {
    let running = registry::alive(client);
    let state = if !running {
        "stopped".to_string()
    } else if client.level == Level::Play {
        "playing".to_string()
    } else {
        control::answered(control::chore(
            &client.name,
            Chore::State,
            Duration::from_secs(3),
        ))
        .ok()
        .and_then(|reply| reply["state"].as_str().map(str::to_string))
        .unwrap_or_else(|| "starting".to_string())
    };
    format!(
        "{:<24} {:<7} {:<6} {state}",
        client.name,
        client.level.name(),
        client.port
    )
}

fn list() {
    for client in registry::all() {
        println!("{}", line_of(&client));
    }
}

fn cmd(args: &[String]) -> Result<(), String> {
    let name = args.first().ok_or("which client?")?;
    let client = registry::load(name).ok_or_else(|| format!("unknown client {name}"))?;
    if client.level == Level::Play {
        return Err(format!("{name} is played by Léo and takes no command"));
    }
    let verb = args[1..].join(" ");
    condition::check(&verb, client.level)?;
    match control::ask(name, &verb) {
        Ok(reply) => {
            println!("{reply}");
            Ok(())
        }
        Err(Failure::Refused(reply)) => {
            println!("{reply}");
            Err(String::new())
        }
        Err(Failure::Lost(error)) => Err(error),
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
        "step" => {
            let level = flag(rest, "--level")
                .as_deref()
                .map_or(Ok(Level::Render), Level::parse)?;
            step::run(&registry, client_of(rest, level)?, rest)
        }
        "run" => scenario::command(
            rest,
            &mut scenario::Live(
                |feature: &str, level, port: Option<String>| {
                    let mut args = vec![feature.to_string()];
                    if let Some(port) = port {
                        args.extend(["--port".to_string(), port]);
                    }
                    up(&registry, &args, level)
                },
                control::Limits::REAL,
            ),
        ),
        "compare" => compare::command(rest),
        "check" => check::command(rest),
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
        "trace" => trace::run(rest),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_failed_up_stops_its_client_and_a_good_one_leaves_it() {
        let stopped = Cell::new(0);
        let stop = |_: &str| {
            stopped.set(stopped.get() + 1);
            Ok(())
        };
        assert!(stop_on_failure("x", Ok(()), stop).is_ok());
        assert_eq!(stopped.get(), 0);
        let error = stop_on_failure("x", Err("not in game".into()), stop).unwrap_err();
        assert_eq!(stopped.get(), 1);
        assert!(error.contains("not in game") && error.contains("x is stopped"));
        let error =
            stop_on_failure("x", Err("not in game".into()), |_| Err("busy".into())).unwrap_err();
        assert!(error.contains("could not be stopped: busy"));
    }

    #[test]
    fn list_still_sees_a_probe_of_another_version() {
        let fake = control::fake::FakeClient::stale(Level::Render);
        let client = registry::load(control::fake::NAME).unwrap();
        let line = line_of(&client);
        assert!(line.ends_with(" connected"), "{line}");
        assert_eq!(fake.heard(), ["state"]);
    }

    #[test]
    fn cmd_refuses_a_probe_of_another_version() {
        let fake = control::fake::FakeClient::stale(Level::Render);
        let args = [control::fake::NAME, "act", "map"].map(String::from);
        fake.assert_refused(cmd(&args));
    }
}
