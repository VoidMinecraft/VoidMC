use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use uuid::Uuid;
use voidmc_codec::{Decode, Encode, VarI32};
use voidmc_protocol::clientbound as cb;
use voidmc_protocol::serverbound as sb;
use voidmc_protocol::{PROTOCOL_VERSION, State as Phase};

use crate::registry;

const LOGIN_SUCCESS: i32 = 0x02;
const CONFIG_FINISH: i32 = 0x03;
const CONFIG_KEEP_ALIVE: i32 = 0x04;
const CONFIG_KNOWN_PACKS: i32 = 0x0E;
const PLAY_ADD_ENTITY: i32 = 0x01;
const PLAY_DISCONNECT: i32 = 0x20;
const PLAY_POSITION_SYNC: i32 = 0x23;
const PLAY_UNLOAD_CHUNK: i32 = 0x25;
const PLAY_KEEP_ALIVE: i32 = 0x2C;
const PLAY_CHUNK: i32 = 0x2D;
const PLAY_PING: i32 = 0x3D;
const PLAY_PLAYER_POSITION: i32 = 0x48;
const PLAY_REMOVE_ENTITIES: i32 = 0x4D;
const PLAY_PACK_PUSH: i32 = 0x51;
const PLAY_SET_CAMERA: i32 = 0x5D;
const PLAY_START_CONFIGURATION: i32 = 0x76;
const PLAY_SYSTEM_CHAT: i32 = 0x79;
const PLAY_TELEPORT_ENTITY: i32 = 0x7D;
const SERVERBOUND_CONFIG_KEEP_ALIVE: i32 = 0x04;
const SERVERBOUND_CONFIGURATION_ACK: i32 = 0x10;
const SERVERBOUND_PACK_RESPONSE: i32 = 0x31;
const PACK_ACCEPTED_DOWNLOADED_LOADED: [i32; 3] = [3, 4, 0];
const RECONNECT_PAUSE: Duration = Duration::from_secs(1);
const DEFAULT_WAIT_MS: u64 = 20_000;

#[derive(Default)]
struct World {
    connected: bool,
    in_play: bool,
    position: [f64; 3],
    entities: HashMap<i32, [f64; 3]>,
    chunks: i64,
    chat: String,
    chat_count: u64,
    camera: i32,
    packets: BTreeMap<i32, u64>,
    bytes: u64,
    reconnects: u64,
    joined_ms: Option<u128>,
    last_error: Option<String>,
}

impl World {
    fn forget_level(&mut self) {
        self.entities.clear();
        self.chunks = 0;
        self.camera = 0;
    }

    fn fields(&self) -> Value {
        let state = if self.in_play {
            "connected"
        } else if self.connected {
            "connecting"
        } else {
            "disconnected"
        };
        json!({
            "state": state,
            "level": "bot",
            "x": self.position[0],
            "y": self.position[1],
            "z": self.position[2],
            "entities": self.entities.len(),
            "chunks": self.chunks,
            "camera": self.camera,
            "chat": self.chat,
            "chat_count": self.chat_count,
            "bytes": self.bytes,
            "reconnects": self.reconnects,
            "joined_ms": self.joined_ms,
            "last_error": self.last_error,
        })
    }
}

type Shared = Arc<Mutex<World>>;
type Writer = Arc<Mutex<Option<TcpStream>>>;

fn send<T: Encode>(writer: &Writer, packet: &T) -> std::io::Result<()> {
    let mut body = Vec::new();
    packet.encode(&mut body);
    let mut frame = Vec::new();
    VarI32(body.len() as i32).encode(&mut frame);
    frame.extend(body);
    match writer.lock().unwrap().as_mut() {
        Some(stream) => stream.write_all(&frame),
        None => Err(std::io::ErrorKind::NotConnected.into()),
    }
}

struct Raw(Vec<u8>);

impl Encode for Raw {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.0);
    }
}

fn raw(id: i32, body: &[u8]) -> Raw {
    let mut bytes = Vec::new();
    VarI32(id).encode(&mut bytes);
    bytes.extend_from_slice(body);
    Raw(bytes)
}

fn receive(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let (mut length, mut shift) = (0u32, 0);
    loop {
        let mut byte = [0u8];
        stream.read_exact(&mut byte)?;
        length |= ((byte[0] & 0x7F) as u32) << shift;
        shift += 7;
        if byte[0] & 0x80 == 0 {
            break;
        }
    }
    let mut body = vec![0; length as usize];
    stream.read_exact(&mut body)?;
    Ok(body)
}

fn invalid(error: impl std::fmt::Debug) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{error:?}"))
}

fn packet_id(rest: &mut &[u8]) -> std::io::Result<i32> {
    Ok(VarI32::decode(rest).map_err(invalid)?.0)
}

fn player_uuid(name: &str) -> Uuid {
    Uuid::from_u128(name.bytes().fold(0xB07u128, |hash, byte| {
        hash.wrapping_mul(131).wrapping_add(byte as u128)
    }))
}

fn login(stream: &mut TcpStream, writer: &Writer, name: &str, port: u16) -> std::io::Result<()> {
    send(
        writer,
        &sb::HandshakePacket::Handshake(sb::Handshake {
            protocol_version: PROTOCOL_VERSION,
            server_address: "127.0.0.1".into(),
            server_port: port,
            next_state: Phase::Login,
        }),
    )?;
    send(
        writer,
        &sb::LoginPacket::LoginStart(sb::LoginStart {
            name: name.chars().take(16).collect(),
            uuid: player_uuid(name),
        }),
    )?;
    loop {
        let body = receive(stream)?;
        if packet_id(&mut body.as_slice())? == LOGIN_SUCCESS {
            break;
        }
    }
    send(
        writer,
        &sb::LoginPacket::LoginAcknowledged(sb::LoginAcknowledged {}),
    )?;
    send(
        writer,
        &sb::ConfigurationPacket::ClientInformation(sb::ClientInformation {
            locale: "fr_fr".into(),
            view_distance: 8,
            chat_mode: sb::ChatMode::Enabled,
            chat_colors: true,
            skin_parts: 0x7F,
            main_hand: sb::MainHand::Right,
            enable_text_filtering: false,
            allow_server_listings: true,
            particle_status: sb::ParticleStatus::Minimal,
        }),
    )?;
    configuration(stream, writer)
}

fn configuration(stream: &mut TcpStream, writer: &Writer) -> std::io::Result<()> {
    loop {
        let body = receive(stream)?;
        let mut rest = body.as_slice();
        match packet_id(&mut rest)? {
            CONFIG_KEEP_ALIVE => send(writer, &raw(SERVERBOUND_CONFIG_KEEP_ALIVE, rest))?,
            CONFIG_KNOWN_PACKS => send(
                writer,
                &sb::ConfigurationPacket::KnownPacks(sb::KnownPacks {
                    known_packs: vec![sb::KnownPack {
                        namespace: "minecraft".into(),
                        id: "core".into(),
                        version: "26.1.2".into(),
                    }],
                }),
            )?,
            CONFIG_FINISH => {
                return send(
                    writer,
                    &sb::ConfigurationPacket::FinishConfigurationAcknowledged(
                        sb::FinishConfigurationAcknowledged {},
                    ),
                );
            }
            _ => {}
        }
    }
}

fn play(
    mut stream: TcpStream,
    writer: &Writer,
    world: &Shared,
    started: Instant,
) -> std::io::Result<()> {
    loop {
        let body = receive(&mut stream)?;
        let mut rest = body.as_slice();
        let id = packet_id(&mut rest)?;
        {
            let mut w = world.lock().unwrap();
            *w.packets.entry(id).or_default() += 1;
            w.bytes += body.len() as u64;
        }
        if id == PLAY_START_CONFIGURATION {
            world.lock().unwrap().forget_level();
            send(writer, &raw(SERVERBOUND_CONFIGURATION_ACK, &[]))?;
            configuration(&mut stream, writer)?;
            continue;
        }
        handle(id, rest, writer, world, started)?;
    }
}

fn handle(
    id: i32,
    mut rest: &[u8],
    writer: &Writer,
    world: &Shared,
    started: Instant,
) -> std::io::Result<()> {
    let mut w = world.lock().unwrap();
    match id {
        PLAY_KEEP_ALIVE => {
            let keep = cb::KeepAlive::decode(&mut rest).map_err(invalid)?;
            send(
                writer,
                &sb::PlayPacket::KeepAlive(sb::KeepAlive {
                    keep_alive_id: keep.keep_alive_id,
                }),
            )?;
        }
        PLAY_PING => {
            let ping = cb::Ping::decode(&mut rest).map_err(invalid)?;
            send(writer, &sb::PlayPacket::Pong(sb::Pong { id: ping.id }))?;
        }
        PLAY_PLAYER_POSITION => {
            let to = cb::SynchronizePlayerPosition::decode(&mut rest).map_err(invalid)?;
            w.position = [to.x, to.y, to.z];
            send(
                writer,
                &sb::PlayPacket::ConfirmTeleportation(sb::ConfirmTeleportation {
                    teleport_id: to.teleport_id,
                }),
            )?;
            send(
                writer,
                &sb::PlayPacket::SetPlayerPosAndRot(sb::SetPlayerPosAndRot {
                    x: to.x,
                    y: to.y,
                    z: to.z,
                    yaw: to.yaw,
                    pitch: to.pitch,
                    flags: 0,
                }),
            )?;
            if !w.in_play {
                w.in_play = true;
                w.joined_ms = Some(started.elapsed().as_millis());
                send(writer, &sb::PlayPacket::PlayerLoaded(sb::PlayerLoaded {}))?;
            }
        }
        PLAY_PACK_PUSH => {
            let id = rest.get(..16).ok_or_else(|| invalid("short pack id"))?;
            for action in PACK_ACCEPTED_DOWNLOADED_LOADED {
                let mut body = id.to_vec();
                VarI32(action).encode(&mut body);
                send(writer, &raw(SERVERBOUND_PACK_RESPONSE, &body))?;
            }
        }
        PLAY_ADD_ENTITY => {
            let entity = cb::SpawnEntity::decode(&mut rest).map_err(invalid)?;
            w.entities
                .insert(entity.entity_id, [entity.x, entity.y, entity.z]);
        }
        PLAY_POSITION_SYNC => {
            let entity = cb::EntityPositionSync::decode(&mut rest).map_err(invalid)?;
            w.entities
                .insert(entity.entity_id, [entity.x, entity.y, entity.z]);
        }
        PLAY_TELEPORT_ENTITY => {
            let entity = cb::TeleportEntity::decode(&mut rest).map_err(invalid)?;
            w.entities
                .insert(entity.entity_id, [entity.x, entity.y, entity.z]);
        }
        PLAY_REMOVE_ENTITIES => {
            let count = VarI32::decode(&mut rest).map_err(invalid)?.0;
            for _ in 0..count {
                let gone = VarI32::decode(&mut rest).map_err(invalid)?.0;
                w.entities.remove(&gone);
            }
        }
        PLAY_CHUNK => w.chunks += 1,
        PLAY_UNLOAD_CHUNK => w.chunks -= 1,
        PLAY_SET_CAMERA => w.camera = VarI32::decode(&mut rest).map_err(invalid)?.0,
        PLAY_SYSTEM_CHAT => {
            if let Ok(message) = cb::SystemChat::decode(&mut rest) {
                w.chat = format!("{:?}", message.content);
                w.chat_count += 1;
            }
        }
        PLAY_DISCONNECT => return Err(std::io::ErrorKind::ConnectionReset.into()),
        _ => {}
    }
    Ok(())
}

fn session(name: &str, port: u16, writer: &Writer, world: &Shared) -> std::io::Result<()> {
    let started = Instant::now();
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_nodelay(true)?;
    *writer.lock().unwrap() = Some(stream.try_clone()?);
    world.lock().unwrap().connected = true;
    login(&mut stream, writer, name, port)?;
    play(stream, writer, world, started)
}

/// Stays on its server: a restart only costs it a reconnection.
fn stay_connected(name: String, port: u16, writer: Writer, world: Shared) {
    loop {
        let result = session(&name, port, &writer, &world);
        let was_in_play = {
            let mut w = world.lock().unwrap();
            let was = w.in_play;
            w.in_play = false;
            w.connected = false;
            w.forget_level();
            if let Err(error) = result {
                w.last_error = Some(error.to_string());
            }
            was
        };
        *writer.lock().unwrap() = None;
        if was_in_play {
            world.lock().unwrap().reconnects += 1;
        }
        thread::sleep(RECONNECT_PAUSE);
    }
}

/// `field op value`, with `>=`, `<=`, `==`, `!=` and `~` for "contains".
pub fn holds(condition: &str, fields: &Value) -> Result<bool, String> {
    let (field, op, expected) = ["<=", ">=", "==", "!=", "~"]
        .iter()
        .find_map(|op| {
            condition
                .split_once(op)
                .map(|(f, e)| (f.trim(), *op, e.trim()))
        })
        .ok_or_else(|| format!("bad condition {condition}"))?;
    let actual = &fields[field];
    if actual.is_null() {
        return Ok(false);
    }
    let text = actual
        .as_str()
        .map_or_else(|| actual.to_string(), str::to_string);
    if op == "~" {
        return Ok(text.contains(expected));
    }
    if let (Some(a), Ok(b)) = (actual.as_f64(), expected.parse::<f64>()) {
        return Ok(match op {
            ">=" => a >= b,
            "<=" => a <= b,
            "==" => a == b,
            _ => a != b,
        });
    }
    Ok((op == "==") == (text == expected))
}

fn answer(line: &str, writer: &Writer, world: &Shared) -> Result<Value, String> {
    let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
    match verb {
        "state" => Ok(world.lock().unwrap().fields()),
        "act" => {
            send(
                writer,
                &sb::PlayPacket::ChatCommand(sb::ChatCommand {
                    command: rest.into(),
                }),
            )
            .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "assert" => {
            let fields = world.lock().unwrap().fields();
            if holds(rest, &fields)? {
                Ok(fields)
            } else {
                Err(format!("{rest} is false: {fields}"))
            }
        }
        "wait_until" => {
            let words: Vec<&str> = rest.split_whitespace().collect();
            let timed = words.len() > 1 && words[words.len() - 1].parse::<u64>().is_ok();
            let limit = if timed {
                words[words.len() - 1].parse().unwrap_or(DEFAULT_WAIT_MS)
            } else {
                DEFAULT_WAIT_MS
            };
            let condition = words[..words.len() - usize::from(timed)].join(" ");
            let start = Instant::now();
            loop {
                let fields = world.lock().unwrap().fields();
                if holds(&condition, &fields)? {
                    return Ok(fields);
                }
                if start.elapsed() > Duration::from_millis(limit) {
                    return Err(format!("timed out on {condition}"));
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        "entities" => {
            let w = world.lock().unwrap();
            let all: Vec<String> = w
                .entities
                .iter()
                .map(|(id, at)| format!("{id} {} {} {}", at[0], at[1], at[2]))
                .collect();
            Ok(json!({ "entities": all }))
        }
        "packets" => {
            let w = world.lock().unwrap();
            let counts: BTreeMap<String, u64> = w
                .packets
                .iter()
                .map(|(id, n)| (format!("0x{id:02X}"), *n))
                .collect();
            Ok(json!({ "packets": counts }))
        }
        "quit" => std::process::exit(0),
        "shot" | "thumb" | "show" | "hide" => Err("a bot has no picture and no window".into()),
        other => Err(format!("unknown verb {other}")),
    }
}

fn serve(stream: UnixStream, writer: Writer, world: Shared) -> std::io::Result<()> {
    let mut out = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
        let reply = match answer(line?.trim(), &writer, &world) {
            Ok(Value::Object(mut fields)) => {
                fields.insert("ok".into(), json!(true));
                Value::Object(fields)
            }
            Ok(other) => json!({ "ok": true, "value": other }),
            Err(error) => json!({ "ok": false, "error": error }),
        };
        writeln!(out, "{reply}")?;
    }
    Ok(())
}

pub fn run(name: &str, port: u16) -> Result<(), String> {
    let socket = registry::socket(name);
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
    let world = Shared::default();
    let writer = Writer::default();
    let (w, out, label) = (world.clone(), writer.clone(), name.to_string());
    thread::spawn(move || stay_connected(label, port, out, w));
    for stream in listener.incoming().flatten() {
        let (writer, world) = (writer.clone(), world.clone());
        thread::spawn(move || serve(stream, writer, world));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_compare_numbers_and_text() {
        let fields = json!({"chunks": 814, "state": "connected", "chat": "Bienvenue sur la carte"});
        assert!(holds("chunks>=800", &fields).unwrap());
        assert!(!holds("chunks<=800", &fields).unwrap());
        assert!(holds("state==connected", &fields).unwrap());
        assert!(holds("state!=disconnected", &fields).unwrap());
        assert!(holds("chat~carte", &fields).unwrap());
        assert!(holds("chunks == 814", &fields).unwrap());
    }

    #[test]
    fn a_missing_field_is_false_and_a_bad_condition_an_error() {
        assert!(!holds("nope>=1", &json!({})).unwrap());
        assert!(holds("chunks", &json!({})).is_err());
    }

    #[test]
    fn leaving_a_level_forgets_what_was_on_it() {
        let mut world = World {
            chunks: 5,
            camera: 3,
            ..Default::default()
        };
        world.entities.insert(1, [0.0; 3]);
        world.forget_level();
        assert_eq!(
            (world.chunks, world.camera, world.entities.len()),
            (0, 0, 0)
        );
    }

    fn connected_writer() -> (Writer, TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        (Arc::new(Mutex::new(Some(client))), server)
    }

    #[test]
    fn a_pack_push_is_answered_accepted_downloaded_loaded_with_its_id() {
        let (writer, mut server) = connected_writer();
        let id = Uuid::from_u128(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        let mut push = Vec::new();
        id.encode(&mut push);
        "http://127.0.0.1:25701/pack.zip"
            .to_string()
            .encode(&mut push);
        "".to_string().encode(&mut push);
        true.encode(&mut push);
        false.encode(&mut push);

        handle(
            PLAY_PACK_PUSH,
            &push,
            &writer,
            &Shared::default(),
            Instant::now(),
        )
        .unwrap();

        for action in [3u8, 4, 0] {
            let mut expected = vec![SERVERBOUND_PACK_RESPONSE as u8];
            expected.extend_from_slice(id.as_bytes());
            expected.push(action);
            assert_eq!(receive(&mut server).unwrap(), expected);
        }
    }

    #[test]
    fn a_pack_push_too_short_for_an_id_is_refused_without_an_answer() {
        let (writer, mut server) = connected_writer();
        let result = handle(
            PLAY_PACK_PUSH,
            &[0; 15],
            &writer,
            &Shared::default(),
            Instant::now(),
        );
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        writer.lock().unwrap().take();
        assert_eq!(
            receive(&mut server).unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn a_player_keeps_its_uuid_from_one_run_to_the_next() {
        assert_eq!(player_uuid("Bot"), player_uuid("Bot"));
        assert_ne!(player_uuid("Bot"), player_uuid("Autre"));
    }
}
