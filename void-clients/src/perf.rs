use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use crate::compare::{self, Picture, Rules};
use crate::lifecycle;
use crate::registry::{self, Level};
use crate::scenario::{Driver, Socket, reply_error};

const NOISE_MS: f64 = 0.05;
const MIN_ROUNDS: usize = 6;
const TOP_SECTIONS: usize = 14;
const TOP_PACKETS: usize = 10;
const GPU_NOTE: &str = "A GPU figure only means something on a frozen scene, on a calm machine, and against another figure taken in turns.";

struct Options {
    seconds: u64,
    frames: u32,
    rounds: u32,
    count: u32,
}

fn number<T: std::str::FromStr + Default + PartialEq>(
    args: &[String],
    name: &str,
    default: T,
) -> Result<T, String> {
    match args.iter().position(|arg| arg == name) {
        None => Ok(default),
        Some(at) => args
            .get(at + 1)
            .and_then(|value| value.parse().ok())
            .filter(|value| *value != T::default())
            .ok_or_else(|| format!("bad {name}: a positive number")),
    }
}

/// What is left of the arguments without the options and their values.
fn positional(args: &[String]) -> Vec<&str> {
    let mut words = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg.starts_with("--") {
            args.next();
        } else {
            words.push(arg.as_str());
        }
    }
    words
}

fn ask(socket: &mut Socket, verb: &str) -> Result<Value, String> {
    let reply = socket.ask(verb)?;
    match reply_error(&reply) {
        Some(error) => Err(error),
        None => Ok(reply),
    }
}

fn millis(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .map(|ns| ns / 1e6)
        .collect()
}

fn sorted(values: &[f64]) -> Vec<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted
}

/// No figure of nothing: a measure without samples has no median, least of all 0 ms.
fn median(values: &[f64]) -> Option<f64> {
    let sorted = sorted(values);
    let middle = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        count if count % 2 == 1 => Some(sorted[middle]),
        _ => Some((sorted[middle - 1] + sorted[middle]) / 2.0),
    }
}

/// The smallest sample at or above that share of them.
fn percentile(values: &[f64], share: f64) -> Option<f64> {
    let sorted = sorted(values);
    let rank = (sorted.len() as f64 * share).ceil() as usize;
    sorted.get(rank.saturating_sub(1)).copied()
}

fn figure(value: Option<f64>) -> String {
    value.map_or_else(|| "no figure".to_string(), |value| format!("{value:.3} ms"))
}

/// What makes a figure worth less than it looks, in words.
fn warnings(reply: &Value, others: usize) -> Vec<String> {
    let mut notes = Vec::new();
    if reply["settled"] == false {
        notes.push("the sections never settled: chunks were still being built".to_string());
    }
    if reply["still"] == false {
        notes.push("the camera or the sections changed during the measure".to_string());
    }
    if let Some(moved) = reply["moved_entities"].as_u64().filter(|moved| *moved > 0) {
        notes.push(format!(
            "{moved} entities still moved, by packets of the server: the scene is not frozen"
        ));
    }
    if let Some(reloads) = reply["reloads"].as_u64().filter(|reloads| *reloads > 0) {
        notes.push(format!("{reloads} windows saw chunks being rebuilt"));
    }
    if others > 0 {
        notes.push(format!("{others} other clients share the GPU"));
    }
    notes
}

fn gpu_report(name: &str, reply: &Value, others: usize) -> (String, Value) {
    let times: Vec<f64> = millis(&reply["ns"])
        .into_iter()
        .filter(|time| *time > 0.0)
        .collect();
    let (middle, high) = (median(&times), percentile(&times, 0.95));
    let notes = warnings(reply, others);
    let mut text = match middle {
        Some(_) => format!(
            "GPU, means of 8 frames, entities stopped, no frame limit: median {}, 95th percentile {}, {} samples, {} sections\n{GPU_NOTE}\n",
            figure(middle),
            figure(high),
            times.len(),
            reply["sections"]
        ),
        None => "GPU: no sample came back, so there is no figure\n".to_string(),
    };
    for note in &notes {
        text.push_str(&format!("  WARNING {note}\n"));
    }
    text.push_str(&format!(
        "Compare with: voidc perf {name} ab clouds, voidc perf {name} ab particles\n"
    ));
    let json = json!({
        "valid": middle.is_some(),
        "median_ms": middle,
        "p95_ms": high,
        "samples": times.len(),
        "sections": reply["sections"],
        "warnings": notes,
    });
    (text, json)
}

/// The ranks around the median that hold it 95 % of the time, by the sign of each round: it
/// assumes nothing of their spread, and one wild round moves it by one rank. `None` under six
/// rounds, where even the two ends are not sure enough.
fn interval(gaps: &[f64]) -> Option<(f64, f64)> {
    let sorted = sorted(gaps);
    let count = sorted.len();
    let chance = |outside: usize| -> f64 {
        let mut ways = 1.0;
        let mut below = 0.0;
        for rank in 0..outside {
            below += ways;
            ways *= (count - rank) as f64 / (rank + 1) as f64;
        }
        2.0 * below / 2f64.powi(count as i32)
    };
    let outside = (1..count / 2)
        .take_while(|outside| chance(*outside) <= 0.05)
        .last()?;
    Some((sorted[outside - 1], sorted[count - outside]))
}

/// A setting that changes nothing on screen costs nothing, whatever it would cost elsewhere.
fn drawn_difference(reply: &Value) -> Option<u64> {
    let shots = reply["shots"].as_array()?;
    let load = |at: usize| Picture::load(Path::new(shots.get(at)?.as_str()?)).ok();
    let rules = Rules::default();
    let (outcome, _) = compare::compare(&load(0)?, &load(1)?, &rules).ok()?;
    Some(outcome.pixels)
}

/// A figure without its uncertainty is how the study got 1.2 and 6.6 ms for the same clouds.
fn ab_report(reply: &Value, others: usize) -> (String, Value) {
    let (all_a, all_b) = (millis(&reply["a_ns"]), millis(&reply["b_ns"]));
    let pairs: Vec<(f64, f64)> = all_a
        .iter()
        .zip(&all_b)
        .filter(|(a, b)| **a > 0.0 && **b > 0.0)
        .map(|(a, b)| (*a, *b))
        .collect();
    let empty = all_a.len().max(all_b.len()) - pairs.len();
    let a: Vec<f64> = pairs.iter().map(|pair| pair.0).collect();
    let b: Vec<f64> = pairs.iter().map(|pair| pair.1).collect();
    let gaps: Vec<f64> = pairs.iter().map(|(a, b)| b - a).collect();
    let gap = median(&gaps);
    let bounds = interval(&gaps);
    let reloads = reply["reloads"].as_u64().unwrap_or(0);
    let valid = empty == 0 && !gaps.is_empty() && reloads == 0;
    let verdict = if gaps.is_empty() || empty > 0 {
        format!("not valid: {empty} rounds had no sample")
    } else if reloads > 0 {
        "not valid: chunks were rebuilt during the measure".to_string()
    } else if let Some((low, high)) = bounds {
        if low > NOISE_MS || high < -NOISE_MS {
            "a real difference".to_string()
        } else {
            format!(
                "no difference found: under {:.3} ms",
                low.abs().max(high.abs()).max(NOISE_MS)
            )
        }
    } else {
        format!("too few rounds to say anything, take at least {MIN_ROUNDS}")
    };
    let mut notes = warnings(reply, others);
    let drawn = drawn_difference(reply);
    if drawn == Some(0) {
        notes.push("the two sides draw the same picture: this view has nothing to measure".into());
    }
    let (label_a, label_b) = (
        reply["a"].as_str().unwrap_or("A"),
        reply["b"].as_str().unwrap_or("B"),
    );
    let mut text = format!(
        "GPU, in turns over {} rounds:\n  A {label_a}: {}\n  B {label_b}: {}\n  B - A: {}{}\n  {verdict}\n",
        gaps.len(),
        figure(median(&a)),
        figure(median(&b)),
        gap.map_or_else(|| "no figure".to_string(), |gap| format!("{gap:+.3} ms")),
        bounds.map_or_else(String::new, |(low, high)| format!(
            ", 95 % between {low:+.3} and {high:+.3}"
        )),
    );
    if let Some(drawn) = drawn {
        text.push_str(&format!("  the two sides differ by {drawn} pixels\n"));
    }
    if let Some(count) = reply["particles"].as_u64() {
        text.push_str(&format!("  {count} particles were alive on side B\n"));
    }
    for note in &notes {
        text.push_str(&format!("  WARNING {note}\n"));
    }
    let json = json!({
        "a": label_a,
        "b": label_b,
        "a_ms": a,
        "b_ms": b,
        "valid": valid,
        "difference_ms": gap,
        "interval_ms": bounds.map(|(low, high)| [low, high]),
        "verdict": verdict,
        "drawn_difference_px": drawn,
        "reloads": reply["reloads"],
        "particles": reply["particles"],
        "warnings": notes,
    });
    (text, json)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub path: String,
    pub own_share: f64,
    pub total_share: f64,
}

/// Reads the tree of `profiling.txt`: `[04] |   | main(40/3189) - 66.73%/27.05%`.
pub fn sections(text: &str) -> Vec<Section> {
    let mut stack: Vec<String> = Vec::new();
    let mut found = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((depth, tree)) = rest.split_once(']') else {
            continue;
        };
        let Ok(depth) = depth.trim().parse::<usize>() else {
            continue;
        };
        let body = tree.trim_start_matches([' ', '|']);
        let Some((label, shares)) = body.rsplit_once(" - ") else {
            continue;
        };
        let Some((own, total)) = shares.trim().split_once('/') else {
            continue;
        };
        let (Ok(own), Ok(total)) = (
            own.trim_end_matches('%').parse(),
            total.trim_end_matches('%').parse(),
        ) else {
            continue;
        };
        let name = label.split('(').next().unwrap_or(label).trim();
        stack.truncate(depth);
        stack.push(name.to_string());
        found.push(Section {
            path: stack.join("/"),
            own_share: own,
            total_share: total,
        });
    }
    found
}

fn unzip(archive: &Path, wanted: &str) -> Result<String, String> {
    let output = Command::new("unzip")
        .arg("-p")
        .arg(archive)
        .arg(wanted)
        .output()
        .map_err(|e| format!("unzip: {e}"))?;
    if !output.status.success() {
        return Err(format!("{wanted} is not in {}", archive.display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn metrics_report(reply: &Value) -> Result<(String, Value), String> {
    let archive = PathBuf::from(reply["path"].as_str().unwrap_or_default());
    let profile = unzip(&archive, "client/profiling.txt")?;
    let mut all = sections(&profile);
    if all.is_empty() {
        return Err(format!("no section in {}", archive.display()));
    }
    all.sort_by(|a, b| b.total_share.total_cmp(&a.total_share));
    let mut text = format!(
        "CPU by section, F3+L over {} s, share of all the time. CPU submission, not GPU time:\n",
        reply["seconds"]
    );
    for section in all.iter().take(TOP_SECTIONS) {
        text.push_str(&format!(
            "  {:>6.2} %  {}\n",
            section.total_share, section.path
        ));
    }
    text.push_str(&format!("  archive: {}\n", archive.display()));
    let rows: Vec<Value> = all
        .iter()
        .map(|section| json!({"section": section.path, "share": section.total_share}))
        .collect();
    Ok((text, json!({"archive": archive, "sections": rows})))
}

fn packet_rows(value: &Value) -> Vec<(String, u64, u64)> {
    let mut rows: Vec<(String, u64, u64)> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some((
                row[0].as_str()?.to_string(),
                row[1].as_u64()?,
                row[2].as_u64()?,
            ))
        })
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.2));
    rows
}

fn jfr_report(reply: &Value) -> (String, Value) {
    let mut text = format!("Packets, JFR over {} s:\n", reply["seconds"]);
    let mut json = BTreeMap::new();
    for (side, key) in [("received", "received"), ("sent", "sent")] {
        let rows = packet_rows(&reply[key]);
        let count: u64 = rows.iter().map(|row| row.1).sum();
        let bytes: u64 = rows.iter().map(|row| row.2).sum();
        text.push_str(&format!("  {side}: {count} packets, {bytes} bytes\n"));
        for (name, count, bytes) in rows.iter().take(TOP_PACKETS) {
            text.push_str(&format!("    {bytes:>10} B {count:>7}  {name}\n"));
        }
        json.insert(
            side,
            json!({"packets": count, "bytes": bytes, "rows": reply[key]}),
        );
    }
    text.push_str(&format!(
        "  recording: {}\n",
        reply["path"].as_str().unwrap_or("")
    ));
    let json = json!({"recording": reply["path"], "packets": json});
    (text, json)
}

/// What else draws on the GPU: neither a bot nor the logic level has anything to draw.
fn other_clients(name: &str) -> usize {
    lifecycle::running()
        .iter()
        .filter(|client| client.name != name)
        .filter(|client| matches!(client.level, Level::Render | Level::Play))
        .count()
}

fn write(name: &str, report: &str, json: &Value) -> Result<PathBuf, String> {
    let dir = registry::client_dir(name).join("perf");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("report.txt"), report).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(json).map_err(|e| e.to_string())?;
    fs::write(dir.join("report.json"), text).map_err(|e| e.to_string())?;
    Ok(dir)
}

const USAGE: &str = "voidc perf <name> [all|metrics|gpu|jfr|ab clouds|ab particles] [--seconds N] [--frames N] [--rounds N] [--count N]";

/// What `voidc perf` was asked for, known before anything reaches the client.
enum Measure<'a> {
    All,
    Metrics,
    Gpu,
    Jfr,
    Ab(&'a str),
}

impl<'a> Measure<'a> {
    fn parse(words: &[&'a str]) -> Result<Self, String> {
        match words {
            [] | ["all", ..] => Ok(Self::All),
            ["metrics", ..] => Ok(Self::Metrics),
            ["gpu", ..] => Ok(Self::Gpu),
            ["jfr", ..] => Ok(Self::Jfr),
            ["ab", setting, ..] => Ok(Self::Ab(setting)),
            _ => Err(USAGE.to_string()),
        }
    }
}

/// `voidc perf <name> ...`: runs in turn what was asked and writes one report.
pub fn command(args: &[String]) -> Result<(), String> {
    let (name, rest) = args.split_first().ok_or(USAGE)?;
    let client = registry::load(name).ok_or_else(|| format!("unknown client {name}"))?;
    if !matches!(client.level, Level::Render | Level::Logic) {
        return Err(format!(
            "{name} is a {} client: perf needs a game",
            client.level.name()
        ));
    }
    let options = Options {
        seconds: number(rest, "--seconds", 10)?,
        frames: number(rest, "--frames", 64)?,
        rounds: number(rest, "--rounds", 12)?,
        count: number(rest, "--count", 2000)?,
    };
    let words = positional(rest);
    let measure = Measure::parse(&words)?;
    let others = other_clients(name);
    let mut report = String::new();
    let mut json = BTreeMap::new();
    let gpu = |socket: &mut Socket,
               report: &mut String,
               json: &mut BTreeMap<&str, Value>|
     -> Result<(), String> {
        let reply = ask(socket, &format!("perf gpu {}", options.frames))?;
        let (text, value) = gpu_report(name, &reply, others);
        report.push_str(&text);
        json.insert("gpu", value);
        Ok(())
    };
    let metrics = |socket: &mut Socket,
                   report: &mut String,
                   json: &mut BTreeMap<&str, Value>|
     -> Result<(), String> {
        let reply = ask(socket, &format!("perf metrics {}", options.seconds))?;
        let (text, value) = metrics_report(&reply)?;
        report.push_str(&text);
        json.insert("metrics", value);
        Ok(())
    };
    let jfr = |socket: &mut Socket,
               report: &mut String,
               json: &mut BTreeMap<&str, Value>|
     -> Result<(), String> {
        let reply = ask(socket, &format!("perf jfr {}", options.seconds))?;
        let (text, value) = jfr_report(&reply);
        report.push_str(&text);
        json.insert("jfr", value);
        Ok(())
    };
    let mut socket = Socket::open(name)?;
    match measure {
        Measure::Gpu => gpu(&mut socket, &mut report, &mut json)?,
        Measure::Metrics => metrics(&mut socket, &mut report, &mut json)?,
        Measure::Jfr => jfr(&mut socket, &mut report, &mut json)?,
        Measure::Ab(setting) => {
            let reply = ask(
                &mut socket,
                &format!(
                    "perf ab {setting} {} {} {}",
                    options.rounds, options.frames, options.count
                ),
            )?;
            let (text, value) = ab_report(&reply, others);
            report.push_str(&text);
            json.insert("ab", value);
        }
        Measure::All if client.level == Level::Logic => {
            metrics(&mut socket, &mut report, &mut json)?;
            jfr(&mut socket, &mut report, &mut json)?;
        }
        Measure::All => {
            metrics(&mut socket, &mut report, &mut json)?;
            gpu(&mut socket, &mut report, &mut json)?;
            jfr(&mut socket, &mut report, &mut json)?;
        }
    }
    let invalid = json.values().any(|part| part["valid"] == false);
    let dir = write(name, &report, &json!(json))?;
    print!("{report}");
    println!("report: {}", dir.join("report.txt").display());
    if invalid {
        Err("this measure is not valid".into())
    } else {
        Ok(())
    }
}

/// How long a `perf` verb may take: a window of frames can run at 10 images per second.
pub fn timeout_of(words: &[&str]) -> Duration {
    let at = |index: usize| words.get(index).and_then(|word| word.parse::<u64>().ok());
    let seconds = match words.get(1).copied() {
        Some("gpu") => at(2).unwrap_or(120) / 5,
        Some("ab") => at(3).unwrap_or(8) * 2 * (at(4).unwrap_or(120) / 5 + 4),
        _ => at(2).unwrap_or(10),
    };
    Duration::from_secs(seconds + 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::fake::{FakeClient, NAME, Reply};

    #[test]
    fn perf_refuses_a_probe_of_another_version() {
        let fake = FakeClient::stale(Level::Render);
        let args = [NAME, "gpu"].map(String::from);
        fake.assert_refused(command(&args));
    }

    #[test]
    fn a_bad_measure_gets_the_usage_before_the_client_is_opened() {
        let fake = FakeClient::stale(Level::Render);
        for wrong in [&[NAME, "bogus"][..], &[NAME, "ab"]] {
            let args = wrong.iter().map(ToString::to_string).collect::<Vec<_>>();
            assert_eq!(command(&args), Err(USAGE.to_string()));
        }
        assert!(fake.heard().is_empty(), "{:?}", fake.heard());
    }

    fn profiling_archive(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir.join("client")).unwrap();
        fs::write(
            dir.join("client/profiling.txt"),
            "[00] frame(2/1) - 90.00%/90.00%\n[01] |   render(2/1) - 10.00%/9.00%\n",
        )
        .unwrap();
        let zipped = Command::new("zip")
            .args(["-qr", "run.zip", "client"])
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(zipped.success());
        dir.join("run.zip")
    }

    #[test]
    fn every_measure_of_a_report_goes_down_one_checked_connection() {
        let dir = std::env::temp_dir().join(format!("voidc-one-socket-{}", std::process::id()));
        let archive = profiling_archive(&dir);
        let fake = FakeClient::scripted(
            Level::Logic,
            Box::new(move |_| {
                Reply::current(
                    Duration::ZERO,
                    json!({"ok": true, "path": archive, "seconds": 10}),
                )
            }),
        );
        command(&[NAME.to_string()]).unwrap();
        assert_eq!(fake.heard(), ["state", "perf metrics 10", "perf jfr 10"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_tree_of_profiling_txt_gives_full_paths() {
        let text = "--- BEGIN ---\n\
            [00] root(3189/1) - 100.00%/100.00%\n\
            [01] |   render(3189/1) - 52.79%/52.79%\n\
            [02] |   |   main(3189/1) - 76.21%/40.23%\n\
            [03] |   |   |   renderTranslucentFeatures(3189/1) - 66.73%/26.85%\n\
            [01] |   gui(3189/1) - 2.51%/2.51%\n\
            noise\n";
        let found = sections(text);
        assert_eq!(found.len(), 5);
        assert_eq!(found[3].path, "root/render/main/renderTranslucentFeatures");
        assert_eq!(found[3].total_share, 26.85);
        assert_eq!(found[4].path, "root/gui");
    }

    #[test]
    fn a_gpu_figure_says_how_little_it_means_alone() {
        let reply = json!({"ns": [1_000_000, 2_000_000, 3_000_000], "sections": 40, "settled": true, "still": true, "moved_entities": 0});
        let (text, json) = gpu_report("carte", &reply, 0);
        assert_eq!(json["median_ms"], 2.0);
        assert_eq!(json["p95_ms"], 3.0);
        assert!(text.contains("frozen scene"));
        assert!(text.contains("voidc perf carte ab clouds"));
        assert!(!text.contains("WARNING"));
    }

    #[test]
    fn a_median_of_an_even_count_lies_between_its_middles() {
        assert_eq!(median(&[1.0, 2.0, 3.0, 10.0]), Some(2.5));
        assert_eq!(median(&[]), None);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.5), Some(2.0));
        assert_eq!(percentile(&[], 0.5), None);
    }

    #[test]
    fn the_interval_widens_with_few_rounds_and_needs_six() {
        let gaps = |count: usize| (0..count).map(|gap| gap as f64).collect::<Vec<_>>();
        assert_eq!(interval(&gaps(5)), None);
        assert_eq!(interval(&gaps(6)), Some((0.0, 5.0)));
        assert_eq!(interval(&gaps(12)), Some((2.0, 9.0)));
    }

    #[test]
    fn options_do_not_read_as_what_to_measure() {
        let args: Vec<String> = ["--seconds", "6", "ab", "--count", "4", "clouds"]
            .map(str::to_string)
            .to_vec();
        assert_eq!(positional(&args), ["ab", "clouds"]);
        assert!(positional(&["--seconds".to_string(), "6".to_string()]).is_empty());
        assert_eq!(number(&args, "--seconds", 10u64), Ok(6));
        let zero = ["--rounds".to_string(), "0".to_string()];
        assert!(number(&zero, "--rounds", 12u32).is_err());
    }

    #[test]
    fn metrics_read_the_sections_of_a_real_archive() {
        let dir = std::env::temp_dir().join(format!("voidc-metrics-{}", std::process::id()));
        let reply = json!({"path": profiling_archive(&dir), "seconds": 6});
        let (text, json) = metrics_report(&reply).unwrap();
        assert!(text.contains("frame/render"), "{text}");
        assert_eq!(json["sections"][1]["share"], 9.0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn metrics_without_an_archive_are_an_error() {
        assert!(metrics_report(&json!({"path": "/no/such/file.zip"})).is_err());
    }

    #[test]
    fn jfr_sums_each_side_and_ranks_by_bytes() {
        let reply = json!({
            "path": "/tmp/a.jfr",
            "seconds": 8,
            "received": [["small", 5, 10], ["large", 1, 900]],
            "sent": [["tick", 3, 3]],
        });
        let (text, json) = jfr_report(&reply);
        assert!(text.find("large").unwrap() < text.find("small").unwrap());
        assert!(text.contains("received: 6 packets, 910 bytes"), "{text}");
        assert_eq!(json["packets"]["sent"]["packets"], 3);
    }

    #[test]
    fn what_spoils_a_figure_is_said() {
        let reply = json!({"ns": [1_000_000], "sections": 40, "settled": false, "still": false, "moved_entities": 12});
        let (text, _) = gpu_report("carte", &reply, 2);
        assert!(text.contains("never settled"));
        assert!(text.contains("12 entities still moved"));
        assert!(text.contains("2 other clients"));
    }

    fn rounds(a: &[i64], b: &[i64]) -> Value {
        json!({"a": "on", "b": "off", "a_ns": a, "b_ns": b, "reloads": 0})
    }

    #[test]
    fn a_difference_counts_when_it_stands_clear_of_its_rounds() {
        let a = [2_000_000; 6];
        let b = [
            1_500_000, 1_510_000, 1_490_000, 1_505_000, 1_495_000, 1_500_000,
        ];
        let (text, json) = ab_report(&rounds(&a, &b), 0);
        assert!(text.contains("a real difference"), "{text}");
        assert!(json["difference_ms"].as_f64().unwrap() < -0.4);
    }

    #[test]
    fn rounds_that_disagree_give_a_bound_not_a_result() {
        let a = [
            2_000_000, 1_000_000, 2_000_000, 1_000_000, 2_000_000, 1_000_000,
        ];
        let b = [
            1_000_000, 2_000_000, 1_000_000, 2_000_000, 1_000_000, 2_000_000,
        ];
        let (text, _) = ab_report(&rounds(&a, &b), 0);
        assert!(text.contains("no difference found: under"), "{text}");
    }

    #[test]
    fn one_wild_round_does_not_make_a_result() {
        let a = [3_000_000; 7];
        let b = [
            3_000_000, 3_010_000, 2_990_000, 3_000_000, 3_010_000, 2_990_000, 25_000_000,
        ];
        let (text, _) = ab_report(&rounds(&a, &b), 0);
        assert!(text.contains("no difference found"), "{text}");
    }

    #[test]
    fn a_setting_that_draws_nothing_is_said_to_measure_nothing() {
        let dir = std::env::temp_dir().join(format!("voidc-drawn-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("same.png");
        let picture = Picture {
            width: 2,
            height: 2,
            rgb: vec![7; 12],
        };
        picture.save(&path).unwrap();
        let mut reply = rounds(&[2_000_000; 6], &[2_000_000; 6]);
        reply["shots"] = json!([path, path]);
        let (text, json) = ab_report(&reply, 0);
        assert!(text.contains("nothing to measure"), "{text}");
        assert_eq!(json["drawn_difference_px"], 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn too_few_rounds_prove_nothing() {
        let (text, _) = ab_report(&rounds(&[2_000_000; 3], &[1_000_000; 3]), 0);
        assert!(text.contains("too few rounds"), "{text}");
    }

    #[test]
    fn a_reload_voids_a_comparison_whatever_its_figures() {
        let reply =
            json!({"a": "on", "b": "off", "a_ns": [6_600_000], "b_ns": [1_200_000], "reloads": 1});
        let (text, _) = ab_report(&reply, 0);
        assert!(text.contains("not valid"));
    }

    #[test]
    fn a_round_without_a_sample_voids_the_comparison() {
        for empty in [0, -1] {
            let a = [2_000_000, 2_000_000, empty, 2_000_000, 2_000_000, 2_000_000];
            let (text, _) = ab_report(&rounds(&a, &[2_000_000; 6]), 0);
            assert!(text.contains("not valid"), "{text}");
            assert!(!text.contains("no difference found"), "{text}");
        }
    }

    #[test]
    fn no_sample_is_no_figure_and_not_zero_ms() {
        let reply = json!({"ns": [], "sections": 40, "settled": true, "still": true});
        let (text, _) = gpu_report("carte", &reply, 0);
        assert!(!text.contains("0.00 ms"), "{text}");
        assert!(text.contains("no sample"), "{text}");
    }

    #[test]
    fn packets_are_ranked_by_bytes() {
        let rows = packet_rows(&json!([["a", 5, 10], ["b", 1, 900]]));
        assert_eq!(rows[0].0, "b");
    }

    #[test]
    fn a_long_measure_outlives_its_own_length() {
        assert!(timeout_of(&["perf", "metrics", "300"]) > Duration::from_secs(300));
        assert!(timeout_of(&["perf", "jfr", "300"]) > Duration::from_secs(300));
        let short = timeout_of(&["perf", "ab", "clouds", "8", "64"]);
        let long = timeout_of(&["perf", "ab", "clouds", "16", "640"]);
        assert!(long > short * 4, "{short:?} {long:?}");
        assert!(timeout_of(&["perf", "gpu", "6000"]) > Duration::from_secs(1200));
    }
}
