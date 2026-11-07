use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::registry::{self, Client, Level, Registry, Settings};

const VERSION: &str = "26.1.2";
const PROBE: &str = include_str!("../probe/Probe.java");
const PERF: &str = include_str!("../probe/Perf.java");
const AGENT: &str = include_str!("../probe/HideAgent.java");
const CONDITION: &str = include_str!("../probe/Condition.java");
const NO_RENDER_URL: &str = "https://github.com/headlesshq/headlessmc/releases/download/3.0.0-RC8/headlessmc-lwjgl-3.0.0-RC8-all.jar";
const NO_RENDER_SHA256: &str = "45c7a0041d580e7c80a190c49140e65c521e992af13229ca71795f3414146009";

/// Whether the probe of this build answers a verb.
#[cfg(test)]
pub fn probe_answers(verb: &str) -> bool {
    PROBE.contains(&format!("case \"{verb}\""))
        || PROBE.contains(&format!("verb.equals(\"{verb}\")"))
}

fn minecraft() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME"))
        .join("Library/Application Support/minecraft")
}

pub fn game_jar() -> PathBuf {
    minecraft().join(format!("versions/{VERSION}/{VERSION}.jar"))
}

pub fn java_home() -> PathBuf {
    minecraft().join(
        "runtime/java-runtime-epsilon/mac-os-arm64/java-runtime-epsilon/jre.bundle/Contents/Home",
    )
}

fn version_json() -> Result<Value, String> {
    let path = minecraft().join(format!("versions/{VERSION}/{VERSION}.json"));
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn allowed_on_this_mac(library: &Value) -> bool {
    let Some(rules) = library["rules"].as_array() else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        let os = &rule["os"];
        let name_matches = os["name"].as_str().is_none_or(|name| name == "osx");
        let arch_matches = os["arch"].as_str().is_none_or(|arch| arch == "arm64");
        if name_matches && arch_matches && rule["features"].is_null() {
            allowed = rule["action"] == "allow";
        }
    }
    allowed
}

fn class_path(version: &Value) -> Vec<String> {
    let root = minecraft();
    let mut paths = Vec::new();
    for library in version["libraries"].as_array().into_iter().flatten() {
        let Some(path) = library["downloads"]["artifact"]["path"].as_str() else {
            continue;
        };
        let full = root.join("libraries").join(path);
        if allowed_on_this_mac(library) && full.exists() {
            paths.push(full.to_string_lossy().into_owned());
        }
    }
    let game = root.join(format!("versions/{VERSION}/{VERSION}.jar"));
    paths.push(game.to_string_lossy().into_owned());
    paths
}

fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command.output().map_err(|e| format!("{what}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{what} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

/// Compiled once for each version of the sources, against the game jar itself.
fn build(classes: &[String]) -> Result<PathBuf, String> {
    let mut hasher = DefaultHasher::new();
    (PROBE, PERF, AGENT, CONDITION, VERSION).hash(&mut hasher);
    let dir = registry::home()
        .join("build")
        .join(format!("{:x}", hasher.finish()));
    if dir.join("hide.jar").exists() {
        return Ok(dir);
    }
    let staging = dir.with_extension(format!("tmp{}", std::process::id()));
    fs::create_dir_all(staging.join("src")).map_err(|e| e.to_string())?;
    fs::write(staging.join("src/Probe.java"), PROBE).map_err(|e| e.to_string())?;
    fs::write(staging.join("src/Perf.java"), PERF).map_err(|e| e.to_string())?;
    fs::write(staging.join("src/HideAgent.java"), AGENT).map_err(|e| e.to_string())?;
    fs::write(staging.join("src/Condition.java"), CONDITION).map_err(|e| e.to_string())?;
    let bin = java_home().join("bin");
    run(
        Command::new(bin.join("javac"))
            .args(["-nowarn", "-d", "classes", "-cp", &classes.join(":")])
            .args([
                "src/Probe.java",
                "src/Perf.java",
                "src/HideAgent.java",
                "src/Condition.java",
            ])
            .current_dir(&staging),
        "javac",
    )?;
    fs::write(staging.join("manifest.txt"), "Premain-Class: HideAgent\n")
        .map_err(|e| e.to_string())?;
    run(
        Command::new(bin.join("jar"))
            .args([
                "cfm",
                "hide.jar",
                "manifest.txt",
                "-C",
                "classes",
                "HideAgent.class",
            ])
            .current_dir(&staging),
        "jar",
    )?;
    if fs::rename(&staging, &dir).is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    Ok(dir)
}

fn no_render_agent() -> Result<PathBuf, String> {
    let path = registry::home().join("build/headlessmc-lwjgl.jar");
    if path.exists() {
        return Ok(path);
    }
    fs::create_dir_all(registry::home().join("build")).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("download");
    run(
        Command::new("curl")
            .args(["-sfL", "-o"])
            .arg(&temporary)
            .arg(NO_RENDER_URL),
        "downloading headlessmc-lwjgl",
    )?;
    let sum = Command::new("shasum")
        .args(["-a", "256"])
        .arg(&temporary)
        .output()
        .map_err(|e| e.to_string())?;
    if !String::from_utf8_lossy(&sum.stdout).starts_with(NO_RENDER_SHA256) {
        let _ = fs::remove_file(&temporary);
        return Err("headlessmc-lwjgl does not match its checksum".into());
    }
    fs::rename(temporary, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn nbt_string(text: &str) -> Vec<u8> {
    let mut out = (text.len() as u16).to_be_bytes().to_vec();
    out.extend_from_slice(text.as_bytes());
    out
}

fn nbt_string_tag(key: &str, value: &str) -> Vec<u8> {
    let mut out = vec![0x08];
    out.extend(nbt_string(key));
    out.extend(nbt_string(value));
    out
}

/// `acceptTextures` keeps the client from asking about the resource pack.
fn servers_dat(port: u16) -> Vec<u8> {
    let mut entry = nbt_string_tag("ip", &format!("127.0.0.1:{port}"));
    entry.extend(nbt_string_tag("name", "void"));
    entry.push(0x01);
    entry.extend(nbt_string("acceptTextures"));
    entry.extend([0x01, 0x00]);
    let mut out = vec![0x0A];
    out.extend(nbt_string(""));
    out.push(0x09);
    out.extend(nbt_string("servers"));
    out.push(0x0A);
    out.extend(1i32.to_be_bytes());
    out.extend(entry);
    out.push(0x00);
    out
}

fn options(level: Level, settings: &Settings, fps: u32) -> String {
    let sound = if level == Level::Play { 1 } else { 0 };
    format!(
        "onboardAccessibility:false\ninactivityFpsLimit:minimized\nguiScale:2\nfullscreen:false\nnarrator:0\nsoundCategory_master:{sound}\nrenderDistance:{}\nsimulationDistance:{}\nmaxFps:{fps}\nenableVsync:false\npauseOnLostFocus:false\ntutorialStep:none\njoinedFirstServer:true\n",
        settings.render_distance, settings.simulation_distance
    )
}

fn log_file(dir: &Path) -> Result<File, String> {
    File::create(dir.join("client.log")).map_err(|e| e.to_string())
}

fn idle_minutes(registry: &Registry) -> u32 {
    std::env::var("VOIDC_IDLE_MINUTES")
        .ok()
        .and_then(|minutes| minutes.parse().ok())
        .unwrap_or(registry.idle_minutes)
}

pub fn game(registry: &Registry, client: &Client) -> Result<Command, String> {
    let version = version_json()?;
    let mut classes = class_path(&version);
    let built = build(&classes)?;
    classes.push(built.join("classes").to_string_lossy().into_owned());

    let settings = registry.settings(client.level);
    let fps = client.fps.unwrap_or(settings.fps);
    let (width, height) = client.size.unwrap_or((settings.width, settings.height));
    let dir = registry::client_dir(&client.name);
    let game_dir = dir.join("mc");
    fs::create_dir_all(&game_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(registry::home().join("run")).map_err(|e| e.to_string())?;
    fs::write(game_dir.join("servers.dat"), servers_dat(client.port)).map_err(|e| e.to_string())?;
    fs::write(
        game_dir.join("options.txt"),
        options(client.level, settings, fps),
    )
    .map_err(|e| e.to_string())?;

    let server = format!("127.0.0.1:{}", client.port);
    let mut command = Command::new(java_home().join("bin/java"));
    command
        .arg("-XstartOnFirstThread")
        .arg(format!("-Xmx{}", settings.heap))
        .arg("--sun-misc-unsafe-memory-access=allow")
        .arg("--enable-native-access=ALL-UNNAMED")
        .arg(format!("-Dvoid.client={}", client.name))
        .arg(format!("-Dvoid.level={}", client.level.name()))
        .arg(format!("-Dvoid.version={}", crate::version::current()))
        .arg(format!(
            "-Dvoid.title={}",
            registry::title(client.level, &client.feature)
        ))
        .arg(format!("-Dvoid.server={server}"))
        .arg(format!(
            "-Dvoid.socket={}",
            registry::socket(&client.name)?.display()
        ))
        .arg(format!("-Dvoid.idle.minutes={}", idle_minutes(registry)));
    match client.level {
        Level::Render => {
            command
                .arg("-XX:+UseSerialGC")
                .arg(format!("-javaagent:{}", built.join("hide.jar").display()));
        }
        Level::Logic => {
            command
                .arg("-XX:+UseSerialGC")
                .arg(format!("-javaagent:{}", no_render_agent()?.display()));
        }
        Level::Play | Level::Bot => {}
    }
    if client.jfr {
        command.arg(format!(
            "-XX:StartFlightRecording=name=client,settings=profile,maxage=30m,disk=true,dumponexit=true,filename={}",
            dir.join("client.jfr").display()
        ));
    }
    let uuid = format!("{:032x}", {
        let mut hasher = DefaultHasher::new();
        client.name.hash(&mut hasher);
        hasher.finish() as u128
    });
    command
        .args(["-cp", &classes.join(":"), "Probe"])
        .args([
            "--username",
            &client.name.chars().take(16).collect::<String>(),
        ])
        .args(["--version", VERSION, "--gameDir"])
        .arg(&game_dir)
        .arg("--assetsDir")
        .arg(minecraft().join("assets"))
        .args([
            "--assetIndex",
            version["assetIndex"]["id"].as_str().unwrap_or_default(),
        ])
        .args([
            "--uuid",
            &uuid,
            "--accessToken",
            "0",
            "--userType",
            "legacy",
            "--versionType",
            "release",
        ])
        .args([
            "--width",
            &width.to_string(),
            "--height",
            &height.to_string(),
        ])
        .args([
            "--quickPlayPath",
            "qp.json",
            "--quickPlayMultiplayer",
            &server,
        ]);
    Ok(command)
}

pub fn bot(registry: &Registry, client: &Client) -> Result<Command, String> {
    let dir = registry::client_dir(&client.name);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(registry::home().join("run")).map_err(|e| e.to_string())?;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command.args([
        "bot-run",
        &client.name,
        &client.port.to_string(),
        &idle_minutes(registry).to_string(),
    ]);
    Ok(command)
}

pub fn spawn(mut command: Command, dir: &Path) -> Result<std::process::Child, String> {
    use std::os::unix::process::CommandExt;
    let log = log_file(dir)?;
    let error = log.try_clone().map_err(|e| e.to_string())?;
    command
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(error)
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stepping_of_the_probe_holds_on_a_channel() {
        let Ok(version) = version_json() else {
            eprintln!("no game installed: skipped");
            return;
        };
        let mut classes = class_path(&version);
        let built = build(&classes).unwrap();
        classes.push(built.join("classes").to_string_lossy().into_owned());
        let dir = std::env::temp_dir().join(format!("voidc-stepping-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("probe/SteppingTest.java");
        run(
            Command::new(java_home().join("bin/javac"))
                .args(["-nowarn", "-d"])
                .arg(&dir)
                .args(["-cp", &classes.join(":")])
                .arg(source),
            "javac",
        )
        .unwrap();
        classes.push(dir.to_string_lossy().into_owned());
        let result = run(
            Command::new(java_home().join("bin/java")).args([
                "-cp",
                &classes.join(":"),
                "SteppingTest",
            ]),
            "SteppingTest",
        );
        fs::remove_dir_all(dir).unwrap();
        result.unwrap();
    }

    #[test]
    fn servers_dat_is_a_list_of_one_server_that_accepts_the_pack() {
        let bytes = servers_dat(25680);
        assert_eq!(&bytes[..3], &[0x0A, 0, 0]);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("127.0.0.1:25680"));
        assert!(text.contains("acceptTextures"));
        assert_eq!(*bytes.last().unwrap(), 0);
    }

    #[test]
    fn a_hidden_client_stays_silent_and_a_played_one_does_not() {
        let settings = Registry::load();
        let hidden = options(Level::Render, settings.settings(Level::Render), 40);
        assert!(hidden.contains("soundCategory_master:0"));
        assert!(hidden.contains("inactivityFpsLimit:minimized"));
        assert!(hidden.contains("maxFps:40"));
        let played = options(Level::Play, settings.settings(Level::Play), 120);
        assert!(played.contains("soundCategory_master:1"));
    }

    #[test]
    fn libraries_for_other_systems_are_left_out() {
        let linux: Value =
            serde_json::json!({"rules": [{"action": "allow", "os": {"name": "linux"}}]});
        let mac: Value = serde_json::json!({"rules": [{"action": "allow", "os": {"name": "osx"}}]});
        assert!(!allowed_on_this_mac(&linux));
        assert!(allowed_on_this_mac(&mac));
        assert!(allowed_on_this_mac(&serde_json::json!({})));
    }
}
