mod parse;
mod report;
mod run;
mod validate;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use parse::Scenario;
pub use run::{Driver, LiveClient, Run};

use crate::control::Limits;
use crate::registry::{self, Level};
use crate::{args, lifecycle};

fn default_out(client: &str, path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    registry::client_dir(client)
        .join("runs")
        .join(format!("{stem}-{seconds}"))
}

/// Where the clients come from, so that a whole `voidc run` can be tested without a game.
pub trait Host {
    type Driver: Driver;
    /// The level of a client, and whether it is running.
    fn find(&self, name: &str) -> Option<(Level, bool)>;
    fn up(&mut self, feature: &str, level: Level, port: Option<String>) -> Result<(), String>;
    fn down(&mut self, name: &str) -> Result<(), String>;
    fn driver(&self, name: &str) -> Result<Self::Driver, String>;
}

pub struct Live<F>(pub F, pub Limits);

impl<F> Host for Live<F>
where
    F: FnMut(&str, Level, Option<String>) -> Result<(), String>,
{
    type Driver = LiveClient;

    fn find(&self, name: &str) -> Option<(Level, bool)> {
        registry::load(name).map(|client| (client.level, registry::alive(&client)))
    }

    fn up(&mut self, feature: &str, level: Level, port: Option<String>) -> Result<(), String> {
        (self.0)(feature, level, port)
    }

    fn down(&mut self, name: &str) -> Result<(), String> {
        lifecycle::down(name)
    }

    fn driver(&self, name: &str) -> Result<LiveClient, String> {
        LiveClient::open(name, self.1)
    }
}

/// Which client plays, and whether this run brings it up.
struct Target {
    name: String,
    level: Level,
    feature: Option<String>,
}

fn target(args: &[String], scenario: &Scenario, host: &impl Host) -> Result<Target, String> {
    let wanted = match args::flag(args, "--level") {
        Some(level) => Some(Level::parse(&level)?),
        None => scenario.level,
    };
    if let Some(name) = args::flag(args, "--on") {
        let (level, running) = host
            .find(&name)
            .ok_or_else(|| format!("unknown client {name}"))?;
        if level == Level::Play {
            return Err(format!("{name} is played by Léo and takes no command"));
        }
        if !running {
            return Err(format!("{name} is not running"));
        }
        if let Some(wanted) = wanted.filter(|wanted| *wanted != level) {
            return Err(format!(
                "{name} is a {} client and the scenario wants {}",
                level.name(),
                wanted.name()
            ));
        }
        return Ok(Target {
            name,
            level,
            feature: None,
        });
    }
    let feature = args::flag(args, "--feature")
        .ok_or("which client? --on <name> or --feature \"Feature\"")?;
    let level = wanted.unwrap_or(Level::Render);
    if level == Level::Play {
        return Err("a client to play takes no scenario".into());
    }
    let name = registry::slug(&feature)?;
    if host.find(&name).is_some_and(|(_, running)| running) {
        return Err(format!("{name} is already running: use --on {name}"));
    }
    Ok(Target {
        name,
        level,
        feature: Some(feature),
    })
}

/// `voidc run <file> ...`: a failed line, or a refused scenario, exits with 1.
pub fn command(args: &[String], host: &mut impl Host) -> Result<(), String> {
    let path = PathBuf::from(args.first().ok_or(
        "voidc run <file> (--on <name> | --feature \"Feature\" [--port N]) [--level L] [--keep-going] [--keep-up] [--out DIR]",
    )?);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let scenario = Scenario::parse(&text)?;
    let chosen = target(args, &scenario, host)?;
    let base = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let base = if base.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        base
    };
    scenario.validate(chosen.level, &base)?;
    if let Some(feature) = &chosen.feature
        && let Err(error) = host.up(feature, chosen.level, args::flag(args, "--port"))
    {
        return Err(lifecycle::with_stop(error, host.down(&chosen.name)));
    }
    let out =
        args::flag(args, "--out").map_or_else(|| default_out(&chosen.name, &path), PathBuf::from);
    let run = Run::new(
        &chosen.name,
        chosen.level,
        base,
        out.clone(),
        args::has(args, "--keep-going"),
    );
    let played = host
        .driver(&chosen.name)
        .and_then(|mut driver| run.play(&scenario, &mut driver))
        .and_then(|report| report.write(&out).map(|()| report));
    let stopped = if chosen.feature.is_some() && !args::has(args, "--keep-up") {
        host.down(&chosen.name)
    } else {
        Ok(())
    };
    let report = match played {
        Ok(report) => report,
        Err(error) => return Err(lifecycle::with_stop(error, stopped)),
    };
    println!("{}", report.verdict());
    println!("report: {}", out.join("report.txt").display());
    stopped?;
    if report.passed {
        Ok(())
    } else {
        Err(String::new())
    }
}

#[cfg(test)]
mod tests;
