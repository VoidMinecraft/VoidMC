#![cfg(all(unix, feature = "packet-trace"))]

//! The stepped runner and the packet trace in one server: one tick number, one packet count.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::thread::sleep;
use std::time::Duration;

use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;
use uuid::Uuid;
use voidmc::packet_trace::trace_layer;
use voidmc::{ServerConfigBuilder, VoidServer};
use voidmc_codec::{Encode, VarI32};
use voidmc_protocol::serverbound::{Handshake, HandshakePacket, LoginPacket, LoginStart};
use voidmc_protocol::{PROTOCOL_VERSION, State};

struct Pilot {
    orders: UnixStream,
    replies: BufReader<UnixStream>,
}

impl Pilot {
    fn connect(socket: &std::path::Path) -> Self {
        for _ in 0..3000 {
            if let Ok(orders) = UnixStream::connect(socket) {
                let replies = BufReader::new(orders.try_clone().unwrap());
                return Self { orders, replies };
            }
            sleep(Duration::from_millis(10));
        }
        panic!("the step socket never opened");
    }

    fn ask(&mut self, order: &str) -> Value {
        writeln!(self.orders, "{order}").unwrap();
        let mut line = String::new();
        self.replies.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["ok"], true, "{order}: {line}");
        reply
    }
}

fn frame(packet: impl Encode) -> Vec<u8> {
    let mut body = Vec::new();
    packet.encode(&mut body);
    let mut frame = Vec::new();
    VarI32(body.len() as i32).encode(&mut frame);
    frame.extend(body);
    frame
}

#[test]
fn a_stepped_server_and_its_trace_agree_on_ticks_and_packets() {
    let id = std::process::id();
    let path = std::env::temp_dir().join(format!("void-step-trace-{id}.jsonl"));
    let socket = std::env::temp_dir().join(format!("void-step-trace-{id}.sock"));
    // SAFETY: this file holds one test, and nothing else runs yet.
    unsafe { std::env::set_var("VOID_TRACE", &path) };
    let subscriber = tracing_subscriber::registry().with(trace_layer());
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = ServerConfigBuilder::new()
        .address(format!("127.0.0.1:{port}"))
        .step_socket(&socket)
        .build();
    let server = std::thread::spawn(move || VoidServer::new(config).run());

    let mut pilot = Pilot::connect(&socket);
    assert_eq!(pilot.ask("status")["tick"], 0);
    assert_eq!(pilot.ask("step 2")["tick"], 2);

    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .write_all(&frame(HandshakePacket::Handshake(Handshake {
            protocol_version: PROTOCOL_VERSION,
            server_address: "localhost".to_string(),
            server_port: port,
            next_state: State::Login,
        })))
        .unwrap();
    client
        .write_all(&frame(LoginPacket::LoginStart(LoginStart {
            name: "sonde".to_string(),
            uuid: Uuid::nil(),
        })))
        .unwrap();
    assert_eq!(pilot.ask("step 3 2")["tick"], 5);
    let settled = pilot.ask("step 10");
    assert_eq!(settled["tick"], 15);

    let last = pilot.ask("stop");
    assert_eq!(last["tick"], 16, "stop runs one last tick");
    server.join().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let first_in = lines.iter().find(|line| line["dir"] == "in").unwrap();
    assert_eq!(first_in["packet"], "Handshake/Handshake");
    assert_eq!(first_in["tick"], 3, "the first tick after the barrier");
    let ticks = lines.iter().map(|line| line["tick"].as_u64().unwrap());
    assert!(ticks.max().unwrap() <= 16);

    let queued = lines
        .iter()
        .filter(|line| line["dir"] == "out" && line.get("dropped").is_none());
    let queued: Vec<&Value> = queued.collect();
    assert!(!queued.is_empty(), "{text}");
    assert_eq!(last["sent"], queued.len(), "{text}");
    assert!(
        queued.iter().all(|line| line["system"].is_string()),
        "{text}"
    );
}
