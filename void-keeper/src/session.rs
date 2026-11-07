//! One client and whichever server is there for it: the whole logic of the proxy, without I/O.
//!
//! A session is fed what happens (`on_*`) with the current time, and answers with [`Action`]s.
//! Between two servers the client stays in play with its world on screen. When a new server
//! answers, a client in play goes through a real configuration: it is told
//! `start_configuration`, and what the server said so far reaches it as if it had just been
//! sent.
//!
//! A client lost in the middle of a configuration has no such way back: it adds up what it
//! receives and cannot start over. The new server must repeat what the client already holds,
//! which the session keeps from it; a server that says anything else ends the session.

use std::time::{Duration, Instant};

use bytes::Bytes;
use uuid::Uuid;
use voidmc_codec::{Decode, Encode, VarI32};
use voidmc_data::v26_1_2::packets::configuration::{
    clientbound as config_cb, serverbound as config_sb,
};
use voidmc_data::v26_1_2::packets::login::{clientbound as login_cb, serverbound as login_sb};
use voidmc_data::v26_1_2::packets::play::{clientbound as play_cb, serverbound as play_sb};
use voidmc_protocol::clientbound::{Disconnect, KeepAlive, ResourcePackPush};

use crate::frame::{Frame, MAX_PLAIN_FRAME_BYTES, text};
use crate::packs::PackBook;
use crate::script::{Script, needs_a_client};

const OWN_KEEP_ALIVE: i64 = 0x4B45_4550_0000_0000;
const OWN_KEEP_ALIVE_MASK: i64 = 0x7FFF_FFFF_0000_0000;
const LOGIN_PATIENCE: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// How long a client waits for a server that went away.
    pub return_timeout: Duration,
    pub retry_every: Duration,
    pub keep_alive_every: Duration,
    /// How long a new server has to offer its packs before the client lets go of the others.
    pub settle: Duration,
    /// How long a backend may stay silent before it counts as lost. Below the 30 seconds after
    /// which a client gives up by itself, above the interval of the keep-alives of a server:
    /// the engine counts it in ticks, 200 of them, which is 10 seconds only at 20 TPS.
    pub backend_silence: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            return_timeout: Duration::from_secs(120),
            retry_every: Duration::from_millis(100),
            keep_alive_every: Duration::from_secs(1),
            settle: Duration::from_secs(3),
            backend_silence: Duration::from_secs(20),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Joined { name: String },
    BackendLost,
    Resumed { outage: Duration },
    GaveUp { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    ToClient(Bytes),
    ToBackend(Bytes),
    /// From now on, the backend's frames are compressed above this threshold.
    Compress(i32),
    Connect,
    DropBackend,
    /// Sends what is pending to the client, then hangs up.
    Close,
    Event(Event),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Login,
    /// `login_finished` is on its way and the client has not acknowledged it.
    Joining,
    Configuration,
    /// The server's `finish_configuration` is on its way and the client has not answered it.
    LeavingConfiguration,
    Play,
    /// A `start_configuration` is on its way and the client has not acknowledged it. The
    /// acknowledgement goes to the live server while relaying, and is the proxy's otherwise.
    LeavingPlay,
}

/// The protocol a client decodes what it receives with.
enum Reads {
    Login,
    Configuration,
    Play,
}

#[derive(Debug)]
enum Stage {
    Relaying,
    Waiting {
        connecting: bool,
        next_attempt: Instant,
    },
    LoggingIn,
    /// The new server goes through what a client in configuration already received.
    Replaying {
        step: usize,
        question_open: bool,
    },
    Reconfiguring {
        held: Vec<Frame>,
    },
}

pub struct Session {
    settings: Settings,
    handshake: Bytes,
    hello: Option<Bytes>,
    client_information: Option<Bytes>,
    phase: Phase,
    stage: Stage,
    bundle_open: bool,
    away_shown: bool,
    packs: PackBook,
    script: Script,
    lost_at: Option<Instant>,
    deadline: Option<Instant>,
    next_keep_alive: Instant,
    keep_alive_count: i64,
    settle_at: Option<Instant>,
    backend_heard_at: Instant,
    closed: bool,
    actions: Vec<Action>,
}

impl Session {
    /// A client whose handshake announced a login.
    pub fn new(settings: Settings, now: Instant, handshake: Bytes) -> Self {
        Self {
            settings,
            handshake,
            hello: None,
            client_information: None,
            phase: Phase::Login,
            stage: Stage::Waiting {
                connecting: false,
                next_attempt: now,
            },
            bundle_open: false,
            away_shown: false,
            packs: PackBook::default(),
            script: Script::default(),
            lost_at: None,
            deadline: Some(now + settings.return_timeout.min(LOGIN_PATIENCE)),
            next_keep_alive: now,
            keep_alive_count: 0,
            settle_at: None,
            backend_heard_at: now,
            closed: false,
            actions: Vec::new(),
        }
    }

    /// What the session decided since the last call. Its buffer stays, for the next packet.
    pub fn actions(&mut self) -> std::vec::Drain<'_, Action> {
        self.actions.drain(..)
    }

    fn send_to_client(&mut self, frame: Bytes) {
        if self.closed {
            return;
        }
        if frame.len() > MAX_PLAIN_FRAME_BYTES {
            self.actions.push(Action::DropBackend);
            self.give_up(format!(
                "The server sent a packet of {} bytes: a client behind the keeper reads at most \
                 {MAX_PLAIN_FRAME_BYTES}.",
                frame.len()
            ));
            return;
        }
        self.actions.push(Action::ToClient(frame));
    }

    fn send_to_backend(&mut self, frame: Bytes) {
        self.actions.push(Action::ToBackend(frame));
    }

    pub fn on_tick(&mut self, now: Instant) {
        if self.closed {
            return;
        }
        if self.backend_is_silent(now) {
            tracing::info!("backend silent, counted as lost");
            self.actions.push(Action::DropBackend);
            self.on_backend_lost(now);
            return;
        }
        if matches!(self.stage, Stage::Relaying) {
            self.settle(now);
            return;
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.give_up("The server did not come back in time.".to_string());
            return;
        }
        if now >= self.next_keep_alive {
            self.next_keep_alive = now + self.settings.keep_alive_every;
            self.keep_client_alive(now);
        }
        if let Stage::Waiting {
            connecting,
            next_attempt,
        } = &mut self.stage
            && !*connecting
            && now >= *next_attempt
            && self.phase != Phase::Joining
        {
            *connecting = true;
            self.actions.push(Action::Connect);
        }
    }

    fn backend_is_silent(&self, now: Instant) -> bool {
        let watched = match self.stage {
            Stage::Waiting { .. } => false,
            Stage::Relaying => match self.phase {
                Phase::Play | Phase::LeavingPlay => true,
                Phase::Configuration | Phase::LeavingConfiguration => {
                    self.lost_at.is_some() && !self.packs.awaits_client()
                }
                Phase::Login | Phase::Joining => false,
            },
            _ => true,
        };
        watched && now.duration_since(self.backend_heard_at) >= self.settings.backend_silence
    }

    pub fn on_connect_failed(&mut self, now: Instant) {
        let retry = self.settings.retry_every;
        if let Stage::Waiting {
            connecting,
            next_attempt,
        } = &mut self.stage
        {
            *connecting = false;
            *next_attempt = now + retry;
        }
    }

    pub fn on_backend_connected(&mut self, now: Instant) {
        self.backend_heard_at = now;
        self.send_to_backend(self.handshake.clone());
        if let Some(hello) = self.hello.clone() {
            self.send_to_backend(hello);
        }
        if self.phase == Phase::Login {
            self.deadline = None;
            self.stage = Stage::Relaying;
            return;
        }
        self.stage = Stage::LoggingIn;
    }

    pub fn on_backend_lost(&mut self, now: Instant) {
        if self.closed {
            return;
        }
        if self.phase == Phase::Play {
            self.close_bundle();
        }
        if self.lost_at.is_none() {
            self.lost_at = Some(now);
            self.packs.outage();
            self.settle_at = None;
            self.next_keep_alive = now;
            self.actions.push(Action::Event(Event::BackendLost));
        }
        let patience = self.patience();
        self.deadline.get_or_insert(now + patience);
        self.wait_for_backend(now + self.settings.retry_every);
    }

    fn patience(&self) -> Duration {
        match self.phase {
            Phase::Login | Phase::Joining => self.settings.return_timeout.min(LOGIN_PATIENCE),
            _ => self.settings.return_timeout,
        }
    }

    fn wait_for_backend(&mut self, next_attempt: Instant) {
        self.stage = Stage::Waiting {
            connecting: false,
            next_attempt,
        };
    }

    /// The backend refused this attempt, which says nothing of the next one.
    fn retry(&mut self, now: Instant) {
        self.actions.push(Action::DropBackend);
        self.wait_for_backend(now + self.settings.retry_every);
    }

    /// A terminal packet switches a client as it arrives, not when the client answers it.
    fn client_reads(&self) -> Reads {
        match self.phase {
            Phase::Login => Reads::Login,
            Phase::Joining | Phase::Configuration | Phase::LeavingPlay => Reads::Configuration,
            Phase::LeavingConfiguration | Phase::Play => Reads::Play,
        }
    }

    fn keep_client_alive(&mut self, now: Instant) {
        self.keep_alive_count += 1;
        let keep_alive = KeepAlive {
            keep_alive_id: OWN_KEEP_ALIVE | self.keep_alive_count,
        };
        match self.phase {
            Phase::Play => {
                self.send_to_client(Frame::encode(play_cb::KEEP_ALIVE, |body| {
                    keep_alive.encode(body)
                }));
                let away = self
                    .lost_at
                    .map_or(0, |lost_at| now.duration_since(lost_at).as_secs());
                let message = format!("Server away for {away}s, hold on");
                self.send_to_client(action_bar(&message));
                self.away_shown = true;
            }
            Phase::Configuration => {
                self.send_to_client(Frame::encode(config_cb::KEEP_ALIVE, |body| {
                    keep_alive.encode(body)
                }));
            }
            Phase::Login | Phase::Joining | Phase::LeavingConfiguration | Phase::LeavingPlay => {}
        }
    }

    fn give_up(&mut self, reason: String) {
        let message = format!("[keeper] {reason}");
        let disconnect = Disconnect {
            reason: text(&message),
        };
        let goodbye = match self.client_reads() {
            Reads::Login => Frame::encode(login_cb::LOGIN_DISCONNECT, |body| {
                let json = serde_json::json!({ "text": message });
                json.to_string().encode(body);
            }),
            Reads::Configuration => {
                Frame::encode(config_cb::DISCONNECT, |body| disconnect.encode(body))
            }
            Reads::Play => {
                self.close_bundle();
                Frame::encode(play_cb::DISCONNECT, |body| disconnect.encode(body))
            }
        };
        self.send_to_client(goodbye);
        self.actions.push(Action::Event(Event::GaveUp { reason }));
        self.actions.push(Action::Close);
        self.closed = true;
    }

    fn settle(&mut self, now: Instant) {
        if self.phase != Phase::Play || self.settle_at.is_none_or(|at| now < at) {
            return;
        }
        self.settle_at = None;
        for pack in self.packs.take_forsaken() {
            self.send_to_client(Frame::encode(play_cb::RESOURCE_PACK_POP, |body| {
                Some(pack).encode(body);
            }));
        }
    }

    pub fn on_client(&mut self, frame: Bytes, now: Instant) {
        let Some((id, body)) = Frame::id_and_body(&frame) else {
            return;
        };
        let forward = match self.phase {
            Phase::Login | Phase::Joining => self.client_in_login(id, body, &frame),
            Phase::Configuration | Phase::LeavingConfiguration => {
                self.client_in_configuration(id, body, &frame, now)
            }
            Phase::Play => self.client_in_play(id, body),
            Phase::LeavingPlay if matches!(self.stage, Stage::Relaying) => {
                self.client_in_play(id, body)
            }
            Phase::LeavingPlay => {
                if id == play_sb::CONFIGURATION_ACKNOWLEDGED {
                    self.client_acknowledged();
                }
                false
            }
        };
        if forward && matches!(self.stage, Stage::Relaying) {
            self.send_to_backend(frame);
        }
    }

    fn client_in_login(&mut self, id: i32, mut body: &[u8], frame: &Bytes) -> bool {
        match id {
            login_sb::HELLO => {
                self.hello = Some(frame.clone());
                if let Ok(name) = String::decode(&mut body) {
                    self.actions.push(Action::Event(Event::Joined { name }));
                }
            }
            login_sb::LOGIN_ACKNOWLEDGED => {
                self.phase = Phase::Configuration;
                if let (Some(lost_at), Some(_)) = (self.lost_at, self.deadline) {
                    self.deadline = Some(lost_at + self.patience());
                }
            }
            _ => {}
        }
        true
    }

    fn client_in_configuration(
        &mut self,
        id: i32,
        body: &[u8],
        frame: &Bytes,
        now: Instant,
    ) -> bool {
        match id {
            config_sb::CLIENT_INFORMATION => {
                self.client_information = Some(Bytes::copy_from_slice(body));
            }
            config_sb::SELECT_KNOWN_PACKS => {
                self.script.set_known_packs_reply(frame.clone());
                if let Stage::Replaying { question_open, .. } = &mut self.stage
                    && std::mem::take(question_open)
                {
                    self.send_to_backend(frame.clone());
                }
            }
            config_sb::RESOURCE_PACK => {
                self.note_pack_status(body);
                if matches!(self.stage, Stage::Replaying { .. }) {
                    self.send_to_backend(frame.clone());
                }
            }
            config_sb::KEEP_ALIVE if is_own_keep_alive(body) => return false,
            config_sb::FINISH_CONFIGURATION => {
                self.phase = Phase::Play;
                match self.stage {
                    Stage::Relaying => self.finish_reconfiguration(now),
                    Stage::Replaying { .. } => {
                        self.actions.push(Action::DropBackend);
                        self.wait_for_backend(now);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        true
    }

    fn client_in_play(&mut self, id: i32, body: &[u8]) -> bool {
        match id {
            play_sb::KEEP_ALIVE if is_own_keep_alive(body) => return false,
            play_sb::CLIENT_INFORMATION => {
                self.client_information = Some(Bytes::copy_from_slice(body));
            }
            play_sb::RESOURCE_PACK => self.note_pack_status(body),
            play_sb::CONFIGURATION_ACKNOWLEDGED => {
                self.phase = Phase::Configuration;
                self.script = Script::default();
            }
            _ => {}
        }
        true
    }

    fn note_pack_status(&mut self, mut body: &[u8]) {
        let Ok(id) = Uuid::decode(&mut body) else {
            return;
        };
        if let Ok(status) = VarI32::decode(&mut body) {
            self.packs.answered(id, status.0);
        }
    }

    fn start_configuration(&mut self) {
        tracing::info!("client goes through a reconfiguration");
        self.send_to_client(Frame::encode(play_cb::START_CONFIGURATION, |_| {}));
        self.phase = Phase::LeavingPlay;
    }

    fn client_acknowledged(&mut self) {
        self.phase = Phase::Configuration;
        self.script = Script::default();
        match std::mem::replace(&mut self.stage, Stage::Relaying) {
            Stage::Reconfiguring { held } => {
                for packet in held {
                    self.relay_from_backend(packet);
                }
            }
            other => self.stage = other,
        }
    }

    fn finish_reconfiguration(&mut self, now: Instant) {
        let Some(lost_at) = self.lost_at.take() else {
            return;
        };
        let outage = now.duration_since(lost_at);
        self.deadline = None;
        self.settle_at = Some(now + self.settings.settle);
        tracing::info!(?outage, digest = self.script.digest(), "client resumed");
        self.actions.push(Action::Event(Event::Resumed { outage }));
    }

    fn close_bundle(&mut self) {
        if std::mem::take(&mut self.bundle_open) {
            self.send_to_client(Frame::encode(play_cb::BUNDLE_DELIMITER, |_| {}));
        }
    }

    pub fn on_backend(&mut self, frame: Bytes, now: Instant) {
        let Some(packet) = Frame::read(frame) else {
            tracing::warn!("backend frame without an id");
            return;
        };
        self.backend_heard_at = now;
        match self.stage {
            Stage::Relaying => self.relay_from_backend(packet),
            Stage::LoggingIn => self.log_in(&packet, now),
            Stage::Replaying { .. } => self.replay(packet, now),
            Stage::Reconfiguring { .. } => self.hold_for_client(packet, now),
            Stage::Waiting { .. } => {}
        }
    }

    fn relay_from_backend(&mut self, packet: Frame) {
        let (id, body) = (packet.id(), packet.body());
        let sent_away = is_sent_away(self.client_reads(), id);
        match self.phase {
            Phase::Login => match id {
                login_cb::LOGIN_COMPRESSION => {
                    let mut body = body;
                    if let Ok(threshold) = VarI32::decode(&mut body) {
                        self.actions.push(Action::Compress(threshold.0));
                    }
                    return;
                }
                login_cb::HELLO => {
                    self.refuse_online_mode();
                    return;
                }
                login_cb::LOGIN_FINISHED => self.phase = Phase::Joining,
                _ => {}
            },
            Phase::Joining => return,
            Phase::Configuration | Phase::LeavingConfiguration => match id {
                config_cb::RESOURCE_PACK_PUSH => {
                    if self.answer_known_pack(body, config_sb::RESOURCE_PACK) {
                        return;
                    }
                }
                config_cb::RESOURCE_PACK_POP => self.note_pack_pop(body),
                config_cb::KEEP_ALIVE | config_cb::PING => {}
                config_cb::FINISH_CONFIGURATION => {
                    self.phase = Phase::LeavingConfiguration;
                    self.script.record(packet.bytes());
                }
                _ => self.script.record(packet.bytes()),
            },
            Phase::Play | Phase::LeavingPlay => match id {
                play_cb::RESOURCE_PACK_PUSH => {
                    if self.answer_known_pack(body, play_sb::RESOURCE_PACK) {
                        return;
                    }
                }
                play_cb::RESOURCE_PACK_POP => self.note_pack_pop(body),
                play_cb::START_CONFIGURATION => self.phase = Phase::LeavingPlay,
                play_cb::BUNDLE_DELIMITER => self.bundle_open = !self.bundle_open,
                play_cb::LOGIN if std::mem::take(&mut self.away_shown) => {
                    self.send_to_client(packet.into_bytes());
                    self.send_to_client(action_bar(""));
                    return;
                }
                _ => {}
            },
        }
        self.send_to_client(packet.into_bytes());
        if sent_away && !self.closed {
            self.actions.push(Action::DropBackend);
            self.actions.push(Action::Close);
            self.closed = true;
        }
    }

    fn refuse_online_mode(&mut self) {
        self.actions.push(Action::DropBackend);
        self.give_up(
            "The server is in online mode: the keeper cannot sit in an encrypted link.".to_string(),
        );
    }

    fn note_pack_pop(&mut self, mut body: &[u8]) {
        if let Ok(id) = Option::<Uuid>::decode(&mut body) {
            self.packs.popped(id);
        }
    }

    /// Answers for the client when it holds this pack already. A client told again about a
    /// pack it holds reloads all its resources, for nothing.
    fn answer_known_pack(&mut self, mut body: &[u8], response_id: i32) -> bool {
        let Ok(push) = ResourcePackPush::decode(&mut body) else {
            return false;
        };
        let Some(statuses) = self.packs.offer(push.id, &push.hash) else {
            return false;
        };
        for status in statuses {
            self.send_to_backend(Frame::encode(response_id, |body| {
                push.id.encode(body);
                VarI32(status).encode(body);
            }));
        }
        true
    }

    fn log_in(&mut self, packet: &Frame, now: Instant) {
        let mut body = packet.body();
        match packet.id() {
            login_cb::LOGIN_COMPRESSION => {
                if let Ok(threshold) = VarI32::decode(&mut body) {
                    self.actions.push(Action::Compress(threshold.0));
                }
            }
            login_cb::HELLO => self.refuse_online_mode(),
            login_cb::LOGIN_DISCONNECT => {
                tracing::info!("server refused the login, trying again");
                self.retry(now);
            }
            login_cb::CUSTOM_QUERY => {
                if let Ok(transaction) = VarI32::decode(&mut body) {
                    self.send_to_backend(Frame::encode(login_sb::CUSTOM_QUERY_ANSWER, |answer| {
                        transaction.encode(answer);
                        false.encode(answer);
                    }));
                }
            }
            login_cb::COOKIE_REQUEST => {
                if let Ok(key) = String::decode(&mut body) {
                    self.send_to_backend(Frame::encode(login_sb::COOKIE_RESPONSE, |answer| {
                        key.encode(answer);
                        false.encode(answer);
                    }));
                }
            }
            login_cb::LOGIN_FINISHED => self.logged_in(),
            _ => {}
        }
    }

    /// A client in play is only taken out of its world once the new server has logged it in: a
    /// frozen server accepts connections too, and never gets that far.
    fn logged_in(&mut self) {
        if self.phase == Phase::Play {
            self.start_configuration();
        }
        self.send_to_backend(Frame::encode(login_sb::LOGIN_ACKNOWLEDGED, |_| {}));
        if let Some(information) = &self.client_information {
            let information = information.clone();
            self.send_to_backend(Frame::encode(config_sb::CLIENT_INFORMATION, |body| {
                body.extend_from_slice(&information);
            }));
        }
        self.stage = match self.phase {
            Phase::Play | Phase::LeavingPlay => Stage::Reconfiguring { held: Vec::new() },
            Phase::Configuration | Phase::LeavingConfiguration
                if !self.script.is_finished_at(0) =>
            {
                Stage::Replaying {
                    step: 0,
                    question_open: false,
                }
            }
            Phase::Login | Phase::Joining | Phase::Configuration | Phase::LeavingConfiguration => {
                self.deadline = None;
                Stage::Relaying
            }
        };
    }

    /// Keeps from the client what it holds already: it would add it up, and fail on the double
    /// when the configuration ends.
    fn replay(&mut self, packet: Frame, now: Instant) {
        if self.answer_for_client(&packet, now) {
            return;
        }
        let Stage::Replaying {
            step,
            mut question_open,
        } = self.stage
        else {
            return;
        };
        let id = packet.id();
        if matches!(
            id,
            config_cb::RESOURCE_PACK_PUSH | config_cb::RESOURCE_PACK_POP
        ) {
            if self.phase == Phase::LeavingConfiguration {
                self.actions.push(Action::DropBackend);
                self.give_up(
                    "The server changes its packs for a client that already left its \
                     configuration. Connect again."
                        .to_string(),
                );
                return;
            }
            self.relay_from_backend(packet);
            return;
        }
        if let Some(reason) = needs_a_client(id) {
            self.actions.push(Action::DropBackend);
            self.give_up(format!(
                "The server came back during the configuration and {reason}: only the client \
                 can answer that. Connect again."
            ));
            return;
        }
        if !self.script.follows(step, packet.bytes()) {
            self.actions.push(Action::DropBackend);
            self.give_up(format!(
                "The server changed during the configuration, at packet {step} (id {id:#x}), and \
                 a client cannot take back what it received. Connect again."
            ));
            return;
        }
        if id == config_cb::SELECT_KNOWN_PACKS {
            match self.script.known_packs_reply().cloned() {
                Some(reply) => self.send_to_backend(reply),
                None => question_open = true,
            }
        }
        let step = step + 1;
        self.stage = if self.script.is_finished_at(step) {
            self.deadline = None;
            Stage::Relaying
        } else {
            Stage::Replaying {
                step,
                question_open,
            }
        };
    }

    /// Answers what a backend asks during a configuration no client is listening to.
    /// `true` when that took care of `frame`.
    fn answer_for_client(&mut self, packet: &Frame, now: Instant) -> bool {
        let body = packet.body();
        match packet.id() {
            config_cb::KEEP_ALIVE => {
                self.send_to_backend(Frame::encode(config_sb::KEEP_ALIVE, |out| {
                    out.extend_from_slice(body);
                }));
                true
            }
            config_cb::PING => {
                self.send_to_backend(Frame::encode(config_sb::PONG, |out| {
                    out.extend_from_slice(body)
                }));
                true
            }
            config_cb::DISCONNECT => {
                tracing::info!("server refused the configuration, trying again");
                self.retry(now);
                true
            }
            _ => false,
        }
    }

    fn hold_for_client(&mut self, packet: Frame, now: Instant) {
        if self.answer_for_client(&packet, now) {
            return;
        }
        if packet.id() == config_cb::RESOURCE_PACK_PUSH
            && self.answer_known_pack(packet.body(), config_sb::RESOURCE_PACK)
        {
            return;
        }
        if let Stage::Reconfiguring { held } = &mut self.stage {
            held.push(packet);
        }
    }
}

/// Whether a live server sends the client away with this packet.
fn is_sent_away(reads: Reads, id: i32) -> bool {
    match reads {
        Reads::Login => id == login_cb::LOGIN_DISCONNECT,
        Reads::Configuration => id == config_cb::DISCONNECT,
        Reads::Play => id == play_cb::DISCONNECT,
    }
}

fn action_bar(message: &str) -> Bytes {
    Frame::encode(play_cb::SET_ACTION_BAR_TEXT, |body| {
        text(message).encode(body)
    })
}

fn is_own_keep_alive(mut body: &[u8]) -> bool {
    i64::decode(&mut body).is_ok_and(|id| id & OWN_KEEP_ALIVE_MASK == OWN_KEEP_ALIVE)
}

#[cfg(test)]
mod tests;
