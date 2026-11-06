use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const MAX_NAME: usize = 24;
const MAX_FEATURE_WORDS: usize = 2;

#[derive(Debug, Clone, Deserialize)]
pub struct Registry {
    pub max_clients: usize,
    pub idle_minutes: u32,
    pub dashboard_port: u16,
    pub lease_from: u16,
    pub lease_to: u16,
    pub reserved: BTreeMap<String, String>,
    pub levels: BTreeMap<String, Settings>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub render_distance: u32,
    pub simulation_distance: u32,
    pub heap: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Render,
    Logic,
    Bot,
    Play,
}

impl Level {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "render" => Ok(Self::Render),
            "logic" => Ok(Self::Logic),
            "bot" => Ok(Self::Bot),
            "play" => Ok(Self::Play),
            other => Err(format!("unknown level {other}: render, logic, bot or play")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::Logic => "logic",
            Self::Bot => "bot",
            Self::Play => "play",
        }
    }

    pub fn counts_towards_limit(self) -> bool {
        self != Self::Bot
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    pub name: String,
    pub feature: String,
    pub port: u16,
    pub level: Level,
    pub pid: u32,
    pub fps: Option<u32>,
    pub size: Option<(u32, u32)>,
    pub jfr: bool,
}

impl Registry {
    pub fn load() -> Self {
        toml::from_str(include_str!("../clients.toml")).expect("clients.toml is part of the build")
    }

    pub fn is_reserved(&self, port: u16) -> bool {
        self.reserved.contains_key(&port.to_string())
    }

    pub fn settings(&self, level: Level) -> &Settings {
        &self.levels[level.name()]
    }
}

/// The folder of the runtime state, shared by every worktree.
pub fn home() -> PathBuf {
    match std::env::var_os("VOIDC_HOME") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".voidc"),
    }
}

pub fn client_dir(name: &str) -> PathBuf {
    home().join("clients").join(name)
}

pub fn socket(name: &str) -> PathBuf {
    home().join("run").join(format!("{name}.sock"))
}

pub fn title(level: Level, feature: &str) -> String {
    let prefix = if level == Level::Play {
        "JOUER"
    } else {
        "SONDE"
    };
    format!("{prefix} {feature}")
}

/// A feature names a client: two words at most, so that a title never exceeds three.
pub fn slug(feature: &str) -> Result<String, String> {
    let words: Vec<&str> = feature.split_whitespace().collect();
    if words.is_empty() || words.len() > MAX_FEATURE_WORDS {
        return Err(format!(
            "a feature is one or two words, got {feature:?}: the title is the prefix and the feature"
        ));
    }
    let slug: String = words
        .join("-")
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    if slug.len() > MAX_NAME {
        return Err(format!("feature {feature:?} is too long"));
    }
    Ok(slug)
}

pub fn save(client: &Client) -> std::io::Result<()> {
    let dir = client_dir(&client.name);
    fs::create_dir_all(&dir)?;
    let temporary = dir.join("client.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(client)?)?;
    fs::rename(temporary, dir.join("client.json"))
}

pub fn load(name: &str) -> Option<Client> {
    let bytes = fs::read(client_dir(name).join("client.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn all() -> Vec<Client> {
    let Ok(entries) = fs::read_dir(home().join("clients")) else {
        return Vec::new();
    };
    let mut clients: Vec<Client> = entries
        .flatten()
        .filter_map(|entry| load(&entry.file_name().to_string_lossy()))
        .collect();
    clients.sort_by(|a, b| a.name.cmp(&b.name));
    clients
}

/// Whether the recorded process is still this client. A reused pid never passes.
pub fn alive(client: &Client) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "command=", "-p", &client.pid.to_string()])
        .output();
    let Ok(output) = output else { return false };
    let command = String::from_utf8_lossy(&output.stdout);
    command.contains(&format!("-Dvoid.client={} ", client.name))
        || command.contains(&format!("bot-run {} ", client.name))
}

/// One `voidc` at a time between a check and what it guards.
pub struct Lock(PathBuf);

impl Lock {
    pub fn take() -> std::io::Result<Self> {
        fs::create_dir_all(home())?;
        let path = home().join("lock");
        for _ in 0..500 {
            if fs::create_dir(&path).is_ok() {
                return Ok(Self(path));
            }
            sleep(Duration::from_millis(10));
        }
        let _ = fs::remove_dir(&path);
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn leases() -> BTreeMap<String, u16> {
    fs::read(home().join("ports.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn lease_of(feature: &str) -> Option<u16> {
    leases().get(feature).copied()
}

pub fn feature_on(port: u16) -> Option<String> {
    leases()
        .into_iter()
        .find(|(_, leased)| *leased == port || *leased + 1 == port)
        .map(|(feature, _)| feature)
}

fn listening(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// The pair of ports of a feature, the same one every time.
pub fn lease(registry: &Registry, feature: &str) -> Result<u16, String> {
    let _lock = Lock::take().map_err(|e| e.to_string())?;
    let mut all = leases();
    if let Some(port) = all.get(feature) {
        return Ok(*port);
    }
    let taken: Vec<u16> = all.values().copied().collect();
    let port = free_pair(registry, &taken, listening).ok_or("no free ports left")?;
    all.insert(feature.to_string(), port);
    let bytes = serde_json::to_vec_pretty(&all).map_err(|e| e.to_string())?;
    fs::write(home().join("ports.json"), bytes).map_err(|e| e.to_string())?;
    Ok(port)
}

fn free_pair(registry: &Registry, taken: &[u16], busy: impl Fn(u16) -> bool) -> Option<u16> {
    (registry.lease_from..registry.lease_to)
        .step_by(2)
        .find(|port| {
            [*port, port + 1]
                .iter()
                .all(|p| !registry.is_reserved(*p) && !taken.contains(p) && !busy(*p))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_never_exceeds_three_words() {
        assert_eq!(slug("Hot reload").unwrap(), "hot-reload");
        assert!(slug("Un titre trop long").is_err());
        assert!(slug("  ").is_err());
        assert_eq!(title(Level::Render, "Hot reload").split(' ').count(), 3);
    }

    #[test]
    fn a_slug_stays_a_safe_file_name() {
        assert_eq!(slug("Vie/plage").unwrap(), "vie-plage");
        assert_eq!(slug("Scènes").unwrap(), "sc-nes");
    }

    #[test]
    fn the_registry_of_the_build_parses_and_reserves_its_dashboard() {
        let registry = Registry::load();
        assert_eq!(registry.max_clients, 9);
        assert!(registry.is_reserved(registry.dashboard_port));
        for level in [Level::Render, Level::Logic, Level::Play] {
            registry.settings(level);
        }
    }

    #[test]
    fn a_lease_skips_what_is_taken_reserved_or_listening() {
        let mut registry = Registry::load();
        registry
            .reserved
            .insert(registry.lease_from.to_string(), "x".into());
        let first = registry.lease_from + 2;
        let taken = [first];
        let busy = first + 3;
        let port = free_pair(&registry, &taken, |p| p == busy).unwrap();
        assert_eq!(port, registry.lease_from + 6);
    }

    #[test]
    fn a_bot_does_not_count_towards_the_limit() {
        assert!(!Level::Bot.counts_towards_limit());
        assert!(Level::Render.counts_towards_limit());
    }
}
