use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
    pub min_free_gib: u64,
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

fn default_home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".voidc")
}

/// The folder of the runtime state, shared by every worktree.
pub fn home() -> PathBuf {
    std::env::var_os("VOIDC_HOME").map_or_else(default_home, PathBuf::from)
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.components().collect())
}

pub fn home_name() -> String {
    canonical(&home()).display().to_string()
}

const MOSAIC: &str = "(mosaic)";

pub fn mosaic_port(registry: &Registry) -> Result<u16, String> {
    mosaic_port_in(registry, &home(), &default_home(), listening)
}

fn mosaic_port_in(
    registry: &Registry,
    home: &Path,
    default: &Path,
    busy: impl Fn(u16) -> bool,
) -> Result<u16, String> {
    let (home, default) = (canonical(home), canonical(default));
    if home == default {
        return Ok(registry.dashboard_port);
    }
    lease_in(
        registry,
        &default,
        &format!("{MOSAIC} {}", home.display()),
        busy,
    )
}

/// The nearest folder that exists, for what has to look at the disk before the first client.
pub fn home_or_parent() -> PathBuf {
    let home = home();
    home.ancestors()
        .find(|path| path.exists())
        .map_or(home.clone(), PathBuf::from)
}

pub fn client_dir(name: &str) -> PathBuf {
    home().join("clients").join(name)
}

const SOCKET_PATH_LIMIT: usize = 103;

pub fn socket(name: &str) -> Result<PathBuf, String> {
    let path = home().join("run").join(format!("{name}.sock"));
    let length = path.as_os_str().len();
    if length > SOCKET_PATH_LIMIT {
        return Err(format!(
            "the socket {} is {length} bytes long and macOS takes at most {SOCKET_PATH_LIMIT}: use a shorter VOIDC_HOME or name",
            path.display()
        ));
    }
    Ok(path)
}

fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
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
    write_whole(
        &dir.join("client.json"),
        &serde_json::to_vec_pretty(client)?,
    )
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

pub struct Lock {
    _held: fs::File,
}

impl Lock {
    pub fn take() -> std::io::Result<Self> {
        fs::create_dir_all(home())?;
        Self::at(&home().join("start.lock"))
    }

    fn at(path: &Path) -> std::io::Result<Self> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        file.lock()?;
        file.set_len(0)?;
        write!(file, "{}", std::process::id())?;
        Ok(Self { _held: file })
    }
}

fn leases() -> Result<BTreeMap<String, u16>, String> {
    leases_at(&home().join("ports.json"))
}

fn leases_at(path: &Path) -> Result<BTreeMap<String, u16>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("{} is damaged ({e}): fix or delete it", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

pub fn lease_of(feature: &str) -> Result<Option<u16>, String> {
    Ok(leases()?.get(feature).copied())
}

pub fn feature_on(port: u16) -> Result<Option<String>, String> {
    Ok(leases()?
        .into_iter()
        .find(|(_, leased)| *leased == port || *leased + 1 == port)
        .map(|(feature, _)| feature))
}

fn listening(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// The pair of ports of a feature, the same one every time.
pub fn lease(registry: &Registry, feature: &str) -> Result<u16, String> {
    slug(feature)?;
    lease_in(registry, &home(), feature, listening)
}

fn lease_in(
    registry: &Registry,
    home: &Path,
    feature: &str,
    busy: impl Fn(u16) -> bool,
) -> Result<u16, String> {
    fs::create_dir_all(home).map_err(|e| e.to_string())?;
    let _lock = Lock::at(&home.join("start.lock")).map_err(|e| e.to_string())?;
    let mut all = leases_at(&home.join("ports.json"))?;
    if let Some(port) = all.get(feature) {
        return Ok(*port);
    }
    let taken: Vec<u16> = all.values().copied().collect();
    let port = free_pair(registry, &taken, busy).ok_or("no free ports left")?;
    all.insert(feature.to_string(), port);
    let bytes = serde_json::to_vec_pretty(&all).map_err(|e| e.to_string())?;
    write_whole(&home.join("ports.json"), &bytes).map_err(|e| e.to_string())?;
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("voidc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn two_homes_never_share_a_mosaic_and_the_default_keeps_its_own() {
        let registry = Registry::load();
        let default = scratch("home-default");
        let (first, second) = (scratch("home-first"), scratch("home-second"));
        let mut slashed = default.clone().into_os_string();
        slashed.push("/");
        assert_eq!(
            mosaic_port_in(&registry, Path::new(&slashed), &default, |_| false).unwrap(),
            registry.dashboard_port
        );
        let link = scratch("home-link").join("voidc");
        std::os::unix::fs::symlink(&default, &link).unwrap();
        assert_eq!(
            mosaic_port_in(&registry, &link, &default, |_| false).unwrap(),
            registry.dashboard_port
        );
        let a = mosaic_port_in(&registry, &first, &default, |_| false).unwrap();
        let b = mosaic_port_in(&registry, &second, &default, |_| false).unwrap();
        let feature = lease_in(&registry, &default, "Hot reload", |_| false).unwrap();
        assert_ne!(a, b);
        assert!(![a, b, registry.dashboard_port].contains(&feature));
        assert_ne!(a, registry.dashboard_port);
        assert_eq!(
            mosaic_port_in(&registry, &first, &default, |_| true).unwrap(),
            a
        );
        assert!(!first.join("ports.json").exists());
    }

    #[test]
    fn a_living_holder_keeps_the_lock_however_slow() {
        let path = scratch("lock").join("start.lock");
        let held = Lock::at(&path).unwrap();
        let waiter = {
            let path = path.clone();
            std::thread::spawn(move || {
                let _lock = Lock::at(&path).unwrap();
                std::time::Instant::now()
            })
        };
        std::thread::sleep(std::time::Duration::from_secs(6));
        let released = std::time::Instant::now();
        drop(held);
        assert!(waiter.join().unwrap() >= released);
    }

    #[test]
    fn a_damaged_ports_file_is_an_error_and_a_missing_one_is_empty() {
        let dir = scratch("ports");
        let path = dir.join("ports.json");
        assert!(leases_at(&path).unwrap().is_empty());
        write_whole(&path, b"{\"Hot reload\": 257").unwrap();
        assert!(leases_at(&path).unwrap_err().contains("damaged"));
        write_whole(&path, b"{\"Hot reload\": 25700}").unwrap();
        assert_eq!(leases_at(&path).unwrap()["Hot reload"], 25700);
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, ["ports.json"]);
    }

    #[test]
    fn a_socket_macos_cannot_bind_is_refused_before_anything_starts() {
        assert!(socket("hot-reload").is_ok());
        let long = "x".repeat(SOCKET_PATH_LIMIT);
        assert!(socket(&long).unwrap_err().contains("VOIDC_HOME"));
    }

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
    fn a_lease_refuses_a_feature_no_client_could_take() {
        let error = lease(&Registry::load(), "Trois mots ici").unwrap_err();
        assert!(error.contains("one or two words"), "{error}");
    }

    #[test]
    fn a_bot_does_not_count_towards_the_limit() {
        assert!(!Level::Bot.counts_towards_limit());
        assert!(Level::Render.counts_towards_limit());
    }
}
