use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::control::{self, Chore};
use crate::launch;
use crate::registry::{self, Client, Level, Registry};

const STOP_PATIENCE: Duration = Duration::from_secs(10);

/// An error of the run keeps the error of the stop that follows it: both have something to say.
pub fn with_stop(error: String, stopped: Result<(), String>) -> String {
    match stopped {
        Ok(()) => error,
        Err(stuck) => format!("{error}\n{stuck}"),
    }
}

pub fn running() -> Vec<Client> {
    registry::all()
        .into_iter()
        .filter(registry::alive)
        .collect()
}

/// Refuses what would go wrong: a limit passed, a port of another feature, a second `JOUER`.
pub fn check(registry: &Registry, client: &Client, others: &[Client]) -> Result<(), String> {
    if others.iter().any(|other| other.name == client.name) {
        return Err(format!("{} is already running", client.name));
    }
    let heavy = others
        .iter()
        .filter(|other| other.level.counts_towards_limit())
        .count();
    if client.level.counts_towards_limit() && heavy >= registry.max_clients {
        return Err(format!(
            "{heavy} clients are running, the limit is {}: stop one with voidc down, or use --level bot",
            registry.max_clients
        ));
    }
    if client.level == Level::Play && others.iter().any(|other| other.level == Level::Play) {
        return Err(
            "a client to play is already running: Léo cannot tell two windows apart".into(),
        );
    }
    Ok(())
}

fn free_gib(path: &Path) -> Option<u64> {
    let output = Command::new("df").arg("-Pk").arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let available: u64 = text
        .lines()
        .last()?
        .split_whitespace()
        .nth(3)?
        .parse()
        .ok()?;
    Some(available / (1024 * 1024))
}

fn low_disk(free: u64, least: u64) -> Option<String> {
    (free < least).then(|| {
        format!(
            "WARNING: DISK NEARLY FULL, {free} GiB free, under the {least} GiB of clients.toml. Empty a target/ before a build breaks the other sessions."
        )
    })
}

/// Only warns: a client that does not start helps nobody free the disk.
fn disk_warning(registry: &Registry) -> Option<String> {
    low_disk(
        free_gib(&registry::home_or_parent())?,
        registry.min_free_gib,
    )
}

pub fn start(registry: &Registry, mut client: Client) -> Result<(), String> {
    check(registry, &client, &running())?;
    if let Some(warning) = disk_warning(registry) {
        eprintln!("{warning}");
    }
    if client.level != Level::Play {
        registry::socket(&client.name)?;
    }
    let command = match client.level {
        Level::Bot => launch::bot(registry, &client)?,
        _ => launch::game(registry, &client)?,
    };
    let _lock = registry::Lock::take().map_err(|e| e.to_string())?;
    check(registry, &client, &running())?;
    if let Some(owner) = registry::feature_on(client.port)?.filter(|owner| *owner != client.feature)
    {
        return Err(format!("port {} is leased to {owner}", client.port));
    }
    let mut child = launch::spawn(command, &registry::client_dir(&client.name))?;
    client.pid = child.id();
    registry::save(&client).map_err(|error| {
        stop(&mut child);
        format!(
            "{} could not be recorded, so it is stopped: {error}",
            client.name
        )
    })
}

fn stop(child: &mut Child) {
    stop_within(child, STOP_PATIENCE);
}

fn stop_within(child: &mut Child, patience: Duration) {
    let _ = Command::new("kill").arg(child.id().to_string()).status();
    let deadline = Instant::now() + patience;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn gone(client: &Client) -> bool {
    let deadline = Instant::now() + STOP_PATIENCE;
    while Instant::now() < deadline {
        if !registry::alive(client) {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

/// Only ever signals a pid that `registry::alive` recognises as this client.
pub fn down(name: &str) -> Result<(), String> {
    let client = registry::load(name).ok_or_else(|| format!("unknown client {name}"))?;
    if !registry::alive(&client) {
        return Ok(());
    }
    if client.level != Level::Play {
        let _ = control::chore(name, Chore::Quit, Duration::from_secs(2));
    }
    if gone(&client) {
        return Ok(());
    }
    if registry::alive(&client) {
        Command::new("kill")
            .arg(client.pid.to_string())
            .status()
            .map_err(|e| e.to_string())?;
    }
    if gone(&client) {
        Ok(())
    } else {
        Err(format!("{name} did not stop"))
    }
}

pub fn forget(name: &str) -> Result<(), String> {
    down(name)?;
    fs::remove_dir_all(registry::client_dir(name)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::held::Held;

    #[test]
    fn a_client_has_ten_seconds_to_stop_for_real() {
        assert_eq!(STOP_PATIENCE, Duration::from_secs(10));
    }

    #[test]
    fn a_child_that_ignores_the_signal_is_killed_and_reaped() {
        let mut polite = Held::waiting("polite");
        let ended = polite.ended_by(|child| stop_within(child, Duration::from_secs(5)));
        assert!(ended.is_some());
        let mut stubborn = Held::launch(
            Command::new("sh")
                .args(["-c", "trap '' TERM; echo ready; read line"])
                .stdout(std::process::Stdio::piped()),
        );
        assert_eq!(stubborn.first_line(), "ready");
        let started = Instant::now();
        let ended = stubborn.ended_by(|child| stop_within(child, Duration::from_millis(500)));
        assert!(started.elapsed() < Duration::from_secs(5));
        let status = ended.expect("reaped");
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(9)
        );
    }

    fn client(name: &str, level: Level) -> Client {
        Client {
            name: name.into(),
            feature: name.into(),
            port: 25700,
            level,
            pid: 0,
            fps: None,
            size: None,
            jfr: false,
        }
    }

    #[test]
    fn the_disk_warns_under_its_threshold_and_only_then() {
        assert!(low_disk(14, 15).unwrap().contains("14 GiB free"));
        assert!(low_disk(15, 15).is_none());
    }

    #[test]
    fn free_space_is_read_from_the_system() {
        assert!(free_gib(Path::new("/")).is_some());
        assert!(free_gib(Path::new("/no/such/folder")).is_none());
    }

    #[test]
    fn the_tenth_client_is_refused_but_a_bot_is_not() {
        let registry = Registry::load();
        let nine: Vec<Client> = (0..9)
            .map(|i| client(&format!("c{i}"), Level::Render))
            .collect();
        assert!(check(&registry, &client("next", Level::Render), &nine).is_err());
        assert!(check(&registry, &client("bot", Level::Bot), &nine).is_ok());
        assert!(check(&registry, &client("next", Level::Render), &nine[..8]).is_ok());
    }

    #[test]
    fn bots_leave_room_for_clients() {
        let registry = Registry::load();
        let bots: Vec<Client> = (0..20)
            .map(|i| client(&format!("b{i}"), Level::Bot))
            .collect();
        assert!(check(&registry, &client("next", Level::Render), &bots).is_ok());
    }

    #[test]
    fn only_one_client_to_play() {
        let registry = Registry::load();
        let others = [client("jouer", Level::Play)];
        assert!(check(&registry, &client("autre", Level::Play), &others).is_err());
    }

    #[test]
    fn a_client_does_not_start_twice() {
        let registry = Registry::load();
        let others = [client("same", Level::Render)];
        assert!(check(&registry, &client("same", Level::Render), &others).is_err());
    }
}
