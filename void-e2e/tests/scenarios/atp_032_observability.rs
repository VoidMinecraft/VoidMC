//! VMC-ATP-032: the opt-in `/health` and `/metrics` endpoints, queried over
//! real HTTP while real clients play.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

use bevy_app::Update;
use bevy_ecs::prelude::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use voidmc::components::KeepAliveState;
use voidmc::{ObservabilityAddress, ObservabilityConfig};
use voidmc_e2e::protocol::packets::game::ServerboundPaddleBoat;
use voidmc_e2e::{TestServer, TestServerBuilder};

use crate::{DECODE_FAILURE, malformed_play_packets, var_int};

/// Short, so that the stall scenario does not wait long, yet fifty ticks at
/// 50 TPS, so that a busy CI machine does not look stalled.
const STALL_THRESHOLD: Duration = Duration::from_secs(1);
const WAIT: Duration = Duration::from_secs(15);
const MAX_PLAYERS: i32 = 20;

fn observed() -> TestServerBuilder {
    TestServer::builder().config(|config| {
        config
            .max_players(MAX_PLAYERS)
            .observability(ObservabilityConfig::localhost(0).stall_threshold(STALL_THRESHOLD))
    })
}

async fn endpoint(server: &TestServer) -> SocketAddr {
    server
        .with_world(|world| world.resource::<ObservabilityAddress>().0)
        .await
}

async fn within<T>(what: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(WAIT, future)
        .await
        .unwrap_or_else(|_| panic!("timed out after {WAIT:?} waiting for {what}"))
}

// --- A minimal HTTP/1.1 client -------------------------------------------

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl HttpResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The `status` field of a `/health` body.
    fn health(&self) -> &str {
        let start = self
            .body
            .find("\"status\":\"")
            .unwrap_or_else(|| panic!("no status in {:?}", self.body))
            + "\"status\":\"".len();
        let end = start + self.body[start..].find('"').expect("unterminated status");
        &self.body[start..end]
    }
}

async fn exchange(address: SocketAddr, request: &[u8]) -> std::io::Result<HttpResponse> {
    let mut stream = TcpStream::connect(address).await?;
    stream.write_all(request).await?;
    let mut raw = Vec::new();
    within("an HTTP response", stream.read_to_end(&mut raw)).await?;
    let raw = String::from_utf8(raw).expect("the response is UTF-8");
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no end of headers in {raw:?}"));
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap();
    let mut parts = status_line.splitn(3, ' ');
    assert_eq!(parts.next(), Some("HTTP/1.1"), "{status_line}");
    let status = parts.next().unwrap().parse().unwrap();
    let headers: Vec<(String, String)> = lines
        .map(|line| {
            let (key, value) = line.split_once(": ").expect("a header line");
            (key.to_string(), value.to_string())
        })
        .collect();
    let response = HttpResponse {
        status,
        headers,
        body: body.to_string(),
    };
    let length: usize = response
        .header("Content-Length")
        .expect("a Content-Length")
        .parse()
        .unwrap();
    if !request.starts_with(b"HEAD ") {
        assert_eq!(
            length,
            response.body.len(),
            "Content-Length matches the body"
        );
    }
    Ok(response)
}

async fn get(address: SocketAddr, path: &str) -> HttpResponse {
    exchange(
        address,
        format!("GET {path} HTTP/1.1\r\nHost: test\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap_or_else(|error| panic!("GET {path} failed: {error}"))
}

/// Polls `/health` back to back until `done` accepts a response. The game
/// thread may be blocked, so there is no tick to pace the polling on.
async fn health_until(
    address: SocketAddr,
    what: &str,
    done: impl Fn(&HttpResponse) -> bool,
) -> HttpResponse {
    within(what, async {
        loop {
            let response = get(address, "/health").await;
            if done(&response) {
                return response;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
}

// --- A strict Prometheus text format (0.0.4) parser ----------------------

type Labels = BTreeMap<String, String>;

#[derive(Debug, Default)]
struct Scrape {
    /// Family name to its declared type.
    types: HashMap<String, String>,
    /// Sample name and labels to value.
    samples: BTreeMap<(String, Labels), f64>,
    /// Sample name and labels to the family it belongs to.
    family_of: HashMap<(String, Labels), String>,
}

impl Scrape {
    fn value(&self, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
        let labels = labels
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        self.samples.get(&(name.to_string(), labels)).copied()
    }

    fn counter(&self, name: &str) -> f64 {
        assert_eq!(self.types.get(name).map(String::as_str), Some("counter"));
        self.value(name, &[])
            .unwrap_or_else(|| panic!("no sample for {name}"))
    }

    fn gauge(&self, name: &str) -> f64 {
        assert_eq!(self.types.get(name).map(String::as_str), Some("gauge"));
        self.value(name, &[])
            .unwrap_or_else(|| panic!("no sample for {name}"))
    }

    fn connection_errors(&self, kind: &str) -> f64 {
        self.value("voidmc_connection_errors_total", &[("kind", kind)])
            .unwrap_or_else(|| panic!("no connection error sample for {kind}"))
    }
}

fn is_metric_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == ':')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

fn is_label_name(name: &str) -> bool {
    let mut chars = name.chars();
    !name.starts_with("__")
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_value(text: &str) -> f64 {
    match text {
        "+Inf" => f64::INFINITY,
        "-Inf" => f64::NEG_INFINITY,
        "NaN" => f64::NAN,
        _ => {
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b)),
                "not a number: {text:?}"
            );
            text.parse()
                .unwrap_or_else(|_| panic!("not a number: {text:?}"))
        }
    }
}

/// Parses `labels` (between the braces) of a sample line.
fn parse_labels(mut rest: &str, line: &str) -> Labels {
    let mut labels = Labels::new();
    while !rest.is_empty() {
        let (name, after) = rest
            .split_once("=\"")
            .unwrap_or_else(|| panic!("bad label in {line:?}"));
        assert!(
            is_label_name(name),
            "invalid label name {name:?} in {line:?}"
        );
        let mut value = String::new();
        let mut chars = after.char_indices();
        let end = loop {
            match chars.next() {
                Some((_, '\\')) => match chars.next() {
                    Some((_, '\\')) => value.push('\\'),
                    Some((_, '"')) => value.push('"'),
                    Some((_, 'n')) => value.push('\n'),
                    other => panic!("bad escape {other:?} in {line:?}"),
                },
                Some((index, '"')) => break index,
                Some((_, '\n')) | None => panic!("unterminated label value in {line:?}"),
                Some((_, c)) => value.push(c),
            }
        };
        assert!(
            labels.insert(name.to_string(), value).is_none(),
            "label {name} repeated in {line:?}"
        );
        rest = &after[end + 1..];
        if let Some(next) = rest.strip_prefix(',') {
            rest = next;
        } else {
            assert!(rest.is_empty(), "junk after label in {line:?}");
        }
    }
    labels
}

/// The family a sample named `name` belongs to, given the declared types.
fn family_of<'a>(name: &'a str, types: &HashMap<String, String>) -> Option<&'a str> {
    if types.contains_key(name) {
        return Some(name);
    }
    for suffix in ["_bucket", "_sum", "_count"] {
        if let Some(base) = name.strip_suffix(suffix) {
            match types.get(base).map(String::as_str) {
                Some("histogram") => return Some(base),
                Some("summary") if suffix != "_bucket" => return Some(base),
                _ => {}
            }
        }
    }
    None
}

fn parse_exposition(text: &str) -> Scrape {
    assert!(text.ends_with('\n'), "the exposition ends with a newline");
    let mut scrape = Scrape::default();
    let mut helped = HashSet::<String>::new();
    let mut finished_families = Vec::<String>::new();
    let mut current: Option<String> = None;

    for line in text[..text.len() - 1].split('\n') {
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            let mut words = comment.trim_start().splitn(3, ' ');
            match words.next() {
                Some("HELP") => {
                    let name = words.next().expect("HELP names a metric");
                    assert!(is_metric_name(name), "invalid name in {line:?}");
                    assert!(words.next().is_some(), "HELP without text: {line:?}");
                    assert!(helped.insert(name.to_string()), "HELP twice for {name}");
                }
                Some("TYPE") => {
                    let name = words.next().expect("TYPE names a metric");
                    assert!(is_metric_name(name), "invalid name in {line:?}");
                    let kind = words.next().expect("TYPE names a type");
                    assert!(
                        ["counter", "gauge", "histogram", "summary", "untyped"].contains(&kind),
                        "unknown type in {line:?}"
                    );
                    assert!(
                        !finished_families.contains(&name.to_string())
                            && current.as_deref() != Some(name),
                        "TYPE for {name} after its samples"
                    );
                    assert!(
                        scrape
                            .types
                            .insert(name.to_string(), kind.to_string())
                            .is_none(),
                        "TYPE twice for {name}"
                    );
                    if kind == "counter" {
                        assert!(name.ends_with("_total"), "counter {name} lacks _total");
                    }
                }
                _ => {}
            }
            continue;
        }

        let name_end = line
            .find(['{', ' '])
            .unwrap_or_else(|| panic!("no value in {line:?}"));
        let name = &line[..name_end];
        assert!(is_metric_name(name), "invalid metric name in {line:?}");
        let (labels, rest) = if line[name_end..].starts_with('{') {
            let close = line
                .rfind('}')
                .unwrap_or_else(|| panic!("unclosed labels in {line:?}"));
            (
                parse_labels(&line[name_end + 1..close], line),
                &line[close + 1..],
            )
        } else {
            (Labels::new(), &line[name_end..])
        };
        let rest = rest
            .strip_prefix(' ')
            .unwrap_or_else(|| panic!("no space before the value in {line:?}"));
        let mut fields = rest.split(' ');
        let value = parse_value(fields.next().unwrap());
        if let Some(timestamp) = fields.next() {
            timestamp
                .parse::<i64>()
                .unwrap_or_else(|_| panic!("bad timestamp in {line:?}"));
        }
        assert!(fields.next().is_none(), "junk after the value in {line:?}");
        assert!(value.is_finite(), "non-finite value in {line:?}");

        let family = family_of(name, &scrape.types)
            .unwrap_or_else(|| panic!("sample {name} belongs to no declared family"))
            .to_string();
        if current.as_deref() != Some(&family) {
            assert!(
                !finished_families.contains(&family),
                "samples of {family} are not grouped together"
            );
            if let Some(previous) = current.replace(family.clone()) {
                finished_families.push(previous);
            }
        }
        match scrape.types[&family].as_str() {
            "counter" => assert!(value >= 0.0, "negative counter in {line:?}"),
            "histogram" if name.ends_with("_bucket") => {
                assert!(labels.contains_key("le"), "bucket without le: {line:?}")
            }
            _ => {}
        }
        let key = (name.to_string(), labels);
        assert!(
            !scrape.samples.contains_key(&key),
            "duplicate series in {line:?}"
        );
        scrape.family_of.insert(key.clone(), family);
        scrape.samples.insert(key, value);
    }

    // Histograms: cumulative buckets ending in +Inf, which equals _count.
    for (family, kind) in &scrape.types {
        if kind != "histogram" {
            continue;
        }
        let mut buckets: Vec<(f64, f64)> = scrape
            .samples
            .iter()
            .filter(|((name, _), _)| *name == format!("{family}_bucket"))
            .map(|((_, labels), value)| (parse_value(&labels["le"]), *value))
            .collect();
        buckets.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!(
            buckets.windows(2).all(|pair| pair[0].1 <= pair[1].1),
            "{family} buckets are not cumulative: {buckets:?}"
        );
        let last = buckets.last().expect("a histogram has buckets");
        assert_eq!(last.0, f64::INFINITY, "{family} has no +Inf bucket");
        let count = scrape
            .value(&format!("{family}_count"), &[])
            .expect("a histogram has _count");
        assert_eq!(last.1, count, "{family} +Inf bucket differs from _count");
        assert!(scrape.value(&format!("{family}_sum"), &[]).is_some());
    }
    scrape
}

/// Fails if a counter (or histogram series) went down between two scrapes.
fn assert_counters_never_decrease(before: &Scrape, after: &Scrape) {
    for (key, value) in &before.samples {
        let kind = before.types[&before.family_of[key]].as_str();
        if kind != "counter" && kind != "histogram" {
            continue;
        }
        let later = after
            .samples
            .get(key)
            .unwrap_or_else(|| panic!("{key:?} disappeared between scrapes"));
        assert!(later >= value, "{key:?} went from {value} down to {later}");
    }
}

async fn scrape(address: SocketAddr) -> Scrape {
    let response = get(address, "/metrics").await;
    assert_eq!(response.status, 200, "{}", response.body);
    assert_eq!(
        response.header("Content-Type"),
        Some("text/plain; version=0.0.4; charset=utf-8")
    );
    parse_exposition(&response.body)
}

/// Scrapes once per server tick until `done` accepts the scrape.
async fn scrape_until(
    server: &TestServer,
    address: SocketAddr,
    what: &str,
    done: impl Fn(&Scrape) -> bool,
) -> Scrape {
    within(what, async {
        loop {
            let scrape = scrape(address).await;
            if done(&scrape) {
                return scrape;
            }
            server.wait_ticks(1).await;
        }
    })
    .await
}

/// The documented metric set and the type of each family.
const DOCUMENTED: [(&str, &str); 13] = [
    ("voidmc_ticks_total", "counter"),
    ("voidmc_tick_duration_seconds", "histogram"),
    ("voidmc_target_ticks_per_second", "gauge"),
    ("voidmc_ticks_per_second", "gauge"),
    ("voidmc_seconds_since_last_tick", "gauge"),
    ("voidmc_players_online", "gauge"),
    ("voidmc_players_max", "gauge"),
    ("voidmc_players_joined_total", "counter"),
    ("voidmc_connections_accepted_total", "counter"),
    ("voidmc_connection_errors_total", "counter"),
    ("voidmc_clients_kicked_total", "counter"),
    ("voidmc_packets_unrecognized_total", "counter"),
    ("voidmc_packet_errors_total", "counter"),
];

// --- Scenarios -------------------------------------------------------------

#[tokio::test]
async fn health_and_metrics_describe_a_running_server() {
    let server = observed().start().await;
    let address = endpoint(&server).await;
    assert!(address.ip().is_loopback(), "{address}");
    assert_ne!(address.port(), 0);
    assert_ne!(address, server.address());

    let health = health_until(address, "the server to be healthy", |response| {
        response.status == 200
    })
    .await;
    assert_eq!(health.header("Content-Type"), Some("application/json"));
    assert_eq!(health.health(), "ok");
    assert!(health.body.contains("\"stall_threshold_ms\":1000"));

    let head = exchange(address, b"HEAD /health HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(head.body, "");

    let first = scrape(address).await;
    for (family, kind) in DOCUMENTED {
        assert_eq!(
            first.types.get(family).map(String::as_str),
            Some(kind),
            "{family} is declared as a {kind}"
        );
    }
    assert_eq!(first.gauge("voidmc_target_ticks_per_second"), 50.0);
    assert_eq!(first.gauge("voidmc_players_max"), f64::from(MAX_PLAYERS));
    assert_eq!(first.gauge("voidmc_players_online"), 0.0);
    assert!(first.gauge("voidmc_seconds_since_last_tick") >= 0.0);
    assert!(first.counter("voidmc_ticks_total") >= 1.0);
    assert!(
        first
            .value("voidmc_tick_duration_seconds_count", &[])
            .unwrap()
            >= 1.0
    );

    server.wait_ticks(5).await;
    let second = scrape(address).await;
    assert!(
        second.counter("voidmc_ticks_total") >= first.counter("voidmc_ticks_total") + 4.0,
        "five more ticks started, so at least four more completed"
    );
    assert_counters_never_decrease(&first, &second);

    let measured = scrape_until(&server, address, "a measured tick rate", |scrape| {
        scrape.value("voidmc_ticks_per_second", &[]).is_some()
    })
    .await;
    let tps = measured.gauge("voidmc_ticks_per_second");
    assert!(
        tps > 0.0 && tps <= 60.0,
        "measured {tps} TPS at a target of 50"
    );
    assert_counters_never_decrease(&second, &measured);

    server.stop().await;
}

#[tokio::test]
async fn player_metrics_follow_clients_joining_and_leaving() {
    let server = observed().start().await;
    let address = endpoint(&server).await;
    let online = |n: f64| move |scrape: &Scrape| scrape.gauge("voidmc_players_online") == n;

    let before = scrape(address).await;
    assert_eq!(before.gauge("voidmc_players_online"), 0.0);
    assert_eq!(before.counter("voidmc_players_joined_total"), 0.0);

    let alice = server.join("Alice").await;
    let one = scrape_until(&server, address, "one player online", online(1.0)).await;
    assert_eq!(one.counter("voidmc_players_joined_total"), 1.0);

    let bob = server.join("Bob").await;
    let two = scrape_until(&server, address, "two players online", online(2.0)).await;
    assert_eq!(two.counter("voidmc_players_joined_total"), 2.0);
    assert!(two.counter("voidmc_connections_accepted_total") >= 2.0);

    alice.disconnect().await;
    let after = scrape_until(&server, address, "one player left", online(1.0)).await;
    assert_eq!(
        after.counter("voidmc_players_joined_total"),
        2.0,
        "leaving does not undo a join"
    );
    assert_counters_never_decrease(&before, &one);
    assert_counters_never_decrease(&one, &two);
    assert_counters_never_decrease(&two, &after);

    // Ordinary play and quitting are not errors.
    assert_eq!(after.counter("voidmc_packet_errors_total"), 0.0);
    assert_eq!(after.counter("voidmc_packets_unrecognized_total"), 0.0);
    assert_eq!(after.counter("voidmc_clients_kicked_total"), 0.0);
    assert_eq!(after.connection_errors("frame"), 0.0);
    assert_eq!(after.connection_errors("decode"), 0.0);

    bob.assert_healthy();
    server.stop().await;
}

/// Since VMC-ATP-034 (#151) `/kick` and despawning a client entity close the
/// connection on the server side, and players still online at shutdown leave
/// in the last tick: each way out brings the players gauge back down, the
/// first two count as kicks, and shutting down does not.
#[tokio::test]
async fn kicks_despawns_and_shutdown_bring_the_players_gauge_back_to_zero() {
    let (release_tx, release_rx) = flume::bounded(1);
    let server = observed()
        .operators()
        .plugin(move |app| {
            app.insert_resource(HoldShutdown(release_rx));
        })
        .start()
        .await;
    // Declared after the server, so a failing assertion releases the
    // shutdown before the server is dropped and waits for it.
    let release = ReleaseShutdown(release_tx);
    let address = endpoint(&server).await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let eve = server.join("Eve").await;
    let three = scrape_until(&server, address, "three players online", |scrape| {
        scrape.gauge("voidmc_players_online") == 3.0
    })
    .await;
    assert_eq!(three.counter("voidmc_clients_kicked_total"), 0.0);

    alice.command("kick Bob Bye now").await;
    let kicked = scrape_until(&server, address, "Bob kicked and gone", |scrape| {
        scrape.counter("voidmc_clients_kicked_total") == 1.0
            && scrape.gauge("voidmc_players_online") == 2.0
    })
    .await;
    drop(bob);

    let eve_entity = server.player_entity(eve.uuid()).await;
    server
        .with_world(move |world| {
            world.despawn(eve_entity);
        })
        .await;
    let despawned = scrape_until(&server, address, "Eve's connection closed", |scrape| {
        scrape.counter("voidmc_clients_kicked_total") == 2.0
            && scrape.gauge("voidmc_players_online") == 1.0
    })
    .await;
    drop(eve);
    assert_eq!(despawned.counter("voidmc_players_joined_total"), 3.0);
    assert_counters_never_decrease(&three, &kicked);
    assert_counters_never_decrease(&kicked, &despawned);

    // Alice is still online when the server stops.
    let ((), stopping) = tokio::join!(server.stop(), async {
        health_until(address, "the shutdown to be reported", |response| {
            response.health() == "stopping"
        })
        .await;
        // The exit tick switches to `stopping` just before it stores the
        // gauges, so give that tick a moment to finish.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut stopping = scrape(address).await;
        while stopping.gauge("voidmc_players_online") != 0.0 && std::time::Instant::now() < deadline
        {
            tokio::task::yield_now().await;
            stopping = scrape(address).await;
        }
        drop(release);
        stopping
    });
    assert_eq!(
        stopping.gauge("voidmc_players_online"),
        0.0,
        "players online at shutdown leave in the exit tick"
    );
    assert_eq!(
        stopping.counter("voidmc_clients_kicked_total"),
        2.0,
        "shutting down is not a kick"
    );
    assert_eq!(stopping.counter("voidmc_players_joined_total"), 3.0);
    assert_counters_never_decrease(&despawned, &stopping);
    drop(alice);
}

#[tokio::test]
async fn error_counters_count_errors_that_clients_cause() {
    let server = observed().start().await;
    let address = endpoint(&server).await;
    let before = scrape(address).await;
    assert_eq!(before.connection_errors("frame"), 0.0);
    assert_eq!(before.counter("voidmc_clients_kicked_total"), 0.0);
    assert_eq!(before.counter("voidmc_packets_unrecognized_total"), 0.0);

    // A connection whose first frame is empty breaks the framing.
    let mut broken = TcpStream::connect(server.address()).await.unwrap();
    broken.write_all(&[0x00]).await.unwrap();
    let framed = scrape_until(&server, address, "a frame error counted", |scrape| {
        scrape.connection_errors("frame") == 1.0
    })
    .await;
    let mut rest = Vec::new();
    within(
        "the server to close the broken connection",
        broken.read_to_end(&mut rest),
    )
    .await
    .unwrap();

    // A player whose keep-alive went unanswered for 30 s is disconnected by
    // the server. Backdating Bob's pending keep-alive sends him down that path.
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let bob_entity = server.player_entity(bob.uuid()).await;
    server
        .with_world(move |world| {
            let mut keep_alive = world.get_mut::<KeepAliveState>(bob_entity).unwrap();
            keep_alive.awaiting_response = true;
            keep_alive.last_sent_id = 0;
        })
        .await;
    let kicked = scrape_until(&server, address, "a timed-out client counted", |scrape| {
        scrape.counter("voidmc_clients_kicked_total") == 1.0
    })
    .await;
    drop(bob);

    // A real client sending a packet VoidMC does not know.
    alice
        .send(ServerboundPaddleBoat {
            left: true,
            right: false,
        })
        .await;
    let unrecognized = scrape_until(
        &server,
        address,
        "an unrecognized packet counted",
        |scrape| scrape.counter("voidmc_packets_unrecognized_total") == 1.0,
    )
    .await;
    assert_eq!(unrecognized.counter("voidmc_packet_errors_total"), 0.0);
    assert_eq!(unrecognized.connection_errors("frame"), 1.0);
    assert_eq!(unrecognized.connection_errors("decode"), 0.0);
    assert_counters_never_decrease(&before, &framed);
    assert_counters_never_decrease(&framed, &kicked);
    assert_counters_never_decrease(&kicked, &unrecognized);
    alice.assert_healthy();

    // The counter tracks the condition the server logs; it is the only
    // failure this scenario provokes, so the server is dropped, not `stop`ped.
    let failures: Vec<String> = server
        .logs()
        .into_iter()
        .filter(|record| record.is_failure())
        .map(|record| record.to_string())
        .collect();
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert!(failures[0].contains("Unrecognized packet"), "{failures:#?}");
    drop(server);
}

/// Since #178 a known packet whose body does not decode disconnects its
/// sender: it counts as a packet error and as a kick. A packet whose id
/// VoidMC does not know is still only dropped: it counts as unrecognized and
/// its sender stays.
#[tokio::test]
async fn malformed_packets_count_as_errors_and_kicks_unknown_ones_as_unrecognized() {
    let server = observed().start().await;
    let address = endpoint(&server).await;
    let alice = server.join("Alice").await;
    let mallory = server.join("Mallory").await;
    let before = scrape_until(&server, address, "two players online", |scrape| {
        scrape.gauge("voidmc_players_online") == 2.0
    })
    .await;
    assert_eq!(before.counter("voidmc_packet_errors_total"), 0.0);
    assert_eq!(before.counter("voidmc_packets_unrecognized_total"), 0.0);
    assert_eq!(before.counter("voidmc_clients_kicked_total"), 0.0);

    // A Use Item On whose face is not one of the six.
    let (_, invalid_face) = malformed_play_packets().swap_remove(0);
    mallory.send_raw(&invalid_face).await;
    let reason = mallory.expect_disconnect().await;
    assert!(reason.starts_with(DECODE_FAILURE), "{reason}");
    let kicked = scrape_until(&server, address, "Mallory kicked and gone", |scrape| {
        scrape.counter("voidmc_clients_kicked_total") == 1.0
            && scrape.gauge("voidmc_players_online") == 1.0
    })
    .await;
    assert_eq!(kicked.counter("voidmc_packet_errors_total"), 1.0);
    assert_eq!(kicked.counter("voidmc_packets_unrecognized_total"), 0.0);

    // Ids no serverbound play packet has, one of them past a byte.
    alice.send_raw(&[0x7f, 1, 2, 3]).await;
    let mut beyond_a_byte = var_int(300);
    beyond_a_byte.push(0);
    alice.send_raw(&beyond_a_byte).await;
    let unrecognized = scrape_until(&server, address, "the unknown packets counted", |scrape| {
        scrape.counter("voidmc_packets_unrecognized_total") == 2.0
    })
    .await;
    assert_eq!(unrecognized.counter("voidmc_packet_errors_total"), 1.0);
    assert_eq!(unrecognized.counter("voidmc_clients_kicked_total"), 1.0);
    assert_eq!(unrecognized.gauge("voidmc_players_online"), 1.0);
    assert_eq!(unrecognized.connection_errors("decode"), 0.0);
    assert_counters_never_decrease(&before, &kicked);
    assert_counters_never_decrease(&kicked, &unrecognized);
    alice.sync().await;
    alice.assert_healthy();

    // A malformed packet is the client's fault, not a server error: the
    // only failures logged are the two unknown packets, so the server is
    // dropped, not `stop`ped.
    let failures: Vec<String> = server
        .logs()
        .into_iter()
        .filter(|record| record.is_failure())
        .map(|record| record.to_string())
        .collect();
    assert_eq!(failures.len(), 2, "{failures:#?}");
    assert!(
        failures
            .iter()
            .all(|failure| failure.contains("Unrecognized packet")),
        "{failures:#?}"
    );
    drop(server);
}

/// Requests sent through `Stalls` block the game thread until released.
#[derive(Resource)]
struct Stalls(flume::Receiver<flume::Receiver<()>>);

fn stall_when_asked(stalls: Res<Stalls>) {
    if let Ok(release) = stalls.0.try_recv() {
        // Bounded, so that a failing test cannot hang the server.
        let _ = release.recv_timeout(WAIT);
    }
}

#[tokio::test]
async fn health_reports_a_stalled_game_loop_until_it_recovers() {
    let (stall_tx, stall_rx) = flume::unbounded();
    let server = observed()
        .plugin(move |app| {
            app.insert_resource(Stalls(stall_rx))
                .add_systems(Update, stall_when_asked);
        })
        .start()
        .await;
    let address = endpoint(&server).await;
    health_until(address, "the server to be healthy", |response| {
        response.status == 200
    })
    .await;

    let (release_tx, release_rx) = flume::bounded(1);
    stall_tx.send(release_rx).unwrap();
    let stalled = health_until(address, "the stall to be reported", |response| {
        response.health() == "stalled"
    })
    .await;
    assert_eq!(stalled.status, 503);
    assert_eq!(stalled.header("Content-Type"), Some("application/json"));
    assert_eq!(stalled.health(), "stalled", "{}", stalled.body);

    // Metrics are still served while the game thread is blocked.
    let during = scrape(address).await;
    assert!(during.gauge("voidmc_seconds_since_last_tick") > STALL_THRESHOLD.as_secs_f64());

    release_tx.send(()).unwrap();
    let recovered = health_until(address, "the server to recover", |response| {
        response.status == 200
    })
    .await;
    assert_eq!(recovered.health(), "ok");
    let after = scrape(address).await;
    assert!(after.counter("voidmc_ticks_total") > during.counter("voidmc_ticks_total"));
    assert_counters_never_decrease(&during, &after);

    server.stop().await;
}

/// Blocks the game thread while the world is dropped, the last step of a
/// shutdown, until released.
#[derive(Resource)]
struct HoldShutdown(flume::Receiver<()>);

impl Drop for HoldShutdown {
    fn drop(&mut self) {
        let _ = self.0.recv_timeout(WAIT);
    }
}

/// Releases a [`HoldShutdown`] when dropped, also when the test panics.
struct ReleaseShutdown(flume::Sender<()>);

impl Drop for ReleaseShutdown {
    fn drop(&mut self) {
        let _ = self.0.try_send(());
    }
}

#[tokio::test]
async fn health_reports_stopping_then_the_endpoint_closes() {
    let (release_tx, release_rx) = flume::bounded(1);
    let server = observed()
        .plugin(move |app| {
            app.insert_resource(HoldShutdown(release_rx));
        })
        .start()
        .await;
    let address = endpoint(&server).await;
    health_until(address, "the server to be healthy", |response| {
        response.status == 200
    })
    .await;

    let ((), stopping) = tokio::join!(server.stop(), async {
        let stopping = health_until(address, "the shutdown to be reported", |response| {
            response.health() == "stopping"
        })
        .await;
        release_tx.send(()).unwrap();
        stopping
    });
    assert_eq!(stopping.status, 503);
    assert_eq!(stopping.health(), "stopping", "{}", stopping.body);

    let error = TcpStream::connect(address)
        .await
        .expect_err("nothing listens once the server has stopped");
    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionRefused);
}

#[tokio::test]
async fn bad_requests_are_rejected_without_blocking_the_server() {
    let server = observed().start().await;
    let address = endpoint(&server).await;

    // A client that never finishes its request holds only its own connection.
    let mut slow = TcpStream::connect(address).await.unwrap();
    slow.write_all(b"GET /health HTTP/1.1\r\n").await.unwrap();

    let missing = get(address, "/nope").await;
    assert_eq!(missing.status, 404);

    let post = exchange(
        address,
        b"POST /metrics HTTP/1.1\r\nContent-Length: 2\r\n\r\nhi",
    )
    .await
    .unwrap();
    assert_eq!(post.status, 405);
    assert_eq!(post.header("Allow"), Some("GET, HEAD"));

    let garbage = exchange(address, b"\x16\x03\x01\x02\x00\x01\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(garbage.status, 400);

    let http2 = exchange(address, b"GET /health HTTP/2.0\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(http2.status, 505);

    let mut oversized = b"GET /metrics HTTP/1.1\r\nX-Padding: ".to_vec();
    oversized.resize(16 * 1024, b'a');
    let too_large = exchange(address, &oversized).await.unwrap();
    assert_eq!(too_large.status, 431);

    let first = scrape(address).await;
    server.wait_ticks(3).await;
    let second = scrape(address).await;
    assert!(second.counter("voidmc_ticks_total") > first.counter("voidmc_ticks_total"));
    health_until(address, "the server to be healthy", |response| {
        response.status == 200
    })
    .await;
    // Everything above was answered while the slow client still waits: it has
    // been sent nothing, not even its 408.
    let mut nothing = [0u8; 1];
    match slow.try_read(&mut nothing) {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("the slow client was answered or closed first: {other:?}"),
    }
    drop(slow);

    server.stop().await;
}

#[tokio::test]
async fn nothing_listens_unless_observability_is_enabled() {
    let server = TestServer::start().await;
    let enabled = server
        .with_world(|world| world.get_resource::<ObservabilityAddress>().copied())
        .await;
    assert_eq!(enabled, None);

    #[cfg(target_os = "linux")]
    for port in listening_ports_of_this_process() {
        if port == server.address().port() {
            continue;
        }
        assert!(
            !speaks_http(port).await,
            "port {port} of this process answers HTTP although observability is off"
        );
    }

    server.stop().await;
}

/// TCP ports this process listens on, from `/proc`.
#[cfg(target_os = "linux")]
fn listening_ports_of_this_process() -> Vec<u16> {
    let inodes: Vec<String> = std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| std::fs::read_link(entry.ok()?.path()).ok())
        .filter_map(|target| {
            let target = target.to_string_lossy().into_owned();
            Some(
                target
                    .strip_prefix("socket:[")?
                    .strip_suffix(']')?
                    .to_string(),
            )
        })
        .collect();
    let mut ports = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // local_address is field 1, st is field 3 (0A = LISTEN), inode is field 9.
            if fields.len() > 9 && fields[3] == "0A" && inodes.iter().any(|i| i == fields[9]) {
                let port = fields[1].rsplit(':').next().unwrap();
                ports.push(u16::from_str_radix(port, 16).unwrap());
            }
        }
    }
    assert!(!ports.is_empty(), "the game port itself is listed");
    ports
}

/// Whether something on `port` answers an HTTP request. A listener left over
/// by an earlier server may accept and stay silent, hence the bounded read.
#[cfg(target_os = "linux")]
async fn speaks_http(port: u16) -> bool {
    for host in ["127.0.0.1", "::1"] {
        let Ok(mut stream) = TcpStream::connect((host, port)).await else {
            continue;
        };
        if stream
            .write_all(b"GET /health HTTP/1.1\r\n\r\n")
            .await
            .is_err()
        {
            continue;
        }
        let mut start = [0u8; 5];
        let read =
            tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut start)).await;
        if matches!(read, Ok(Ok(_))) && &start == b"HTTP/" {
            return true;
        }
    }
    false
}
