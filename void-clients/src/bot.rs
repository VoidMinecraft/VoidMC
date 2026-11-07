use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use ussr_nbt::owned::{Compound, List, Nbt, Tag};
use uuid::Uuid;
use voidmc_codec::{Decode, Encode, VarI32};
use voidmc_data::Version;
use voidmc_protocol::clientbound as cb;
use voidmc_protocol::serverbound as sb;
use voidmc_protocol::{PROTOCOL_VERSION, State as Phase};

use crate::condition::{self, Condition};
use crate::{frame, registry};

const VERSION: Version = Version::V26_1_2;
const LOGIN: &str = "login";
const CONFIGURATION: &str = "configuration";
const PLAY: &str = "play";
const PACK_ACCEPTED_DOWNLOADED_LOADED: [i32; 3] = [3, 4, 0];
const FIRST_PAUSE: Duration = Duration::from_millis(100);
const LONGEST_PAUSE: Duration = Duration::from_secs(1);
const STEADY_PLAY: Duration = Duration::from_secs(1);
const CONNECT_LIMIT: Duration = Duration::from_secs(2);
const SILENCE_LIMIT: Duration = Duration::from_secs(30);
const IDLE_CHECK: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Backoff {
    failures: u32,
}

#[derive(Debug, PartialEq)]
enum Ending {
    PortClosed,
    Ended(Option<Duration>),
}

impl Backoff {
    fn after(&mut self, ending: Ending) -> Duration {
        let played = match ending {
            Ending::PortClosed => {
                self.failures = 0;
                return FIRST_PAUSE;
            }
            Ending::Ended(played) => played,
        };
        if played.is_some_and(|played| played >= STEADY_PLAY) {
            self.failures = 0;
        }
        let pause = FIRST_PAUSE
            .saturating_mul(1 << self.failures.min(4))
            .min(LONGEST_PAUSE);
        self.failures = self.failures.saturating_add(1);
        pause
    }
}

macro_rules! packets {
    ($($constant:ident: $state:ident $direction:literal $name:literal,)*) => {
        $(const $constant: &str = $name;)*
        #[cfg(test)]
        const PACKETS: &[(&str, &str, &str)] = &[$(($state, $direction, $name)),*];
    };
}

packets! {
    LOGIN_SUCCESS: LOGIN "clientbound" "minecraft:login_finished",
    LOGIN_COMPRESSION: LOGIN "clientbound" "minecraft:login_compression",
    LOGIN_DISCONNECT: LOGIN "clientbound" "minecraft:login_disconnect",
    LOGIN_CUSTOM_QUERY: LOGIN "clientbound" "minecraft:custom_query",
    LOGIN_COOKIE: LOGIN "clientbound" "minecraft:cookie_request",
    CONFIG_COOKIE: CONFIGURATION "clientbound" "minecraft:cookie_request",
    CONFIG_DISCONNECT: CONFIGURATION "clientbound" "minecraft:disconnect",
    CONFIG_PING: CONFIGURATION "clientbound" "minecraft:ping",
    CONFIG_PACK_PUSH: CONFIGURATION "clientbound" "minecraft:resource_pack_push",
    CONFIG_FINISH: CONFIGURATION "clientbound" "minecraft:finish_configuration",
    CONFIG_KEEP_ALIVE: CONFIGURATION "clientbound" "minecraft:keep_alive",
    CONFIG_KNOWN_PACKS: CONFIGURATION "clientbound" "minecraft:select_known_packs",
    PLAY_ADD_ENTITY: PLAY "clientbound" "minecraft:add_entity",
    PLAY_COOKIE: PLAY "clientbound" "minecraft:cookie_request",
    PLAY_MOVE_ENTITY: PLAY "clientbound" "minecraft:move_entity_pos",
    PLAY_MOVE_ENTITY_ROTATING: PLAY "clientbound" "minecraft:move_entity_pos_rot",
    PLAY_DISCONNECT: PLAY "clientbound" "minecraft:disconnect",
    PLAY_POSITION_SYNC: PLAY "clientbound" "minecraft:entity_position_sync",
    PLAY_UNLOAD_CHUNK: PLAY "clientbound" "minecraft:forget_level_chunk",
    PLAY_KEEP_ALIVE: PLAY "clientbound" "minecraft:keep_alive",
    PLAY_CHUNK: PLAY "clientbound" "minecraft:level_chunk_with_light",
    PLAY_PING: PLAY "clientbound" "minecraft:ping",
    PLAY_PLAYER_POSITION: PLAY "clientbound" "minecraft:player_position",
    PLAY_REMOVE_ENTITIES: PLAY "clientbound" "minecraft:remove_entities",
    PLAY_PACK_PUSH: PLAY "clientbound" "minecraft:resource_pack_push",
    PLAY_SET_CAMERA: PLAY "clientbound" "minecraft:set_camera",
    PLAY_START_CONFIGURATION: PLAY "clientbound" "minecraft:start_configuration",
    PLAY_SYSTEM_CHAT: PLAY "clientbound" "minecraft:system_chat",
    PLAY_TELEPORT_ENTITY: PLAY "clientbound" "minecraft:teleport_entity",
    SERVERBOUND_CONFIG_KEEP_ALIVE: CONFIGURATION "serverbound" "minecraft:keep_alive",
    SERVERBOUND_CONFIG_PONG: CONFIGURATION "serverbound" "minecraft:pong",
    SERVERBOUND_CONFIG_PACK_RESPONSE: CONFIGURATION "serverbound" "minecraft:resource_pack",
    SERVERBOUND_LOGIN_QUERY_ANSWER: LOGIN "serverbound" "minecraft:custom_query_answer",
    SERVERBOUND_LOGIN_COOKIE: LOGIN "serverbound" "minecraft:cookie_response",
    SERVERBOUND_CONFIG_COOKIE: CONFIGURATION "serverbound" "minecraft:cookie_response",
    SERVERBOUND_PLAY_COOKIE: PLAY "serverbound" "minecraft:cookie_response",
    SERVERBOUND_CONFIGURATION_ACK: PLAY "serverbound" "minecraft:configuration_acknowledged",
    SERVERBOUND_PACK_RESPONSE: PLAY "serverbound" "minecraft:resource_pack",
}

#[derive(Default)]
struct World {
    connected: bool,
    in_play: bool,
    position: [f64; 3],
    entities: HashMap<i32, [f64; 3]>,
    chunks: HashSet<(i32, i32)>,
    chat: String,
    action_bar: String,
    chat_count: u64,
    camera: i32,
    packets: BTreeMap<i32, u64>,
    bytes: u64,
    reconnects: u64,
    joined_ms: Option<u128>,
    playing_since: Option<Instant>,
    last_error: Option<String>,
}

impl World {
    fn enter_play(&mut self, started: Instant) -> bool {
        if self.in_play {
            return false;
        }
        self.in_play = true;
        self.playing_since = Some(Instant::now());
        self.joined_ms = Some(started.elapsed().as_millis());
        true
    }

    fn end_session(&mut self, error: Option<std::io::Error>) -> Ending {
        let port_closed = error
            .as_ref()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::ConnectionRefused);
        let played = self.playing_since.take().map(|since| since.elapsed());
        if self.in_play {
            self.reconnects += 1;
        }
        self.in_play = false;
        self.connected = false;
        self.forget_level();
        if let Some(error) = error {
            self.last_error = Some(error.to_string());
        }
        if port_closed {
            Ending::PortClosed
        } else {
            Ending::Ended(played)
        }
    }

    fn forget_level(&mut self) {
        self.entities.clear();
        self.chunks.clear();
        self.camera = 0;
    }

    fn shift(&mut self, entity: i32, delta: [i16; 3]) {
        if let Some(at) = self.entities.get_mut(&entity) {
            for (axis, step) in at.iter_mut().zip(delta) {
                *axis += f64::from(step) / 4096.0;
            }
        }
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
            "version": crate::version::current(),
            "x": self.position[0],
            "y": self.position[1],
            "z": self.position[2],
            "entities": self.entities.len(),
            "chunks": self.chunks.len(),
            "camera": self.camera,
            "chat": self.chat,
            "action_bar": self.action_bar,
            "chat_count": self.chat_count,
            "bytes": self.bytes,
            "reconnects": self.reconnects,
            "joined_ms": self.joined_ms,
            "last_error": self.last_error,
        })
    }
}

type Shared = Arc<Mutex<World>>;
type Writer = Arc<Mutex<Option<Link>>>;

struct Link {
    stream: TcpStream,
    threshold: Option<usize>,
}

struct Reader {
    stream: TcpStream,
    threshold: Option<usize>,
}

impl Reader {
    fn next(&mut self) -> std::io::Result<Vec<u8>> {
        frame::read(&mut self.stream, self.threshold)
    }
}

fn send<T: Encode>(writer: &Writer, packet: &T) -> std::io::Result<()> {
    let mut body = Vec::new();
    packet.encode(&mut body);
    let mut link = writer.lock().unwrap();
    let link = link.as_mut().ok_or(std::io::ErrorKind::NotConnected)?;
    let frame = frame::encode(&body, link.threshold)?;
    link.stream.write_all(&frame)
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

fn refused(reason: String) -> std::io::Error {
    std::io::Error::other(format!("the server sent the bot away: {reason}"))
}

fn component(rest: &mut &[u8]) -> std::io::Result<String> {
    if rest.first() == Some(&0x08) {
        let length = rest
            .get(1..3)
            .map(|bytes| usize::from(u16::from_be_bytes([bytes[0], bytes[1]])))
            .ok_or_else(|| invalid("short text"))?;
        let text = rest
            .get(3..3 + length)
            .ok_or_else(|| invalid("short text"))?;
        let text = String::from_utf8_lossy(text).into_owned();
        *rest = &rest[3 + length..];
        return Ok(text);
    }
    let nbt = Nbt::decode(rest).map_err(invalid)?;
    let mut text = String::new();
    plain(&nbt.compound, language(), &mut text);
    Ok(text)
}

type Language = HashMap<String, String>;

fn language() -> &'static Language {
    static ENGLISH: OnceLock<Language> = OnceLock::new();
    ENGLISH.get_or_init(|| {
        std::process::Command::new("unzip")
            .arg("-p")
            .arg(crate::launch::game_jar())
            .arg("assets/minecraft/lang/en_us.json")
            .output()
            .ok()
            .and_then(|out| serde_json::from_slice(&out.stdout).ok())
            .unwrap_or_default()
    })
}

fn tag<'a>(compound: &'a Compound, name: &str) -> Option<&'a Tag> {
    compound
        .tags
        .iter()
        .find(|(key, _)| key.to_string() == name)
        .map(|(_, tag)| tag)
}

fn parts(list: Option<&Tag>, language: &Language) -> Vec<String> {
    match list {
        Some(Tag::List(List::Compound(parts))) => parts
            .iter()
            .map(|part| {
                let mut text = String::new();
                plain(part, language, &mut text);
                text
            })
            .collect(),
        Some(Tag::List(List::String(parts))) => parts.iter().map(|p| p.to_string()).collect(),
        _ => Vec::new(),
    }
}

fn plain(compound: &Compound, language: &Language, text: &mut String) {
    match (tag(compound, "text"), tag(compound, "translate")) {
        (Some(Tag::String(value)), _) => text.push_str(&value.to_string()),
        (_, Some(Tag::String(key))) => {
            let key = key.to_string();
            let fallback = match tag(compound, "fallback") {
                Some(Tag::String(fallback)) => fallback.to_string(),
                _ => key.clone(),
            };
            let pattern = language.get(&key).unwrap_or(&fallback);
            text.push_str(&format_translation(
                pattern,
                &parts(tag(compound, "with"), language),
            ));
        }
        _ => {}
    }
    for part in parts(tag(compound, "extra"), language) {
        text.push_str(&part);
    }
}

fn format_translation(pattern: &str, arguments: &[String]) -> String {
    let mut out = String::new();
    let mut next = 0;
    let mut rest = pattern;
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        if let Some(after) = rest.strip_prefix('%') {
            out.push('%');
            rest = after;
            continue;
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let position = rest[..digits]
            .parse::<usize>()
            .ok()
            .filter(|_| rest[digits..].starts_with("$s"));
        let (index, skip) = match (position, rest.starts_with('s')) {
            (Some(n), _) => (n.checked_sub(1), digits + 2),
            (None, true) => (Some(next), 1),
            _ => {
                out.push('%');
                continue;
            }
        };
        if position.is_none() {
            next += 1;
        }
        out.push_str(
            index
                .and_then(|i| arguments.get(i))
                .map_or("", String::as_str),
        );
        rest = &rest[skip..];
    }
    out.push_str(rest);
    out
}

fn cookie(writer: &Writer, state: &str, answer: &str, mut rest: &[u8]) -> std::io::Result<()> {
    let key = String::decode(&mut rest).map_err(invalid)?;
    let mut body = Vec::new();
    key.encode(&mut body);
    false.encode(&mut body);
    send(writer, &raw(serverbound(state, answer), &body))
}

fn answer_pack(writer: &Writer, state: &str, answer: &str, rest: &[u8]) -> std::io::Result<()> {
    let id = rest.get(..16).ok_or_else(|| invalid("short pack id"))?;
    for action in PACK_ACCEPTED_DOWNLOADED_LOADED {
        let mut body = id.to_vec();
        VarI32(action).encode(&mut body);
        send(writer, &raw(serverbound(state, answer), &body))?;
    }
    Ok(())
}

fn invalid(error: impl std::fmt::Debug) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{error:?}"))
}

fn packet_id(rest: &mut &[u8]) -> std::io::Result<i32> {
    Ok(VarI32::decode(rest).map_err(invalid)?.0)
}

fn clientbound(state: &str, id: i32) -> Option<&'static str> {
    voidmc_data::packets(VERSION, state, "clientbound")?
        .iter()
        .find(|(_, packet)| *packet == id)
        .map(|(name, _)| *name)
}

fn serverbound(state: &str, name: &str) -> i32 {
    voidmc_data::packet_id(VERSION, state, "serverbound", name)
        .unwrap_or_else(|| panic!("{name} is not a {state} packet of the reference"))
}

fn player_uuid(name: &str) -> Uuid {
    Uuid::from_u128(name.bytes().fold(0xB07u128, |hash, byte| {
        hash.wrapping_mul(131).wrapping_add(byte as u128)
    }))
}

fn login(reader: &mut Reader, writer: &Writer, name: &str, port: u16) -> std::io::Result<()> {
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
        let body = reader.next()?;
        let mut rest = body.as_slice();
        match clientbound(LOGIN, packet_id(&mut rest)?) {
            Some(LOGIN_SUCCESS) => break,
            Some(LOGIN_COMPRESSION) => {
                let threshold = usize::try_from(VarI32::decode(&mut rest).map_err(invalid)?.0).ok();
                reader.threshold = threshold;
                if let Some(link) = writer.lock().unwrap().as_mut() {
                    link.threshold = threshold;
                }
            }
            Some(LOGIN_DISCONNECT) => {
                return Err(refused(String::decode(&mut rest).map_err(invalid)?));
            }
            Some(LOGIN_CUSTOM_QUERY) => {
                let mut body = Vec::new();
                VarI32::decode(&mut rest)
                    .map_err(invalid)?
                    .encode(&mut body);
                false.encode(&mut body);
                send(
                    writer,
                    &raw(serverbound(LOGIN, SERVERBOUND_LOGIN_QUERY_ANSWER), &body),
                )?;
            }
            Some(LOGIN_COOKIE) => cookie(writer, LOGIN, SERVERBOUND_LOGIN_COOKIE, rest)?,
            _ => {}
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
    configuration(reader, writer)
}

fn configuration(reader: &mut Reader, writer: &Writer) -> std::io::Result<()> {
    loop {
        let body = reader.next()?;
        let mut rest = body.as_slice();
        match clientbound(CONFIGURATION, packet_id(&mut rest)?) {
            Some(CONFIG_KEEP_ALIVE) => send(
                writer,
                &raw(
                    serverbound(CONFIGURATION, SERVERBOUND_CONFIG_KEEP_ALIVE),
                    rest,
                ),
            )?,
            Some(CONFIG_KNOWN_PACKS) => send(
                writer,
                &sb::ConfigurationPacket::KnownPacks(sb::KnownPacks {
                    known_packs: vec![sb::KnownPack {
                        namespace: "minecraft".into(),
                        id: "core".into(),
                        version: "26.1.2".into(),
                    }],
                }),
            )?,
            Some(CONFIG_PING) => send(
                writer,
                &raw(serverbound(CONFIGURATION, SERVERBOUND_CONFIG_PONG), rest),
            )?,
            Some(CONFIG_COOKIE) => cookie(writer, CONFIGURATION, SERVERBOUND_CONFIG_COOKIE, rest)?,
            Some(CONFIG_PACK_PUSH) => {
                answer_pack(
                    writer,
                    CONFIGURATION,
                    SERVERBOUND_CONFIG_PACK_RESPONSE,
                    rest,
                )?;
            }
            Some(CONFIG_DISCONNECT) => return Err(refused(component(&mut rest)?)),
            Some(CONFIG_FINISH) => {
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
    mut reader: Reader,
    writer: &Writer,
    world: &Shared,
    started: Instant,
) -> std::io::Result<()> {
    loop {
        let body = reader.next()?;
        let mut rest = body.as_slice();
        let id = packet_id(&mut rest)?;
        {
            let mut w = world.lock().unwrap();
            *w.packets.entry(id).or_default() += 1;
            w.bytes += body.len() as u64;
        }
        let Some(name) = clientbound(PLAY, id) else {
            continue;
        };
        if name == PLAY_START_CONFIGURATION {
            world.lock().unwrap().forget_level();
            send(
                writer,
                &raw(serverbound(PLAY, SERVERBOUND_CONFIGURATION_ACK), &[]),
            )?;
            configuration(&mut reader, writer)?;
            continue;
        }
        handle(name, rest, writer, world, started)?;
    }
}

fn handle(
    name: &str,
    mut rest: &[u8],
    writer: &Writer,
    world: &Shared,
    started: Instant,
) -> std::io::Result<()> {
    let mut w = world.lock().unwrap();
    match name {
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
            if w.enter_play(started) {
                send(writer, &sb::PlayPacket::PlayerLoaded(sb::PlayerLoaded {}))?;
            }
        }
        PLAY_PACK_PUSH => answer_pack(writer, PLAY, SERVERBOUND_PACK_RESPONSE, rest)?,
        PLAY_COOKIE => cookie(writer, PLAY, SERVERBOUND_PLAY_COOKIE, rest)?,
        PLAY_ADD_ENTITY => {
            let entity = cb::SpawnEntity::decode(&mut rest).map_err(invalid)?;
            w.entities
                .insert(entity.entity_id, [entity.x, entity.y, entity.z]);
        }
        PLAY_POSITION_SYNC => {
            let entity = cb::EntityPositionSync::decode(&mut rest).map_err(invalid)?;
            if let Some(at) = w.entities.get_mut(&entity.entity_id) {
                *at = [entity.x, entity.y, entity.z];
            }
        }
        PLAY_TELEPORT_ENTITY => {
            let entity = cb::TeleportEntity::decode(&mut rest).map_err(invalid)?;
            if let Some(at) = w.entities.get_mut(&entity.entity_id) {
                *at = [entity.x, entity.y, entity.z];
            }
        }
        PLAY_MOVE_ENTITY => {
            let step = cb::UpdateEntityPosition::decode(&mut rest).map_err(invalid)?;
            w.shift(step.entity_id, [step.delta_x, step.delta_y, step.delta_z]);
        }
        PLAY_MOVE_ENTITY_ROTATING => {
            let step = cb::UpdateEntityPositionAndRotation::decode(&mut rest).map_err(invalid)?;
            w.shift(step.entity_id, [step.delta_x, step.delta_y, step.delta_z]);
        }
        PLAY_REMOVE_ENTITIES => {
            let count = VarI32::decode(&mut rest).map_err(invalid)?.0;
            for _ in 0..count {
                let gone = VarI32::decode(&mut rest).map_err(invalid)?.0;
                w.entities.remove(&gone);
            }
        }
        PLAY_CHUNK => {
            let x = i32::decode(&mut rest).map_err(invalid)?;
            let z = i32::decode(&mut rest).map_err(invalid)?;
            w.chunks.insert((x, z));
        }
        PLAY_UNLOAD_CHUNK => {
            let gone = cb::UnloadChunk::decode(&mut rest).map_err(invalid)?;
            w.chunks.remove(&(gone.chunk_x, gone.chunk_z));
        }
        PLAY_SET_CAMERA => w.camera = VarI32::decode(&mut rest).map_err(invalid)?.0,
        PLAY_SYSTEM_CHAT => {
            let text = component(&mut rest)?;
            if bool::decode(&mut rest).map_err(invalid)? {
                w.action_bar = text;
            } else {
                w.chat = text;
                w.chat_count += 1;
            }
        }
        PLAY_DISCONNECT => return Err(refused(component(&mut rest)?)),
        _ => {}
    }
    Ok(())
}

fn session(name: &str, port: u16, writer: &Writer, world: &Shared) -> std::io::Result<()> {
    let started = Instant::now();
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let stream = TcpStream::connect_timeout(&address, CONNECT_LIMIT).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!(
                "the bot waited up to {} s for {address} to accept: {error}",
                CONNECT_LIMIT.as_secs()
            ),
        )
    })?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(SILENCE_LIMIT))?;
    stream.set_write_timeout(Some(SILENCE_LIMIT))?;
    *writer.lock().unwrap() = Some(Link {
        stream: stream.try_clone()?,
        threshold: None,
    });
    world.lock().unwrap().connected = true;
    let mut reader = Reader {
        stream,
        threshold: None,
    };
    login(&mut reader, writer, name, port)?;
    play(reader, writer, world, started)
}

/// Stays on its server: a restart only costs it a reconnection.
fn stay_connected(name: String, port: u16, writer: Writer, world: Shared) {
    let mut backoff = Backoff::default();
    loop {
        thread::sleep(step(&name, port, &writer, &world, &mut backoff));
    }
}

fn step(name: &str, port: u16, writer: &Writer, world: &Shared, backoff: &mut Backoff) -> Duration {
    let result = session(name, port, writer, world);
    *writer.lock().unwrap() = None;
    pause_after(backoff, world, result)
}

fn pause_after(backoff: &mut Backoff, world: &Shared, result: std::io::Result<()>) -> Duration {
    let ending = world.lock().unwrap().end_session(result.err());
    backoff.after(ending)
}

fn answer(line: &str, writer: &Writer, world: &Shared) -> Result<Value, String> {
    let (verb, rest) = condition::split(line);
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
            let condition = Condition::parse(rest)?;
            let fields = world.lock().unwrap().fields();
            if condition.holds(&fields)? {
                Ok(fields)
            } else {
                Err(format!("{rest} is false: {fields}"))
            }
        }
        "wait_until" => {
            let (condition, limit) = condition::wait(rest)?;
            let start = Instant::now();
            loop {
                let fields = world.lock().unwrap().fields();
                if condition.holds(&fields)? {
                    return Ok(fields);
                }
                if start.elapsed() > Duration::from_millis(limit) {
                    return Err(format!("timed out on {rest}"));
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

type LastVerb = Arc<Mutex<Instant>>;

fn serve(
    stream: UnixStream,
    writer: Writer,
    world: Shared,
    last_verb: LastVerb,
) -> std::io::Result<()> {
    let mut out = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
        let line = line?;
        if !matches!(condition::split(&line).0, "state" | "thumb") {
            *last_verb.lock().unwrap() = Instant::now();
        }
        let mut reply = match answer(&line, &writer, &world) {
            Ok(Value::Object(mut fields)) => {
                fields.insert("ok".into(), json!(true));
                Value::Object(fields)
            }
            Ok(other) => json!({ "ok": true, "value": other }),
            Err(error) => json!({ "ok": false, "error": error }),
        };
        reply["version"] = json!(crate::version::current());
        writeln!(out, "{reply}")?;
    }
    Ok(())
}

fn stop_when_idle(socket: std::path::PathBuf, last_verb: LastVerb, idle: Duration) {
    loop {
        thread::sleep(IDLE_CHECK);
        if last_verb.lock().unwrap().elapsed() > idle {
            let _ = fs::remove_file(&socket);
            std::process::exit(0);
        }
    }
}

pub fn run(name: &str, port: u16, idle: Duration) -> Result<(), String> {
    let socket = registry::socket(name)?;
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
    let world = Shared::default();
    let writer = Writer::default();
    let last_verb = LastVerb::new(Mutex::new(Instant::now()));
    let (w, out, label) = (world.clone(), writer.clone(), name.to_string());
    thread::spawn(move || stay_connected(label, port, out, w));
    let watched = last_verb.clone();
    thread::spawn(move || stop_when_idle(socket, watched, idle));
    for stream in listener.incoming().flatten() {
        let (writer, world, last_verb) = (writer.clone(), world.clone(), last_verb.clone());
        thread::spawn(move || serve(stream, writer, world, last_verb));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reconnection_starts_quick_and_slows_to_one_second() {
        let mut backoff = Backoff::default();
        let pauses: Vec<u128> = (0..7)
            .map(|_| backoff.after(Ending::Ended(None)).as_millis())
            .collect();
        assert_eq!(pauses, [100, 200, 400, 800, 1000, 1000, 1000]);
        backoff.failures = u32::MAX;
        assert_eq!(backoff.after(Ending::Ended(None)), LONGEST_PAUSE);
    }

    #[test]
    fn a_session_tells_how_it_ended_once() {
        let mut world = World::default();
        assert_eq!(world.end_session(None), Ending::Ended(None));
        assert!(world.enter_play(Instant::now()));
        assert!(!world.enter_play(Instant::now()));
        thread::sleep(Duration::from_millis(20));
        let Ending::Ended(Some(played)) = world.end_session(None) else {
            panic!("a game that was played tells how long");
        };
        assert!(played >= Duration::from_millis(20));
        assert_eq!(world.reconnects, 1);
        assert!(!world.in_play);
        assert_eq!(world.end_session(None), Ending::Ended(None));
        assert_eq!(world.reconnects, 1);
        let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        assert_eq!(world.end_session(Some(refused)), Ending::PortClosed);
        let reset = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        assert_eq!(world.end_session(Some(reset)), Ending::Ended(None));
    }

    #[test]
    fn a_closed_port_is_tried_again_soon_and_starts_the_pauses_over() {
        let mut backoff = Backoff::default();
        for _ in 0..5 {
            backoff.after(Ending::Ended(None));
        }
        assert_eq!(backoff.after(Ending::PortClosed), FIRST_PAUSE);
        assert_eq!(backoff.after(Ending::PortClosed), FIRST_PAUSE);
        assert_eq!(backoff.after(Ending::Ended(None)), FIRST_PAUSE);
        assert_eq!(backoff.after(Ending::Ended(None)), FIRST_PAUSE * 2);
    }

    #[test]
    fn a_closed_port_is_told_by_the_bot_with_what_it_waited_for() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let world: Shared = Default::default();
        let writer: Writer = Default::default();
        let error = session("bot", port, &writer, &world).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionRefused);
        assert!(
            error
                .to_string()
                .contains(&format!("127.0.0.1:{port} to accept")),
            "{error}"
        );
        let mut backoff = Backoff::default();
        for _ in 0..5 {
            backoff.after(Ending::Ended(None));
        }
        assert_eq!(pause_after(&mut backoff, &world, Err(error)), FIRST_PAUSE);
        assert_eq!(pause_after(&mut backoff, &world, Ok(())), FIRST_PAUSE);
    }

    #[test]
    fn a_step_clears_its_link_and_returns_the_pause_it_computed() {
        let world: Shared = Default::default();
        let writer: Writer = Default::default();
        let mut backoff = Backoff::default();
        for _ in 0..5 {
            backoff.after(Ending::Ended(None));
        }
        let closing = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        closing.set_nonblocking(true).unwrap();
        let open_port = closing.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if let Ok((stream, _)) = closing.accept() {
                    drop(stream);
                    return true;
                }
                thread::sleep(Duration::from_millis(10));
            }
            false
        });
        assert_eq!(
            step("bot", open_port, &writer, &world, &mut backoff),
            LONGEST_PAUSE
        );
        assert!(server.join().unwrap(), "the bot never reached the server");
        assert!(writer.lock().unwrap().is_none());
        let freed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_port = freed.local_addr().unwrap().port();
        drop(freed);
        assert_eq!(
            step("bot", closed_port, &writer, &world, &mut backoff),
            FIRST_PAUSE
        );
        assert!(writer.lock().unwrap().is_none());
    }

    #[test]
    fn a_server_that_never_accepts_is_waited_for_two_seconds() {
        let listener =
            socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();
        listener
            .bind(&std::net::SocketAddr::from(([127, 0, 0, 1], 0)).into())
            .unwrap();
        listener.listen(1).unwrap();
        let address = listener.local_addr().unwrap().as_socket().unwrap();
        let mut full = Vec::new();
        while full.len() < 64 {
            match TcpStream::connect_timeout(&address, Duration::from_millis(200)) {
                Ok(stream) => full.push(stream),
                Err(_) => break,
            }
        }
        assert!(full.len() < 64, "the listen queue never filled");
        let started = Instant::now();
        let error = session(
            "bot",
            address.port(),
            &Writer::default(),
            &Shared::default(),
        )
        .unwrap_err();
        let waited = started.elapsed();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::TimedOut,
            "the queue held {} connections: {error}",
            full.len()
        );
        assert!(
            (CONNECT_LIMIT..CONNECT_LIMIT + Duration::from_secs(1)).contains(&waited),
            "{waited:?} {error}"
        );
        assert!(error.to_string().contains("waited up to 2 s"), "{error}");
    }

    #[test]
    fn only_a_game_of_a_second_starts_the_pauses_over() {
        let mut backoff = Backoff::default();
        for _ in 0..5 {
            backoff.after(Ending::Ended(None));
        }
        let cut_on_arrival = Some(STEADY_PLAY - Duration::from_millis(1));
        assert_eq!(backoff.after(Ending::Ended(cut_on_arrival)), LONGEST_PAUSE);
        assert_eq!(backoff.after(Ending::Ended(Some(STEADY_PLAY))), FIRST_PAUSE);
        assert_eq!(
            backoff.after(Ending::Ended(Some(Duration::from_millis(10)))),
            FIRST_PAUSE * 2
        );
        assert_eq!(backoff.after(Ending::Ended(None)), FIRST_PAUSE * 4);
    }

    #[test]
    fn leaving_a_level_forgets_what_was_on_it() {
        let mut world = World {
            camera: 3,
            ..Default::default()
        };
        world.chunks.insert((0, 0));
        world.entities.insert(1, [0.0; 3]);
        world.forget_level();
        assert_eq!(
            (world.chunks.len(), world.camera, world.entities.len()),
            (0, 0, 0)
        );
    }

    #[test]
    fn every_packet_name_is_in_the_reference() {
        for (state, direction, name) in PACKETS {
            let found = voidmc_data::packet_id(VERSION, state, direction, name);
            assert!(found.is_some(), "{state} {direction} {name}");
        }
    }

    #[test]
    fn a_packet_outside_the_reference_has_no_name() {
        assert_eq!(clientbound(PLAY, i32::MAX), None);
    }

    fn connected() -> (TcpStream, TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        for side in [&client, &server] {
            side.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        }
        (client, server)
    }

    fn connected_writer() -> (Writer, TcpStream) {
        let (client, server) = connected();
        let link = Link {
            stream: client,
            threshold: None,
        };
        (Arc::new(Mutex::new(Some(link))), server)
    }

    fn receive(server: &mut TcpStream) -> std::io::Result<Vec<u8>> {
        frame::read(server, None)
    }

    fn handled(world: &Shared, name: &str, body: &[u8]) -> std::io::Result<()> {
        let (writer, _server) = connected_writer();
        handle(name, body, &writer, world, Instant::now())
    }

    fn chunk(x: i32, z: i32) -> Vec<u8> {
        let mut body = Vec::new();
        x.encode(&mut body);
        z.encode(&mut body);
        body
    }

    #[test]
    fn a_chunk_counts_once_and_an_unknown_unload_changes_nothing() {
        let world = Shared::default();
        handled(&world, PLAY_CHUNK, &chunk(1, 2)).unwrap();
        handled(&world, PLAY_CHUNK, &chunk(1, 2)).unwrap();
        handled(&world, PLAY_UNLOAD_CHUNK, &chunk(9, 9)).unwrap();
        assert_eq!(world.lock().unwrap().fields()["chunks"], 1);
        handled(&world, PLAY_UNLOAD_CHUNK, &chunk(2, 1)).unwrap();
        assert_eq!(world.lock().unwrap().fields()["chunks"], 0);
    }

    #[test]
    fn an_entity_follows_its_relative_moves_and_an_unseen_one_is_not_invented() {
        let world = Shared::default();
        world.lock().unwrap().entities.insert(7, [1.0, 2.0, 3.0]);
        let step = |id: i32| {
            let mut body = Vec::new();
            VarI32(id).encode(&mut body);
            for delta in [4096i16, -2048, 0] {
                delta.encode(&mut body);
            }
            true.encode(&mut body);
            body
        };
        handled(&world, PLAY_MOVE_ENTITY, &step(7)).unwrap();
        handled(&world, PLAY_MOVE_ENTITY, &step(8)).unwrap();
        let w = world.lock().unwrap();
        assert_eq!(w.entities[&7], [2.0, 1.5, 3.0]);
        assert!(!w.entities.contains_key(&8));
    }

    fn chat(nbt: Nbt, overlay: bool) -> Vec<u8> {
        let mut body = Vec::new();
        nbt.encode(&mut body);
        overlay.encode(&mut body);
        body
    }

    fn text(value: &str) -> Compound {
        Compound {
            tags: vec![("text".into(), Tag::String(value.into()))],
        }
    }

    #[test]
    fn chat_is_plain_text_and_an_action_bar_does_not_replace_it() {
        let world = Shared::default();
        let mut message = text("Bienvenue ");
        message.tags.push((
            "extra".into(),
            Tag::List(List::Compound(vec![text("sur la "), text("carte")])),
        ));
        let nbt = |compound| Nbt {
            name: "".into(),
            compound,
        };
        handled(&world, PLAY_SYSTEM_CHAT, &chat(nbt(message), false)).unwrap();
        handled(&world, PLAY_SYSTEM_CHAT, &chat(nbt(text("Tour 2")), true)).unwrap();
        let fields = world.lock().unwrap().fields();
        assert_eq!(fields["chat"], "Bienvenue sur la carte");
        assert_eq!(fields["action_bar"], "Tour 2");
        assert_eq!(fields["chat_count"], 1);
    }

    #[test]
    fn a_translated_chat_reads_as_the_game_would_show_it() {
        let language: Language = [
            (
                "multiplayer.player.joined".to_string(),
                "%s joined the game".to_string(),
            ),
            ("two".to_string(), "%2$s then %1$s, 100%%".to_string()),
        ]
        .into();
        let joined = Compound {
            tags: vec![
                (
                    "translate".into(),
                    Tag::String("multiplayer.player.joined".into()),
                ),
                ("with".into(), Tag::List(List::Compound(vec![text("Bot")]))),
                ("extra".into(), Tag::List(List::String(vec!["!".into()]))),
            ],
        };
        let mut shown = String::new();
        plain(&joined, &language, &mut shown);
        assert_eq!(shown, "Bot joined the game!");
        assert_eq!(
            format_translation("%2$s then %1$s, 100%%", &["a".into(), "b".into()]),
            "b then a, 100%"
        );
        let unknown = Compound {
            tags: vec![
                ("translate".into(), Tag::String("nope".into())),
                ("fallback".into(), Tag::String("Repli %s".into())),
                ("with".into(), Tag::List(List::String(vec!["x".into()]))),
            ],
        };
        let mut shown = String::new();
        plain(&unknown, &language, &mut shown);
        assert_eq!(shown, "Repli x");
    }

    #[test]
    fn a_chat_sent_as_a_bare_string_is_read_too() {
        let world = Shared::default();
        let mut body = vec![0x08, 0, 5];
        body.extend(b"Salut");
        body.push(0);
        handled(&world, PLAY_SYSTEM_CHAT, &body).unwrap();
        assert_eq!(world.lock().unwrap().fields()["chat"], "Salut");
    }

    fn packet(state: &str, name: &str, body: &[u8]) -> Vec<u8> {
        let id = voidmc_data::packet_id(VERSION, state, "clientbound", name).unwrap();
        let mut packet = Vec::new();
        VarI32(id).encode(&mut packet);
        packet.extend_from_slice(body);
        packet
    }

    fn sent_id(body: &[u8]) -> i32 {
        packet_id(&mut &body[..]).unwrap()
    }

    #[test]
    fn a_bot_follows_compression_from_login_to_the_end_of_configuration() {
        let (client, mut server) = connected();
        let writer: Writer = Arc::new(Mutex::new(Some(Link {
            stream: client.try_clone().unwrap(),
            threshold: None,
        })));
        let bot = thread::spawn(move || {
            let mut reader = Reader {
                stream: client,
                threshold: None,
            };
            login(&mut reader, &writer, "Bot", 25700)
        });
        let threshold = Some(256);
        frame::read(&mut server, None).unwrap();
        frame::read(&mut server, None).unwrap();
        let mut compression = Vec::new();
        VarI32(256).encode(&mut compression);
        let start = packet(LOGIN, LOGIN_COMPRESSION, &compression);
        server
            .write_all(&frame::encode(&start, None).unwrap())
            .unwrap();
        let done = packet(LOGIN, LOGIN_SUCCESS, &[]);
        server
            .write_all(&frame::encode(&done, threshold).unwrap())
            .unwrap();
        let acknowledged = frame::read(&mut server, threshold).unwrap();
        assert_eq!(
            sent_id(&acknowledged),
            serverbound(LOGIN, "minecraft:login_acknowledged")
        );
        let information = frame::read(&mut server, threshold).unwrap();
        assert_eq!(
            sent_id(&information),
            serverbound(CONFIGURATION, "minecraft:client_information")
        );
        let mut ping = Vec::new();
        42i32.encode(&mut ping);
        let ping = packet(CONFIGURATION, CONFIG_PING, &ping);
        server
            .write_all(&frame::encode(&ping, threshold).unwrap())
            .unwrap();
        let pong = frame::read(&mut server, threshold).unwrap();
        assert_eq!(pong[1..], 42i32.to_be_bytes());
        let finish = packet(CONFIGURATION, CONFIG_FINISH, &[]);
        server
            .write_all(&frame::encode(&finish, threshold).unwrap())
            .unwrap();
        frame::read(&mut server, threshold).unwrap();
        bot.join().unwrap().unwrap();
    }

    #[test]
    fn a_refused_login_keeps_its_reason() {
        let (client, mut server) = connected();
        let writer: Writer = Arc::new(Mutex::new(Some(Link {
            stream: client.try_clone().unwrap(),
            threshold: None,
        })));
        let bot = thread::spawn(move || {
            let mut reader = Reader {
                stream: client,
                threshold: None,
            };
            login(&mut reader, &writer, "Bot", 25700)
        });
        frame::read(&mut server, None).unwrap();
        frame::read(&mut server, None).unwrap();
        let mut reason = Vec::new();
        "\"Serveur plein\"".to_string().encode(&mut reason);
        let refusal = packet(LOGIN, LOGIN_DISCONNECT, &reason);
        server
            .write_all(&frame::encode(&refusal, None).unwrap())
            .unwrap();
        let error = bot.join().unwrap().unwrap_err();
        assert!(error.to_string().contains("Serveur plein"), "{error}");
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
            let mut expected = vec![serverbound(PLAY, SERVERBOUND_PACK_RESPONSE) as u8];
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
