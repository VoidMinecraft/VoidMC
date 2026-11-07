use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::parse::*;
use super::run::*;
use super::validate::*;
use super::*;
use crate::condition::{self, DEFAULT_WAIT_MS};
use crate::control::fake::{FakeClient, NAME, Reply};
use crate::control::{self, Ask, Failure, Limits};
use crate::{check, compare, version};

mod command;
mod live;
mod play;
mod validation;

struct Fake {
    state: Value,
    replies: VecDeque<Value>,
    asked: Vec<String>,
    dir: PathBuf,
    moving: bool,
    frames: String,
    finishes: bool,
    stale_when_asked: Vec<bool>,
    log: Vec<String>,
    collected_when_released: Vec<bool>,
    untold: bool,
    gone_at_release: bool,
    rewritten_after_read: bool,
}

impl Fake {
    fn new(dir: &Path, replies: Vec<Value>) -> Self {
        Self {
            state: json!({
                "ok": true, "state": "connected", "x": 0.0, "chunks": 71, "camera": "player", "clock": "real",
                "chat": "", "cam_x": 1.0, "cam_y": 2.0, "cam_z": 3.0, "cam_yaw": 0.0, "cam_pitch": 0.0,
            }),
            replies: replies.into(),
            asked: Vec::new(),
            dir: dir.to_path_buf(),
            moving: false,
            frames: String::new(),
            finishes: true,
            stale_when_asked: Vec::new(),
            log: Vec::new(),
            collected_when_released: Vec::new(),
            untold: false,
            gone_at_release: false,
            rewritten_after_read: false,
        }
    }
}

impl Ask for Fake {
    fn ask(&mut self, verb: &str) -> Result<Value, Failure> {
        self.log.push(verb.to_string());
        if verb == "state" {
            if self.moving {
                self.state["cam_x"] = json!(self.state["cam_x"].as_f64().unwrap() + 1.0);
            }
            return control::judged(self.state.clone());
        }
        self.asked.push(verb.to_string());
        if verb.starts_with("frames") {
            let path = self.recording("frames");
            self.stale_when_asked.push(path.exists());
            fs::write(&path, &self.frames).unwrap();
            if self.finishes {
                fs::write(path.with_extension("done"), "").unwrap();
            }
        }
        if verb == "clock real" && self.gone_at_release {
            return Err(Failure::Lost("connection refused".into()));
        }
        if verb == "clock real" {
            let collected = self.dir.join("out/frames.txt").exists();
            self.collected_when_released.push(collected);
        }
        let reply = self.replies.pop_front().unwrap_or(json!({"ok": true}));
        if let (Some(mode), true) = (verb.strip_prefix("clock "), reply["ok"] == true)
            && !self.untold
        {
            self.state["clock"] = json!(mode);
        }
        control::judged(reply)
    }
}

impl Driver for Fake {
    fn give_back_clock(&mut self) -> Result<Value, Failure> {
        self.ask("clock real")
    }

    fn recording(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.txt"))
    }

    fn read(&self, path: &Path) -> Result<String, String> {
        let text = check::read(path)?;
        if self.rewritten_after_read {
            fs::write(path, "rewritten\n").unwrap();
        }
        Ok(text)
    }
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voidc-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run<'a>(dir: &Path, keep_going: bool) -> Run<'a> {
    let mut run = Run::new(
        "test",
        Level::Render,
        dir.to_path_buf(),
        dir.join("out"),
        keep_going,
    );
    run.patience = Duration::from_millis(100);
    run
}

struct Stage {
    dir: PathBuf,
    clients: BTreeMap<String, (Level, bool)>,
    up: Result<(), String>,
    driver: Result<(), String>,
    down: Result<(), String>,
    ups: Vec<String>,
    downs: Vec<String>,
}

impl Stage {
    fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            clients: BTreeMap::new(),
            up: Ok(()),
            driver: Ok(()),
            down: Ok(()),
            ups: Vec::new(),
            downs: Vec::new(),
        }
    }
}

impl Host for Stage {
    type Driver = Fake;

    fn find(&self, name: &str) -> Option<(Level, bool)> {
        self.clients.get(name).copied()
    }

    fn up(&mut self, feature: &str, _: Level, _: Option<String>) -> Result<(), String> {
        self.ups.push(feature.to_string());
        self.up.clone()
    }

    fn down(&mut self, name: &str) -> Result<(), String> {
        self.downs.push(name.to_string());
        self.down.clone()
    }

    fn driver(&self, _: &str) -> Result<Fake, String> {
        self.driver.clone().map(|()| Fake::new(&self.dir, vec![]))
    }
}

fn words(dir: &Path, more: &[&str]) -> Vec<String> {
    let mut args = vec![
        dir.join("s.scn").display().to_string(),
        "--out".to_string(),
        dir.join("out").display().to_string(),
    ];
    args.extend(more.iter().map(|word| word.to_string()));
    args
}

fn staged(name: &str, text: &str) -> PathBuf {
    let dir = folder(name);
    fs::write(dir.join("s.scn"), text).unwrap();
    dir
}
