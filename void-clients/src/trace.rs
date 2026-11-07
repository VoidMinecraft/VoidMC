//! Reads a packet trace written by `void` with its `packet-trace` feature.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::args::Arguments;

const USAGE: &str =
    "voidc trace <file> [filters] [--lines] [--json]   totals, or the lines themselves
voidc trace diff <a> <b> [filters] [--json]      what differs tick by tick, exits 1 if anything does

filters: --client N  --player NAME  --packet TEXT  --system TEXT  --dir in|out
         --from TICK  --to TICK  --align PACKET";

const SHOWN: usize = 12;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Line {
    pub tick: i64,
    pub dir: String,
    pub client: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<String>,
    pub packet: String,
    pub bytes: u64,
    pub hash: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<String>,
}

const VALUE_FLAGS: [&str; 8] = [
    "--client", "--player", "--packet", "--system", "--dir", "--from", "--to", "--align",
];

#[derive(Debug, Default)]
struct Filter {
    client: Option<u32>,
    player: Option<String>,
    packet: Option<String>,
    system: Option<String>,
    dir: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    align: Option<String>,
}

impl Filter {
    fn parse(args: &Arguments) -> Result<Self, String> {
        let dir = args.value("--dir");
        if dir
            .as_deref()
            .is_some_and(|dir| dir != "in" && dir != "out")
        {
            return Err("--dir is in or out".into());
        }
        Ok(Self {
            client: args.number("--client")?,
            player: args.value("--player"),
            packet: args.value("--packet"),
            system: args.value("--system"),
            dir,
            from: args.number("--from")?,
            to: args.number("--to")?,
            align: args.value("--align"),
        })
    }

    fn keeps(&self, line: &Line) -> bool {
        self.client.is_none_or(|client| line.client == client)
            && self
                .player
                .as_ref()
                .is_none_or(|player| line.player.as_ref() == Some(player))
            && self
                .packet
                .as_ref()
                .is_none_or(|packet| line.packet.contains(packet.as_str()))
            && self.system.as_ref().is_none_or(|system| {
                line.system
                    .as_ref()
                    .is_some_and(|name| name.contains(system.as_str()))
            })
            && self.dir.as_ref().is_none_or(|dir| &line.dir == dir)
            && self.from.is_none_or(|from| line.tick >= from)
            && self.to.is_none_or(|to| line.tick <= to)
    }
}

/// A trace being read may end in the middle of a line: only the last one may be broken.
fn parse(text: &str) -> Result<Vec<Line>, String> {
    let rows: Vec<&str> = text.lines().filter(|row| !row.trim().is_empty()).collect();
    let mut lines = Vec::with_capacity(rows.len());
    for (at, row) in rows.iter().enumerate() {
        match serde_json::from_str(row) {
            Ok(line) => lines.push(line),
            Err(_) if at + 1 == rows.len() && !text.ends_with('\n') => {}
            Err(error) => return Err(format!("line {}: {error}", at + 1)),
        }
    }
    Ok(lines)
}

/// A client named after its login carries that name on the lines before it too.
fn spread_players(lines: &mut [Line]) {
    let named: HashMap<u32, String> = lines
        .iter()
        .filter_map(|line| Some((line.client, line.player.clone()?)))
        .collect();
    for line in lines {
        if line.player.is_none() {
            line.player = named.get(&line.client).cloned();
        }
    }
}

/// Makes the first line of `packet` tick 0, for runs that did not start on the same tick.
fn align(lines: &mut [Line], packet: &str) -> Result<(), String> {
    let origin = lines
        .iter()
        .find(|line| line.packet.contains(packet))
        .ok_or_else(|| format!("no {packet} in this trace"))?
        .tick;
    for line in lines {
        line.tick -= origin;
    }
    Ok(())
}

fn load(path: &str, filter: &Filter) -> Result<Vec<Line>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut lines = parse(&text).map_err(|e| format!("{path}: {e}"))?;
    spread_players(&mut lines);
    if let Some(packet) = &filter.align {
        align(&mut lines, packet).map_err(|e| format!("{path}: {e}"))?;
    }
    lines.retain(|line| filter.keeps(line));
    Ok(lines)
}

#[derive(Debug, Default, Serialize, PartialEq)]
struct Total {
    name: String,
    packets: u64,
    bytes: u64,
}

fn totals<'a>(lines: &'a [Line], key: impl Fn(&'a Line) -> String) -> Vec<Total> {
    let mut by_key: BTreeMap<String, Total> = BTreeMap::new();
    for line in lines {
        let name = key(line);
        let total = by_key.entry(name.clone()).or_default();
        total.name = name;
        total.packets += 1;
        total.bytes += line.bytes;
    }
    let mut all: Vec<Total> = by_key.into_values().collect();
    all.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    all
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 100.0;
    }
    part as f64 * 100.0 / whole as f64
}

fn client_of(line: &Line) -> String {
    match &line.player {
        Some(player) => format!("{} ({player})", line.client),
        None => line.client.to_string(),
    }
}

fn system_of(line: &Line) -> String {
    line.system.clone().unwrap_or_else(|| "(no system)".into())
}

fn packet_of(line: &Line) -> String {
    line.packet.clone()
}

/// Totals are what was queued: a lost packet only counts in the `dropped` fields.
fn summary(lines: &[Line]) -> Value {
    let out: Vec<&Line> = lines.iter().filter(|line| line.dir == "out").collect();
    let attributed = out.iter().filter(|line| line.system.is_some()).count();
    let (lost, kept): (Vec<Line>, Vec<Line>) = lines
        .iter()
        .cloned()
        .partition(|line| line.dropped.is_some());
    json!({
        "lines": lines.len(),
        "bytes": kept.iter().map(|line| line.bytes).sum::<u64>(),
        "first_tick": lines.iter().map(|line| line.tick).min(),
        "last_tick": lines.iter().map(|line| line.tick).max(),
        "out": out.len(),
        "out_dropped": lost.len(),
        "dropped_bytes": lost.iter().map(|line| line.bytes).sum::<u64>(),
        "out_attributed": attributed,
        "out_attributed_percent": percent(attributed, out.len()),
        "by_system": totals(&kept, system_of),
        "by_packet": totals(&kept, packet_of),
        "by_client": totals(&kept, client_of),
        "dropped_by_system": totals(&lost, system_of),
        "dropped_by_packet": totals(&lost, packet_of),
    })
}

fn print_table(title: &str, rows: &Value) {
    println!("\n{title}");
    let rows = rows.as_array().map_or(&[][..], Vec::as_slice);
    for row in rows.iter().take(SHOWN) {
        println!(
            "{:>12} {:>8}  {}",
            row["bytes"].as_u64().unwrap_or(0),
            row["packets"].as_u64().unwrap_or(0),
            row["name"].as_str().unwrap_or_default()
        );
    }
    if rows.len() > SHOWN {
        println!("{:>21}  ... {} more, see --json", "", rows.len() - SHOWN);
    }
}

fn print_summary(summary: &Value) {
    println!(
        "{} lines, {} bytes, ticks {} to {}",
        summary["lines"], summary["bytes"], summary["first_tick"], summary["last_tick"]
    );
    println!(
        "attributed: {} of {} outgoing, {:.2} %",
        summary["out_attributed"],
        summary["out"],
        summary["out_attributed_percent"].as_f64().unwrap_or(0.0)
    );
    if summary["out_dropped"] != 0 {
        println!(
            "dropped: {} outgoing never queued, {} bytes, left out of the totals below",
            summary["out_dropped"], summary["dropped_bytes"]
        );
    }
    print_table("by system (bytes, packets)", &summary["by_system"]);
    print_table("by packet (bytes, packets)", &summary["by_packet"]);
    print_table("by client (bytes, packets)", &summary["by_client"]);
    if summary["out_dropped"] != 0 {
        print_table("dropped, by system", &summary["dropped_by_system"]);
        print_table("dropped, by packet", &summary["dropped_by_packet"]);
    }
}

/// Who a line goes to, across runs: client ids depend on what connected before.
fn who(line: &Line, ranks: &HashMap<u32, usize>) -> String {
    line.player
        .clone()
        .unwrap_or_else(|| format!("#{}", ranks[&line.client]))
}

struct Sent {
    packet: String,
    hash: u64,
    system: Option<String>,
    dropped: Option<String>,
}

impl Sent {
    fn key(&self) -> Fate<'_> {
        (&self.packet, self.hash, self.dropped.as_deref())
    }

    fn label(&self) -> String {
        match &self.dropped {
            Some(why) => format!("{} (dropped: {why})", self.packet),
            None => self.packet.clone(),
        }
    }
}

type Tick = BTreeMap<String, Vec<Sent>>;
type Fate<'a> = (&'a str, u64, Option<&'a str>);
type Wire<'a> = BTreeMap<&'a str, Vec<Fate<'a>>>;

fn by_tick(lines: &[Line]) -> BTreeMap<i64, Tick> {
    let mut ranks: HashMap<u32, usize> = HashMap::new();
    for line in lines {
        let next = ranks.len();
        ranks.entry(line.client).or_insert(next);
    }
    let mut ticks: BTreeMap<i64, Tick> = BTreeMap::new();
    for line in lines {
        ticks
            .entry(line.tick)
            .or_default()
            .entry(who(line, &ranks))
            .or_default()
            .push(Sent {
                packet: line.packet.clone(),
                hash: line.hash,
                system: line.system.clone(),
                dropped: line.dropped.clone(),
            });
    }
    ticks
}

/// What became of the packets of a tick: the system behind one is not part of it.
fn wire(tick: &Tick) -> Wire<'_> {
    tick.iter()
        .map(|(client, sent)| (client.as_str(), sent.iter().map(Sent::key).collect()))
        .collect()
}

fn sorted(mut wire: Wire<'_>) -> Wire<'_> {
    wire.values_mut().for_each(|packets| packets.sort());
    wire
}

/// What `of` holds beyond `other`, copy by copy.
fn excess<'a>(of: &'a Tick, other: &Tick) -> Vec<&'a Sent> {
    let mut left: HashMap<(&str, Fate<'_>), usize> = HashMap::new();
    for (client, sent) in other {
        for sent in sent {
            *left.entry((client, sent.key())).or_default() += 1;
        }
    }
    let mut extra = Vec::new();
    for (client, sent) in of {
        for sent in sent {
            match left.get_mut(&(client.as_str(), sent.key())) {
                Some(count) if *count > 0 => *count -= 1,
                _ => extra.push(sent),
            }
        }
    }
    extra
}

fn count_names(sent: &[&Sent], into: &mut BTreeMap<String, u64>) {
    for sent in sent {
        *into.entry(sent.label()).or_default() += 1;
    }
}

fn count_systems(sent: &[&Sent]) -> BTreeMap<String, u64> {
    let mut systems = BTreeMap::new();
    for sent in sent {
        let system = sent.system.as_deref().unwrap_or("(no system)");
        *systems.entry(system.to_string()).or_default() += 1;
    }
    systems
}

fn compare(a: &[Line], b: &[Line]) -> Value {
    let (ticks_a, ticks_b) = (by_tick(a), by_tick(b));
    let (mut same, mut order_only, mut differing) = (0u64, 0u64, 0u64);
    let (mut lone_a, mut lone_b) = (0u64, 0u64);
    let (mut only_a, mut only_b) = (BTreeMap::new(), BTreeMap::new());
    let mut first = None;
    let mut first_systems = json!({"a": {}, "b": {}});
    let nothing = Tick::new();
    let all: BTreeSet<&i64> = ticks_a.keys().chain(ticks_b.keys()).collect();
    for tick in all {
        let (in_a, in_b) = (ticks_a.get(tick), ticks_b.get(tick));
        let (tick_a, tick_b) = (in_a.unwrap_or(&nothing), in_b.unwrap_or(&nothing));
        match (in_a, in_b) {
            (Some(_), None) => lone_a += 1,
            (None, Some(_)) => lone_b += 1,
            _ if wire(tick_a) == wire(tick_b) => {
                same += 1;
                continue;
            }
            _ if sorted(wire(tick_a)) == sorted(wire(tick_b)) => {
                order_only += 1;
                continue;
            }
            _ => differing += 1,
        }
        let (extra_a, extra_b) = (excess(tick_a, tick_b), excess(tick_b, tick_a));
        count_names(&extra_a, &mut only_a);
        count_names(&extra_b, &mut only_b);
        if first.is_none() {
            first = Some(*tick);
            first_systems = json!({"a": count_systems(&extra_a), "b": count_systems(&extra_b)});
        }
    }
    json!({
        "ticks_compared": same + order_only + differing,
        "identical": same,
        "order_only": order_only,
        "differing": differing,
        "ticks_only_in_a": lone_a,
        "ticks_only_in_b": lone_b,
        "first_differing_tick": first,
        "packets_only_in_a": only_a,
        "packets_only_in_b": only_b,
        "systems_at_first_difference": first_systems,
    })
}

fn is_identical(report: &Value) -> bool {
    report["differing"] == 0
        && report["order_only"] == 0
        && report["ticks_only_in_a"] == 0
        && report["ticks_only_in_b"] == 0
}

fn print_compare(report: &Value) {
    println!(
        "{} of {} common ticks identical, {} in another order, {} different",
        report["identical"], report["ticks_compared"], report["order_only"], report["differing"]
    );
    println!(
        "ticks only in a: {}, only in b: {}",
        report["ticks_only_in_a"], report["ticks_only_in_b"]
    );
    if !report["first_differing_tick"].is_null() {
        println!("first difference: tick {}", report["first_differing_tick"]);
        println!("only in a: {}", report["packets_only_in_a"]);
        println!("only in b: {}", report["packets_only_in_b"]);
        println!("systems there: {}", report["systems_at_first_difference"]);
    }
}

fn pretty(value: &Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

fn diff(args: &[String]) -> Result<(), String> {
    let args = Arguments::parse(args, &VALUE_FLAGS, &["--json"], USAGE)?;
    let [a, b] = args.files.as_slice() else {
        return Err(USAGE.to_string());
    };
    let mut filter = Filter::parse(&args)?;
    filter.dir.get_or_insert_with(|| "out".to_string());
    let report = compare(&load(a, &filter)?, &load(b, &filter)?);
    if args.has("--json") {
        println!("{}", pretty(&report)?);
    } else {
        print_compare(&report);
    }
    if is_identical(&report) {
        Ok(())
    } else {
        Err(String::new())
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("diff") => return diff(&args[1..]),
        Some("--help") | None => return Err(USAGE.to_string()),
        _ => {}
    }
    let args = Arguments::parse(args, &VALUE_FLAGS, &["--json", "--lines"], USAGE)?;
    let [file] = args.files.as_slice() else {
        return Err(USAGE.to_string());
    };
    let lines = load(file, &Filter::parse(&args)?)?;
    if args.has("--lines") {
        for line in &lines {
            println!(
                "{}",
                serde_json::to_string(line).map_err(|e| e.to_string())?
            );
        }
        return Ok(());
    }
    let summary = summary(&lines);
    if args.has("--json") {
        println!("{}", pretty(&summary)?);
    } else {
        print_summary(&summary);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(tick: i64, client: u32, packet: &str, hash: u64, system: Option<&str>) -> Line {
        Line {
            tick,
            dir: "out".into(),
            client,
            player: None,
            packet: packet.into(),
            bytes: 10,
            hash,
            system: system.map(str::to_string),
            dropped: None,
        }
    }

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    fn filter_of(text: &str) -> Result<Filter, String> {
        Filter::parse(&Arguments::parse(
            &args(text),
            &VALUE_FLAGS,
            &["--json", "--lines"],
            USAGE,
        )?)
    }

    #[test]
    fn negative_ticks_from_an_alignment_are_values() {
        assert_eq!(filter_of("--from -5").unwrap().from, Some(-5));
    }

    #[test]
    fn a_trace_and_a_diff_refuse_stray_arguments() {
        assert!(run(&args("a.jsonl b.jsonl")).is_err());
        assert!(run(&args("diff a.jsonl")).is_err());
        let unknown = run(&args("diff a b --lines")).unwrap_err();
        assert!(unknown.contains("--lines"), "{unknown}");
    }

    #[test]
    fn reads_the_format_written_by_the_server() {
        let text = r#"{"tick":661,"dir":"out","client":1,"packet":"Play/UpdateEntityPosition","bytes":9,"hash":18446744073709551615,"system":"a::b"}
{"tick":662,"dir":"in","client":1,"player":"sonde","packet":"Play/?","bytes":2,"hash":1}
"#;
        let lines = parse(text).unwrap();
        assert_eq!(lines[0].hash, u64::MAX);
        assert_eq!(lines[0].system.as_deref(), Some("a::b"));
        assert_eq!(lines[1].player.as_deref(), Some("sonde"));
        assert_eq!(lines[1].system, None);
    }

    #[test]
    fn a_line_cut_by_a_live_writer_is_skipped_but_a_broken_one_is_not() {
        let good = r#"{"tick":1,"dir":"in","client":1,"packet":"p","bytes":2,"hash":1}"#;
        assert_eq!(parse(&format!("{good}\n{{\"ti")).unwrap().len(), 1);
        let error = parse(&format!("nope\n{good}\n")).unwrap_err();
        assert!(error.contains("line 1"), "{error}");
        assert!(parse(&format!("{good}\nnope\n")).is_err());
    }

    #[test]
    fn filters_combine() {
        let filter = filter_of("--packet Keep --system tick --from 5 --to 6").unwrap();
        assert!(filter.keeps(&line(5, 1, "Play/KeepAlive", 0, Some("a::tick"))));
        assert!(!filter.keeps(&line(7, 1, "Play/KeepAlive", 0, Some("a::tick"))));
        assert!(!filter.keeps(&line(4, 1, "Play/KeepAlive", 0, Some("a::tick"))));
        assert!(!filter.keeps(&line(5, 1, "Play/Chunk", 0, Some("a::tick"))));
        assert!(!filter.keeps(&line(5, 1, "Play/KeepAlive", 0, None)));
    }

    #[test]
    fn a_player_filter_finds_lines_from_before_the_login() {
        let mut lines = vec![line(1, 1, "a", 0, None), line(2, 1, "b", 0, None)];
        lines[1].player = Some("sonde".into());
        lines.push(line(3, 2, "c", 0, None));
        spread_players(&mut lines);
        let filter = filter_of("--player sonde").unwrap();
        lines.retain(|line| filter.keeps(line));
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn bad_flags_are_refused() {
        assert!(filter_of("--dir sideways").is_err());
        assert!(filter_of("--from soon").is_err());
        assert!(filter_of("--from").unwrap_err().contains("--from"));
        assert!(filter_of("--from --lines").unwrap_err().contains("--from"));
        assert!(filter_of("--fromm 5").unwrap_err().contains("--fromm"));
    }

    #[test]
    fn totals_go_by_bytes_and_count_what_is_unattributed() {
        let mut big = line(1, 1, "Play/Chunk", 0, Some("stream"));
        big.bytes = 100;
        let lines = [
            big,
            line(1, 1, "Play/KeepAlive", 0, Some("tick")),
            line(1, 1, "Play/KeepAlive", 0, None),
        ];
        let report = summary(&lines);
        assert_eq!(report["out"], 3);
        assert_eq!(report["out_attributed"], 2);
        assert_eq!(report["by_system"][0]["name"], "stream");
        assert_eq!(report["by_packet"][1]["packets"], 2);
        assert_eq!(report["by_packet"][1]["bytes"], 20);
    }

    #[test]
    fn an_empty_trace_has_nothing_unattributed() {
        assert_eq!(summary(&[])["out_attributed_percent"], 100.0);
    }

    #[test]
    fn same_lines_compare_identical_even_with_other_client_ids() {
        let a = [line(1, 1, "p", 1, None), line(2, 1, "q", 2, None)];
        let b = [line(1, 8, "p", 1, None), line(2, 8, "q", 2, None)];
        let report = compare(&a, &b);
        assert!(is_identical(&report), "{report}");
        assert_eq!(report["identical"], 2);
    }

    #[test]
    fn a_changed_hash_is_found_with_its_tick_packet_and_system() {
        let a = [
            line(1, 1, "same", 1, Some("s")),
            line(2, 1, "SpawnEntity", 2, Some("visibility")),
            line(2, 1, "same", 1, Some("s")),
        ];
        let b = [
            line(1, 1, "same", 1, Some("s")),
            line(2, 1, "SpawnEntity", 3, Some("visibility")),
            line(2, 1, "same", 1, Some("s")),
        ];
        let report = compare(&a, &b);
        assert!(!is_identical(&report));
        assert_eq!(report["first_differing_tick"], 2);
        assert_eq!(report["packets_only_in_a"]["SpawnEntity"], 1);
        assert_eq!(report["packets_only_in_a"].as_object().unwrap().len(), 1);
        assert_eq!(report["systems_at_first_difference"]["a"]["visibility"], 1);
        assert_eq!(report["systems_at_first_difference"]["b"]["visibility"], 1);
    }

    #[test]
    fn the_same_packets_in_another_order_are_told_apart() {
        let a = [line(1, 1, "p", 1, None), line(1, 1, "q", 2, None)];
        let b = [line(1, 1, "q", 2, None), line(1, 1, "p", 1, None)];
        let report = compare(&a, &b);
        assert_eq!(report["order_only"], 1);
        assert_eq!(report["differing"], 0);
        assert!(!is_identical(&report));
    }

    #[test]
    fn a_packet_sent_twice_in_one_run_only_counts_as_the_extra_one() {
        let a = [line(1, 1, "p", 1, None), line(1, 1, "p", 1, None)];
        let b = [line(1, 1, "p", 1, None)];
        let report = compare(&a, &b);
        assert_eq!(report["packets_only_in_a"]["p"], 1);
        assert_eq!(report["packets_only_in_b"], json!({}));
    }

    #[test]
    fn a_tick_missing_from_one_run_is_a_difference() {
        let a = [line(1, 1, "p", 1, None), line(2, 1, "p", 1, None)];
        let b = [line(1, 1, "p", 1, None)];
        let report = compare(&a, &b);
        assert_eq!(report["ticks_only_in_a"], 1);
        assert!(!is_identical(&report));
    }

    #[test]
    fn a_tick_only_in_a_counts_as_the_first_difference() {
        let a = [
            line(10, 1, "Gone", 1, Some("leaver")),
            line(20, 1, "p", 2, None),
        ];
        let b = [line(20, 1, "q", 3, None)];
        let report = compare(&a, &b);
        assert_eq!(report["first_differing_tick"], 10);
        assert_eq!(report["packets_only_in_a"]["Gone"], 1);
        assert_eq!(report["systems_at_first_difference"]["a"]["leaver"], 1);
    }

    #[test]
    fn a_lone_tick_is_reported_even_when_it_is_the_only_difference() {
        let a = [line(1, 1, "p", 1, None)];
        let b = [line(1, 1, "p", 1, None), line(5, 1, "New", 2, Some("late"))];
        let report = compare(&a, &b);
        assert_eq!(report["first_differing_tick"], 5);
        assert_eq!(report["ticks_only_in_b"], 1);
        assert_eq!(report["packets_only_in_b"]["New"], 1);
        assert_eq!(report["systems_at_first_difference"]["b"]["late"], 1);
    }

    #[test]
    fn a_packet_only_in_b_names_its_system() {
        let a = [line(1, 1, "p", 1, Some("s"))];
        let b = [line(1, 1, "p", 1, Some("s")), line(1, 1, "q", 2, Some("t"))];
        let report = compare(&a, &b);
        assert_eq!(report["systems_at_first_difference"]["b"]["t"], 1);
        assert_eq!(report["systems_at_first_difference"]["a"], json!({}));
    }

    #[test]
    fn only_the_copies_in_excess_count_for_the_system() {
        let copy = || line(1, 1, "p", 1, Some("s"));
        let report = compare(&[copy(), copy(), copy()], &[copy()]);
        assert_eq!(report["systems_at_first_difference"]["a"]["s"], 2);
    }

    #[test]
    fn the_same_packet_from_another_system_is_the_same_on_the_wire() {
        let a = [line(1, 1, "p", 1, Some("s"))];
        let b = [line(1, 1, "p", 1, Some("t"))];
        assert!(is_identical(&compare(&a, &b)));
    }

    fn lost(tick: i64, packet: &str, hash: u64, why: &str) -> Line {
        let mut line = line(tick, 1, packet, hash, Some("s"));
        line.dropped = Some(why.into());
        line
    }

    #[test]
    fn a_packet_lost_in_one_run_and_queued_in_the_other_is_a_difference() {
        let report = compare(&[lost(1, "p", 1, "full")], &[line(1, 1, "p", 1, Some("s"))]);
        assert!(!is_identical(&report), "{report}");
        assert_eq!(report["differing"], 1);
        assert_eq!(report["packets_only_in_a"]["p (dropped: full)"], 1);
        assert_eq!(report["packets_only_in_b"]["p"], 1);
    }

    #[test]
    fn packets_lost_for_another_reason_differ_and_for_the_same_one_do_not() {
        let a = [lost(1, "p", 1, "full")];
        assert!(!is_identical(&compare(&a, &[lost(1, "p", 1, "closed")])));
        assert!(is_identical(&compare(&a, &[lost(1, "p", 1, "full")])));
    }

    #[test]
    fn totals_keep_lost_packets_apart() {
        let report = summary(&[
            line(1, 1, "kept", 1, Some("s")),
            lost(1, "gone", 2, "full"),
            lost(2, "gone", 2, "closed"),
        ]);
        assert_eq!(report["bytes"], 10);
        assert_eq!(report["by_system"][0]["packets"], 1);
        assert_eq!(report["by_packet"].as_array().unwrap().len(), 1);
        assert_eq!(report["by_packet"][0]["name"], "kept");
        assert_eq!(report["by_client"][0]["packets"], 1);
        assert_eq!(report["out_dropped"], 2);
        assert_eq!(report["dropped_bytes"], 20);
        assert_eq!(report["dropped_by_packet"][0]["name"], "gone");
        assert_eq!(report["dropped_by_packet"][0]["packets"], 2);
        assert_eq!(report["dropped_by_system"][0]["name"], "s");
    }

    #[test]
    fn a_dropped_packet_is_counted_and_read_back() {
        let text = r#"{"tick":1,"dir":"out","client":1,"packet":"p","bytes":2,"hash":1,"dropped":"full"}
"#;
        let lines = parse(text).unwrap();
        assert_eq!(lines[0].dropped.as_deref(), Some("full"));
        assert_eq!(summary(&lines)["out_dropped"], 1);
    }

    #[test]
    fn players_are_matched_by_name_whatever_the_order_they_joined() {
        let named = |tick, client, name: &str, hash| {
            let mut line = line(tick, client, "p", hash, None);
            line.player = Some(name.into());
            line
        };
        let a = [named(1, 1, "ann", 1), named(1, 2, "bob", 2)];
        let b = [named(1, 7, "bob", 2), named(1, 9, "ann", 1)];
        assert_eq!(compare(&a, &b)["order_only"], 0);
        assert_eq!(compare(&a, &b)["differing"], 0);
    }

    #[test]
    fn aligning_puts_the_chosen_packet_at_tick_zero() {
        let mut lines = vec![line(40, 1, "a", 0, None), line(50, 1, "Login", 0, None)];
        align(&mut lines, "Login").unwrap();
        assert_eq!((lines[0].tick, lines[1].tick), (-10, 0));
        assert!(align(&mut lines, "Nothing").is_err());
    }
}
