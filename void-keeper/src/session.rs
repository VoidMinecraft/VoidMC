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

use crate::packs::PackBook;
use crate::script::{Script, needs_a_client};
use crate::wire::{MAX_PLAIN_FRAME_BYTES, frame, split, text};

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
    /// The server's `start_configuration` is on its way and the client has not acknowledged it.
    LeavingPlay,
    /// The proxy's own `start_configuration` is on its way and the client has not acknowledged it.
    AwaitingAck,
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
        held: Vec<Bytes>,
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
                Phase::Login | Phase::Joining | Phase::AwaitingAck => false,
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
        if self.phase == Phase::Play {
            self.start_configuration();
        }
        self.stage = Stage::LoggingIn;
    }

    pub fn on_backend_lost(&mut self, now: Instant) {
        if self.closed {
            return;
        }
        if self.phase == Phase::LeavingPlay {
            self.phase = Phase::AwaitingAck;
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
            Phase::Joining | Phase::Configuration | Phase::LeavingPlay | Phase::AwaitingAck => {
                Reads::Configuration
            }
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
                self.send_to_client(frame(play_cb::KEEP_ALIVE, |body| keep_alive.encode(body)));
                let away = self
                    .lost_at
                    .map_or(0, |lost_at| now.duration_since(lost_at).as_secs());
                let message = format!("Server away for {away}s, hold on");
                self.send_to_client(action_bar(&message));
                self.away_shown = true;
            }
            Phase::Configuration => {
                self.send_to_client(frame(config_cb::KEEP_ALIVE, |body| keep_alive.encode(body)));
            }
            Phase::Login
            | Phase::Joining
            | Phase::LeavingConfiguration
            | Phase::LeavingPlay
            | Phase::AwaitingAck => {}
        }
    }

    fn give_up(&mut self, reason: String) {
        let message = format!("[keeper] {reason}");
        let disconnect = Disconnect {
            reason: text(&message),
        };
        let goodbye = match self.client_reads() {
            Reads::Login => frame(login_cb::LOGIN_DISCONNECT, |body| {
                let json = serde_json::json!({ "text": message });
                json.to_string().encode(body);
            }),
            Reads::Configuration => frame(config_cb::DISCONNECT, |body| disconnect.encode(body)),
            Reads::Play => {
                self.close_bundle();
                frame(play_cb::DISCONNECT, |body| disconnect.encode(body))
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
            self.send_to_client(frame(play_cb::RESOURCE_PACK_POP, |body| {
                Some(pack).encode(body);
            }));
        }
    }

    pub fn on_client(&mut self, frame: Bytes, now: Instant) {
        let Some((id, body)) = split(&frame) else {
            return;
        };
        let forward = match self.phase {
            Phase::Login | Phase::Joining => self.client_in_login(id, body, &frame),
            Phase::Configuration | Phase::LeavingConfiguration => {
                self.client_in_configuration(id, body, &frame, now)
            }
            Phase::Play | Phase::LeavingPlay => self.client_in_play(id, body),
            Phase::AwaitingAck => {
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
        self.send_to_client(frame(play_cb::START_CONFIGURATION, |_| {}));
        self.phase = Phase::AwaitingAck;
    }

    fn client_acknowledged(&mut self) {
        self.phase = Phase::Configuration;
        self.script = Script::default();
        match std::mem::replace(&mut self.stage, Stage::Relaying) {
            Stage::Reconfiguring { held } => {
                for frame in held {
                    self.relay_from_backend(frame);
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
            self.send_to_client(frame(play_cb::BUNDLE_DELIMITER, |_| {}));
        }
    }

    pub fn on_backend(&mut self, frame: Bytes, now: Instant) {
        if split(&frame).is_none() {
            tracing::warn!("backend frame without an id");
            return;
        }
        self.backend_heard_at = now;
        match self.stage {
            Stage::Relaying => self.relay_from_backend(frame),
            Stage::LoggingIn => self.log_in(frame, now),
            Stage::Replaying { .. } => self.replay(frame, now),
            Stage::Reconfiguring { .. } => self.hold_for_client(frame, now),
            Stage::Waiting { .. } => {}
        }
    }

    fn relay_from_backend(&mut self, frame: Bytes) {
        let Some((id, body)) = split(&frame) else {
            return;
        };
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
            Phase::Joining | Phase::AwaitingAck => return,
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
                    self.script.record(&frame);
                }
                _ => self.script.record(&frame),
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
                    self.send_to_client(frame);
                    self.send_to_client(action_bar(""));
                    return;
                }
                _ => {}
            },
        }
        self.send_to_client(frame);
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
            self.send_to_backend(frame(response_id, |body| {
                push.id.encode(body);
                VarI32(status).encode(body);
            }));
        }
        true
    }

    fn log_in(&mut self, frame_in: Bytes, now: Instant) {
        let Some((id, mut body)) = split(&frame_in) else {
            return;
        };
        match id {
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
                    self.send_to_backend(frame(login_sb::CUSTOM_QUERY_ANSWER, |answer| {
                        transaction.encode(answer);
                        false.encode(answer);
                    }));
                }
            }
            login_cb::COOKIE_REQUEST => {
                if let Ok(key) = String::decode(&mut body) {
                    self.send_to_backend(frame(login_sb::COOKIE_RESPONSE, |answer| {
                        key.encode(answer);
                        false.encode(answer);
                    }));
                }
            }
            login_cb::LOGIN_FINISHED => self.logged_in(),
            _ => {}
        }
    }

    fn logged_in(&mut self) {
        self.send_to_backend(frame(login_sb::LOGIN_ACKNOWLEDGED, |_| {}));
        if let Some(information) = &self.client_information {
            let information = information.clone();
            self.send_to_backend(frame(config_sb::CLIENT_INFORMATION, |body| {
                body.extend_from_slice(&information);
            }));
        }
        self.stage = match self.phase {
            Phase::Play | Phase::AwaitingAck | Phase::LeavingPlay => {
                Stage::Reconfiguring { held: Vec::new() }
            }
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
    fn replay(&mut self, frame_in: Bytes, now: Instant) {
        if self.answer_for_client(&frame_in, now) {
            return;
        }
        let Stage::Replaying {
            step,
            mut question_open,
        } = self.stage
        else {
            return;
        };
        let id = split(&frame_in).map_or(-1, |(id, _)| id);
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
            self.relay_from_backend(frame_in);
            return;
        }
        if let Some(reason) = needs_a_client(&frame_in) {
            self.actions.push(Action::DropBackend);
            self.give_up(format!(
                "The server came back during the configuration and {reason}: only the client \
                 can answer that. Connect again."
            ));
            return;
        }
        if !self.script.follows(step, &frame_in) {
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
    fn answer_for_client(&mut self, frame_in: &Bytes, now: Instant) -> bool {
        let Some((id, body)) = split(frame_in) else {
            return true;
        };
        match id {
            config_cb::KEEP_ALIVE => {
                self.send_to_backend(frame(config_sb::KEEP_ALIVE, |out| {
                    out.extend_from_slice(body);
                }));
                true
            }
            config_cb::PING => {
                self.send_to_backend(frame(config_sb::PONG, |out| out.extend_from_slice(body)));
                true
            }
            config_cb::DISCONNECT => {
                tracing::info!("server refused the configuration, trying again");
                self.retry(now);
                true
            }
            config_cb::RESOURCE_PACK_PUSH => self.answer_known_pack(body, config_sb::RESOURCE_PACK),
            _ => false,
        }
    }

    fn hold_for_client(&mut self, frame: Bytes, now: Instant) {
        if self.answer_for_client(&frame, now) {
            return;
        }
        if let Stage::Reconfiguring { held } = &mut self.stage {
            held.push(frame);
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
    frame(play_cb::SET_ACTION_BAR_TEXT, |body| {
        text(message).encode(body)
    })
}

fn is_own_keep_alive(mut body: &[u8]) -> bool {
    i64::decode(&mut body).is_ok_and(|id| id & OWN_KEEP_ALIVE_MASK == OWN_KEEP_ALIVE)
}

#[cfg(test)]
mod tests {
    use voidmc_protocol::clientbound::PlayPacket;

    use super::*;
    use crate::wire::tagged;

    fn settings() -> Settings {
        Settings {
            return_timeout: Duration::from_secs(10),
            retry_every: Duration::from_millis(100),
            keep_alive_every: Duration::from_secs(1),
            settle: Duration::from_secs(3),
            backend_silence: Duration::from_secs(5),
        }
    }

    fn login_finished() -> Bytes {
        frame(login_cb::LOGIN_FINISHED, |body| {
            Uuid::from_u128(77).encode(body);
        })
    }

    fn login() -> Bytes {
        frame(play_cb::LOGIN, |body| body.push(7))
    }

    fn empty(id: i32) -> Bytes {
        frame(id, |_| {})
    }

    fn hello() -> Bytes {
        frame(login_sb::HELLO, |body| {
            "Bot".to_string().encode(body);
            Uuid::from_u128(1).encode(body);
        })
    }

    fn known_packs_question() -> Bytes {
        frame(config_cb::SELECT_KNOWN_PACKS, |body| body.push(0))
    }

    fn known_packs_reply() -> Bytes {
        frame(config_sb::SELECT_KNOWN_PACKS, |body| body.push(0))
    }

    fn registry(tag: u8) -> Bytes {
        frame(config_cb::REGISTRY_DATA, |body| body.push(tag))
    }

    fn information() -> Bytes {
        frame(config_sb::CLIENT_INFORMATION, |body| body.push(42))
    }

    fn push(id: Uuid, hash: &str) -> Bytes {
        tagged(&PlayPacket::ResourcePackPush(ResourcePackPush {
            id,
            url: "http://x".to_string(),
            hash: hash.to_string(),
            required: false,
            prompt: None,
        }))
    }

    fn config_push(id: Uuid) -> Bytes {
        let pushed = push(id, "abc");
        let (_, body) = split(&pushed).unwrap();
        frame(config_cb::RESOURCE_PACK_PUSH, |out| {
            out.extend_from_slice(body);
        })
    }

    fn pack_answer(id: Uuid, status: i32) -> Bytes {
        frame(play_sb::RESOURCE_PACK, |body| {
            id.encode(body);
            VarI32(status).encode(body);
        })
    }

    fn config_pack_answer(id: Uuid, status: i32) -> Bytes {
        frame(config_sb::RESOURCE_PACK, |body| {
            id.encode(body);
            VarI32(status).encode(body);
        })
    }

    struct Rig {
        session: Session,
        now: Instant,
        client: Vec<Bytes>,
        backend: Vec<Bytes>,
        rest: Vec<Action>,
    }

    impl Rig {
        fn fresh() -> Self {
            Self::with(settings())
        }

        fn with(settings: Settings) -> Self {
            let now = Instant::now();
            Self {
                session: Session::new(settings, now, empty(0)),
                now,
                client: Vec::new(),
                backend: Vec::new(),
                rest: Vec::new(),
            }
        }

        fn in_play() -> Self {
            let mut rig = Self::fresh();
            rig.join();
            rig
        }

        fn drain(&mut self) {
            for action in self.session.actions() {
                match action {
                    Action::ToClient(frame) => self.client.push(frame),
                    Action::ToBackend(frame) => self.backend.push(frame),
                    other => self.rest.push(other),
                }
            }
        }

        fn clear(&mut self) {
            self.drain();
            self.client.clear();
            self.backend.clear();
            self.rest.clear();
        }

        fn tick(&mut self, by: Duration) {
            self.now += by;
            self.session.on_tick(self.now);
            self.drain();
        }

        fn receive_from_client(&mut self, frame: Bytes) {
            self.session.on_client(frame, self.now);
            self.drain();
        }

        fn receive_from_backend(&mut self, frame: Bytes) {
            self.session.on_backend(frame, self.now);
            self.drain();
        }

        fn lose_backend(&mut self) {
            self.session.on_backend_lost(self.now);
            self.drain();
        }

        fn connected(&mut self) {
            self.session.on_backend_connected(self.now);
            self.drain();
        }

        fn join(&mut self) {
            self.tick(Duration::ZERO);
            self.connected();
            self.receive_from_client(hello());
            self.receive_from_backend(login_finished());
            self.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
            self.receive_from_client(information());
            self.receive_from_backend(known_packs_question());
            self.receive_from_client(known_packs_reply());
            self.receive_from_backend(registry(1));
            self.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
            self.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
            self.receive_from_backend(login());
            self.receive_from_client(empty(play_sb::PLAYER_LOADED));
            self.clear();
        }

        fn in_configuration() -> Self {
            let mut rig = Self::fresh();
            rig.tick(Duration::ZERO);
            rig.connected();
            rig.receive_from_client(hello());
            rig.receive_from_backend(login_finished());
            rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
            rig.receive_from_client(information());
            rig.receive_from_backend(known_packs_question());
            rig.receive_from_client(known_packs_reply());
            rig.receive_from_backend(registry(1));
            rig.clear();
            rig
        }

        fn log_back_in(&mut self) {
            self.tick(settings().retry_every);
            self.connected();
            self.receive_from_backend(login_finished());
        }

        /// A client in play taken through the configuration of a new server, up to its `login`.
        fn come_back(&mut self) {
            self.log_back_in();
            self.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
            self.receive_from_backend(known_packs_question());
            self.receive_from_client(known_packs_reply());
            self.receive_from_backend(registry(1));
            self.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
            self.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
            self.receive_from_backend(login());
        }

        fn events(&self) -> Vec<&Event> {
            self.rest
                .iter()
                .filter_map(|action| match action {
                    Action::Event(event) => Some(event),
                    _ => None,
                })
                .collect()
        }

        fn resumed(&self) -> usize {
            self.events()
                .into_iter()
                .filter(|event| matches!(event, Event::Resumed { .. }))
                .count()
        }

        fn sent_to_client(&self, id: i32) -> usize {
            self.client
                .iter()
                .filter(|frame| split(frame).unwrap().0 == id)
                .count()
        }

        fn sent_to_backend(&self, id: i32) -> usize {
            self.backend
                .iter()
                .filter(|frame| split(frame).unwrap().0 == id)
                .count()
        }
    }

    #[test]
    fn a_first_join_passes_through_untouched() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        assert_eq!(rig.rest, vec![Action::Connect]);
        rig.connected();
        rig.receive_from_client(hello());
        assert_eq!(rig.backend, vec![empty(0), hello()]);
        rig.receive_from_backend(known_packs_question());
        assert_eq!(rig.client, vec![known_packs_question()]);
    }

    #[test]
    fn a_client_is_kept_alive_while_the_server_is_away() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        assert_eq!(rig.events(), vec![&Event::BackendLost]);
        rig.tick(Duration::ZERO);
        assert_eq!(rig.sent_to_client(play_cb::KEEP_ALIVE), 1);
        assert_eq!(rig.sent_to_client(play_cb::SET_ACTION_BAR_TEXT), 1);
        rig.tick(Duration::from_millis(500));
        assert_eq!(rig.sent_to_client(play_cb::KEEP_ALIVE), 1);
        rig.tick(Duration::from_millis(600));
        assert_eq!(rig.sent_to_client(play_cb::KEEP_ALIVE), 2);
        assert!(rig.rest.contains(&Action::Connect));
    }

    #[test]
    fn nothing_from_the_client_reaches_a_server_that_is_not_there() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.backend.clear();
        rig.receive_from_client(empty(play_sb::CHAT_COMMAND));
        assert!(rig.backend.is_empty());
    }

    #[test]
    fn the_answer_to_our_keep_alive_never_reaches_the_new_server() {
        let mut rig = Rig::in_play();
        let answer = frame(play_sb::KEEP_ALIVE, |body| {
            (OWN_KEEP_ALIVE | 3).encode(body);
        });
        rig.receive_from_client(answer);
        assert!(rig.backend.is_empty());
        let theirs = frame(play_sb::KEEP_ALIVE, |body| {
            1_700_000_000_000i64.encode(body)
        });
        rig.receive_from_client(theirs.clone());
        assert_eq!(rig.backend, vec![theirs]);
    }

    #[test]
    fn connecting_is_retried_at_its_own_pace() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(Duration::from_millis(100));
        rig.tick(Duration::from_millis(100));
        assert_eq!(
            rig.rest.iter().filter(|a| **a == Action::Connect).count(),
            1
        );
        rig.session.on_connect_failed(rig.now);
        rig.tick(Duration::from_millis(50));
        rig.tick(Duration::from_millis(50));
        assert_eq!(
            rig.rest.iter().filter(|a| **a == Action::Connect).count(),
            2
        );
    }

    #[test]
    fn a_new_server_takes_the_client_through_a_configuration() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(settings().retry_every);
        rig.clear();
        rig.connected();
        assert_eq!(rig.client, vec![empty(play_cb::START_CONFIGURATION)]);
        rig.receive_from_backend(login_finished());
        assert!(rig.backend.contains(&empty(login_sb::LOGIN_ACKNOWLEDGED)));
        assert!(rig.backend.contains(&information()));
        rig.clear();

        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(registry(2));
        assert!(rig.client.is_empty());
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        assert!(rig.backend.is_empty());
        assert_eq!(rig.client, vec![known_packs_question(), registry(2)]);
        rig.receive_from_client(known_packs_reply());
        assert_eq!(rig.backend, vec![known_packs_reply()]);

        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        assert_eq!(rig.resumed(), 0);
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        assert_eq!(rig.resumed(), 1);
        rig.receive_from_backend(login());
        rig.receive_from_client(empty(play_sb::PLAYER_LOADED));
        assert_eq!(rig.sent_to_backend(play_sb::PLAYER_LOADED), 1);
        let commands = frame(play_cb::COMMANDS, |_| {});
        rig.receive_from_backend(commands.clone());
        assert_eq!(rig.client.last(), Some(&commands));
    }

    #[test]
    fn keep_alives_of_a_configuration_are_answered_and_do_not_wait_for_the_client() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.receive_from_backend(frame(config_cb::KEEP_ALIVE, |body| 9i64.encode(body)));
        rig.receive_from_backend(frame(config_cb::PING, |body| 4i32.encode(body)));
        assert_eq!(rig.sent_to_backend(config_sb::KEEP_ALIVE), 1);
        assert_eq!(rig.sent_to_backend(config_sb::PONG), 1);
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        assert!(rig.client.is_empty());
    }

    #[test]
    fn every_return_goes_through_a_configuration() {
        let mut rig = Rig::in_play();
        for _ in 0..3 {
            rig.lose_backend();
            rig.come_back();
            assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 1);
            assert_eq!(rig.resumed(), 1);
            rig.receive_from_client(empty(play_sb::PLAYER_LOADED));
            rig.clear();
        }
    }

    #[test]
    fn frames_held_for_a_slow_client_arrive_once_it_acknowledges_after_the_login() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.log_back_in();
        rig.receive_from_backend(known_packs_question());
        assert_eq!(rig.sent_to_client(config_cb::SELECT_KNOWN_PACKS), 0);
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        assert_eq!(rig.sent_to_client(config_cb::SELECT_KNOWN_PACKS), 1);
        rig.receive_from_client(known_packs_reply());
        assert_eq!(rig.sent_to_backend(config_sb::SELECT_KNOWN_PACKS), 1);
    }

    #[test]
    fn a_client_that_acknowledges_before_the_login_is_relayed_live() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(settings().retry_every);
        rig.connected();
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        rig.receive_from_backend(login_finished());
        rig.clear();
        rig.receive_from_backend(known_packs_question());
        assert_eq!(rig.client, vec![known_packs_question()]);
        rig.receive_from_client(known_packs_reply());
        assert_eq!(rig.backend, vec![known_packs_reply()]);
    }

    #[test]
    fn a_server_still_starting_is_tried_again() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(settings().retry_every);
        rig.connected();
        rig.clear();
        rig.receive_from_backend(frame(login_cb::LOGIN_DISCONNECT, |body| {
            "starting".to_string().encode(body);
        }));
        assert!(rig.rest.contains(&Action::DropBackend));
        rig.tick(settings().retry_every);
        assert!(rig.rest.contains(&Action::Connect));
    }

    #[test]
    fn a_crash_in_the_middle_of_the_return_starts_the_wait_again() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.log_back_in();
        rig.receive_from_backend(known_packs_question());
        rig.clear();
        rig.lose_backend();
        assert!(rig.events().is_empty());
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        assert!(rig.client.is_empty());
        rig.log_back_in();
        assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 0);
        rig.client.clear();
        rig.receive_from_backend(known_packs_question());
        assert_eq!(rig.client, vec![known_packs_question()]);
        rig.receive_from_client(known_packs_reply());
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        assert_eq!(rig.resumed(), 1);
    }

    #[test]
    fn a_server_that_never_returns_ends_with_an_explanation_in_play() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(Duration::from_secs(9));
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
        rig.tick(Duration::from_secs(2));
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
        assert!(matches!(rig.rest.last(), Some(Action::Close)));
        assert!(matches!(rig.events()[1], Event::GaveUp { .. }));
        rig.clear();
        rig.tick(Duration::from_secs(5));
        assert!(rig.rest.is_empty() && rig.client.is_empty());
    }

    #[test]
    fn no_server_at_all_ends_with_a_login_disconnect() {
        let mut rig = Rig::fresh();
        rig.receive_from_client(hello());
        rig.tick(Duration::from_secs(11));
        assert_eq!(rig.sent_to_client(login_cb::LOGIN_DISCONNECT), 1);
    }

    #[test]
    fn a_server_lost_during_the_configuration_ends_with_a_configuration_disconnect() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_client(hello());
        rig.receive_from_backend(login_finished());
        rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
        rig.clear();
        rig.lose_backend();
        rig.tick(Duration::from_millis(100));
        assert_eq!(rig.sent_to_client(config_cb::KEEP_ALIVE), 1);
        rig.tick(Duration::from_secs(11));
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
    }

    #[test]
    fn a_client_in_configuration_goes_on_with_the_new_server_live() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_client(hello());
        rig.receive_from_backend(login_finished());
        rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
        rig.receive_from_client(information());
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.receive_from_backend(known_packs_question());
        assert_eq!(rig.client, vec![known_packs_question()]);
        rig.receive_from_client(known_packs_reply());
        assert_eq!(rig.backend, vec![known_packs_reply()]);
    }

    #[test]
    fn a_client_told_it_logged_in_is_not_told_twice() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_client(hello());
        rig.receive_from_backend(login_finished());
        rig.clear();
        rig.lose_backend();
        rig.tick(settings().retry_every);
        assert!(!rig.rest.contains(&Action::Connect));

        rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
        assert!(rig.backend.is_empty());
        rig.clear();
        rig.log_back_in();
        assert_eq!(rig.sent_to_client(login_cb::LOGIN_FINISHED), 0);
        assert_eq!(rig.sent_to_backend(login_sb::LOGIN_ACKNOWLEDGED), 1);
        rig.client.clear();
        rig.receive_from_backend(known_packs_question());
        assert_eq!(rig.client, vec![known_packs_question()]);
    }

    #[test]
    fn a_client_that_never_acknowledges_its_login_gets_a_configuration_disconnect() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_client(hello());
        rig.receive_from_backend(login_finished());
        rig.clear();
        rig.lose_backend();
        rig.tick(Duration::from_secs(11));
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
        assert_eq!(rig.sent_to_client(login_cb::LOGIN_DISCONNECT), 0);
    }

    #[test]
    fn a_client_on_its_way_to_a_configuration_is_disconnected_in_configuration() {
        let mut rig = Rig::in_play();
        rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
        rig.clear();
        rig.receive_from_backend(frame(play_cb::SYSTEM_CHAT, |body| {
            body.resize(MAX_PLAIN_FRAME_BYTES + 1, 0);
        }));
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
    }

    #[test]
    fn a_client_told_to_reconfigure_is_disconnected_in_configuration() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(settings().retry_every);
        rig.connected();
        rig.clear();
        rig.tick(Duration::from_secs(11));
        rig.tick(Duration::ZERO);
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
    }

    #[test]
    fn a_client_on_its_way_to_play_reads_nothing_of_a_configuration() {
        let mut rig = Rig::in_configuration();
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.clear();
        rig.lose_backend();
        rig.tick(Duration::ZERO);
        assert_eq!(rig.sent_to_client(config_cb::KEEP_ALIVE), 0);
        rig.tick(Duration::from_secs(11));
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 0);
    }

    #[test]
    fn a_pack_pushed_during_a_replay_reaches_the_client_and_the_replay_goes_on() {
        let pack = Uuid::from_u128(5);
        let mut rig = Rig::in_configuration();
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();

        rig.receive_from_backend(config_push(pack));
        assert_eq!(rig.sent_to_client(config_cb::RESOURCE_PACK_PUSH), 1);
        rig.receive_from_client(config_pack_answer(pack, 0));
        assert_eq!(rig.sent_to_backend(config_sb::RESOURCE_PACK), 1);

        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(registry(1));
        assert_eq!(rig.sent_to_client(config_cb::REGISTRY_DATA), 0);
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 0);
    }

    #[test]
    fn a_pack_the_client_took_stays_out_of_what_a_replay_follows() {
        let pack = Uuid::from_u128(5);
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_client(hello());
        rig.receive_from_backend(login_finished());
        rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
        rig.receive_from_backend(config_push(pack));
        rig.receive_from_client(config_pack_answer(pack, 0));
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_client(known_packs_reply());
        rig.receive_from_backend(registry(1));
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();

        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(config_push(pack));
        rig.receive_from_backend(registry(1));
        assert_eq!(
            rig.backend,
            vec![known_packs_reply(), config_pack_answer(pack, 0)]
        );
        assert!(rig.client.is_empty());
    }

    #[test]
    fn a_pack_for_a_client_that_left_its_configuration_ends_the_session() {
        let mut rig = Rig::in_configuration();
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();

        rig.receive_from_backend(config_push(Uuid::from_u128(5)));
        assert_eq!(rig.sent_to_client(config_cb::RESOURCE_PACK_PUSH), 0);
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
        assert!(rig.rest.contains(&Action::DropBackend));
        assert!(rig.rest.contains(&Action::Close));
    }

    #[test]
    fn a_new_server_does_not_repeat_what_a_configuring_client_received() {
        let mut rig = Rig::in_configuration();
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();

        rig.receive_from_backend(known_packs_question());
        assert_eq!(rig.backend, vec![known_packs_reply()]);
        rig.receive_from_backend(registry(1));
        assert!(rig.client.is_empty());

        rig.receive_from_backend(registry(2));
        assert_eq!(rig.client, vec![registry(2)]);
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        assert_eq!(rig.sent_to_backend(config_sb::FINISH_CONFIGURATION), 1);
        assert_eq!(rig.resumed(), 1);
    }

    #[test]
    fn a_server_that_differs_from_what_a_configuring_client_holds_ends_the_session() {
        let mut rig = Rig::in_configuration();
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(registry(9));
        assert_eq!(rig.sent_to_client(config_cb::REGISTRY_DATA), 0);
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
        assert!(rig.rest.contains(&Action::DropBackend));
        assert!(rig.rest.contains(&Action::Close));
        assert!(matches!(rig.events()[..], [Event::GaveUp { .. }]));
    }

    #[test]
    fn a_question_the_client_has_not_answered_yet_waits_for_its_reply() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_client(hello());
        rig.receive_from_backend(login_finished());
        rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(registry(1));
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();

        rig.receive_from_backend(known_packs_question());
        assert_eq!(rig.sent_to_backend(config_sb::SELECT_KNOWN_PACKS), 0);
        rig.receive_from_client(known_packs_reply());
        assert_eq!(rig.backend, vec![known_packs_reply()]);
        rig.receive_from_backend(registry(1));
        assert!(rig.client.is_empty());
    }

    #[test]
    fn a_client_that_ends_its_configuration_during_a_replay_goes_through_a_new_one() {
        let mut rig = Rig::in_configuration();
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        assert!(rig.rest.contains(&Action::DropBackend));
        assert_eq!(rig.sent_to_backend(config_sb::FINISH_CONFIGURATION), 0);
        rig.tick(Duration::ZERO);
        rig.connected();
        assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 1);
    }

    #[test]
    fn a_server_lost_during_the_login_ends_in_resumed_and_the_next_crash_is_seen() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_client(hello());
        rig.lose_backend();
        rig.tick(settings().retry_every);
        rig.connected();
        rig.receive_from_backend(login_finished());
        rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_client(known_packs_reply());
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        assert_eq!(rig.resumed(), 1);
        rig.receive_from_backend(login());
        rig.clear();

        rig.lose_backend();
        assert_eq!(rig.events(), vec![&Event::BackendLost]);
    }

    #[test]
    fn a_silent_server_in_play_counts_as_lost() {
        let mut rig = Rig::in_play();
        rig.tick(settings().backend_silence - Duration::from_secs(1));
        assert!(rig.rest.is_empty());
        rig.tick(Duration::from_secs(1));
        assert!(rig.rest.contains(&Action::DropBackend));
        assert_eq!(rig.events(), vec![&Event::BackendLost]);
    }

    #[test]
    fn a_server_that_freezes_during_the_return_is_tried_again() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.tick(settings().backend_silence);
        assert!(rig.rest.contains(&Action::DropBackend));
        rig.tick(settings().retry_every);
        assert!(rig.rest.contains(&Action::Connect));
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
    }

    #[test]
    fn a_client_waiting_for_its_packs_does_not_make_a_server_silent() {
        let mut rig = Rig::in_configuration();
        rig.receive_from_backend(config_push(Uuid::from_u128(5)));
        assert_eq!(rig.sent_to_client(config_cb::RESOURCE_PACK_PUSH), 1);
        rig.clear();
        rig.tick(settings().backend_silence);
        assert!(rig.rest.is_empty());
    }

    #[test]
    fn a_server_that_freezes_once_the_client_acknowledged_is_tried_again() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.log_back_in();
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        rig.receive_from_backend(known_packs_question());
        rig.clear();
        rig.tick(settings().backend_silence - Duration::from_secs(1));
        assert!(!rig.rest.contains(&Action::DropBackend));
        rig.tick(Duration::from_secs(1));
        assert!(rig.rest.contains(&Action::DropBackend));
        assert!(rig.events().is_empty());
        rig.tick(settings().retry_every);
        assert!(rig.rest.contains(&Action::Connect));
        assert_eq!(rig.sent_to_client(config_cb::KEEP_ALIVE), 1);
    }

    #[test]
    fn a_client_downloading_a_pack_during_a_return_does_not_make_a_server_silent() {
        let pack = Uuid::from_u128(9);
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.log_back_in();
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        rig.receive_from_backend(config_push(pack));
        rig.receive_from_client(config_pack_answer(pack, 3));
        rig.clear();
        rig.tick(settings().backend_silence);
        assert!(rig.rest.is_empty());
        rig.receive_from_client(config_pack_answer(pack, 0));
        rig.tick(Duration::ZERO);
        assert!(rig.rest.contains(&Action::DropBackend));
    }

    #[test]
    fn a_disconnect_inside_a_bundle_closes_the_bundle_first() {
        let mut rig = Rig::in_play();
        rig.receive_from_backend(empty(play_cb::BUNDLE_DELIMITER));
        rig.clear();
        rig.receive_from_backend(frame(play_cb::SYSTEM_CHAT, |body| {
            body.resize(MAX_PLAIN_FRAME_BYTES + 1, 0);
        }));
        let ids: Vec<i32> = rig.client.iter().map(|f| split(f).unwrap().0).collect();
        assert_eq!(ids, vec![play_cb::BUNDLE_DELIMITER, play_cb::DISCONNECT]);
    }

    #[test]
    fn a_client_that_reaches_its_configuration_waits_the_whole_return_timeout() {
        let patient = Settings {
            return_timeout: Duration::from_secs(60),
            ..settings()
        };
        let mut rig = Rig::with(patient);
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_client(hello());
        rig.receive_from_backend(login_finished());
        rig.lose_backend();
        rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
        rig.clear();
        rig.tick(Duration::from_secs(30));
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 0);
        rig.tick(Duration::from_secs(31));
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
    }

    #[test]
    fn a_question_only_the_client_answers_ends_a_replay_with_a_message() {
        let cookie = frame(config_cb::COOKIE_REQUEST, |body| {
            "minecraft:token".to_string().encode(body);
        });
        let mut rig = Rig::in_configuration();
        rig.receive_from_backend(cookie.clone());
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();

        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(registry(1));
        rig.receive_from_backend(cookie);
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 1);
        assert!(rig.rest.contains(&Action::DropBackend));
        assert!(matches!(
            rig.events()[..],
            [Event::GaveUp { reason }] if reason.contains("cookie")
        ));
    }

    #[test]
    fn a_kick_from_a_live_server_ends_the_session_without_a_loss() {
        let mut rig = Rig::in_play();
        let kick = frame(play_cb::DISCONNECT, |body| {
            text("Kicked").encode(body);
        });
        rig.receive_from_backend(kick.clone());
        assert_eq!(rig.client, vec![kick]);
        assert!(rig.rest.contains(&Action::DropBackend));
        assert!(rig.rest.contains(&Action::Close));
        rig.lose_backend();
        assert!(rig.events().is_empty());
    }

    #[test]
    fn a_packet_a_client_could_not_read_ends_the_session_with_its_size() {
        let mut rig = Rig::in_play();
        let large = frame(play_cb::SYSTEM_CHAT, |body| {
            body.resize(MAX_PLAIN_FRAME_BYTES + 1, 0);
        });
        rig.receive_from_backend(large);
        assert_eq!(rig.sent_to_client(play_cb::SYSTEM_CHAT), 0);
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
        assert!(rig.rest.contains(&Action::DropBackend));
        assert!(rig.rest.contains(&Action::Close));
        assert!(matches!(rig.events()[..], [Event::GaveUp { .. }]));
    }

    #[test]
    fn the_largest_packet_a_client_reads_still_reaches_it() {
        let mut rig = Rig::in_play();
        let largest = frame(play_cb::SYSTEM_CHAT, |body| {
            body.resize(MAX_PLAIN_FRAME_BYTES, 0);
        });
        rig.receive_from_backend(largest);
        assert_eq!(rig.sent_to_client(play_cb::SYSTEM_CHAT), 1);
    }

    #[test]
    fn nothing_of_play_reaches_a_client_on_its_way_to_a_configuration() {
        let pack = Uuid::from_u128(5);
        let mut rig = Rig::in_play();
        rig.receive_from_backend(push(pack, "abc"));
        rig.receive_from_client(pack_answer(pack, 0));
        rig.lose_backend();
        rig.come_back();
        rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
        rig.clear();

        rig.tick(settings().settle);
        assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_POP), 0);
        rig.lose_backend();
        rig.tick(Duration::ZERO);
        assert_eq!(rig.sent_to_client(play_cb::KEEP_ALIVE), 0);
        assert_eq!(rig.sent_to_client(play_cb::SET_ACTION_BAR_TEXT), 0);
    }

    #[test]
    fn a_client_acknowledging_the_end_of_configuration_as_the_server_dies_still_returns() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_client(hello());
        rig.receive_from_backend(login_finished());
        rig.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_client(known_packs_reply());
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.lose_backend();
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        rig.clear();
        rig.tick(settings().retry_every);
        rig.connected();
        assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 1);
        rig.receive_from_backend(login_finished());
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        assert!(!rig.rest.contains(&Action::DropBackend));
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        assert_eq!(rig.sent_to_client(config_cb::FINISH_CONFIGURATION), 1);
        rig.receive_from_client(known_packs_reply());
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        assert_eq!(rig.resumed(), 1);
    }

    #[test]
    fn the_away_message_goes_once_the_client_is_back() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(Duration::ZERO);
        assert_eq!(rig.sent_to_client(play_cb::SET_ACTION_BAR_TEXT), 1);
        rig.come_back();
        rig.clear();
        rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        rig.receive_from_backend(login());
        assert_eq!(rig.sent_to_client(play_cb::SET_ACTION_BAR_TEXT), 0);
    }

    #[test]
    fn the_away_message_is_cleared_right_after_the_login() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(Duration::ZERO);
        rig.log_back_in();
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        rig.clear();
        rig.receive_from_backend(login());
        assert_eq!(rig.client, vec![login(), action_bar("")]);
    }

    #[test]
    fn a_pack_the_client_holds_is_answered_in_its_name() {
        let pack = Uuid::from_u128(5);
        let mut rig = Rig::in_play();
        rig.receive_from_backend(push(pack, "abc"));
        assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_PUSH), 1);
        for status in [3, 4, 0] {
            rig.receive_from_client(pack_answer(pack, status));
        }
        rig.clear();
        rig.lose_backend();
        rig.come_back();
        rig.clear();

        rig.receive_from_backend(push(pack, "abc"));
        assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_PUSH), 0);
        assert_eq!(
            rig.backend,
            vec![
                pack_answer(pack, 3),
                pack_answer(pack, 4),
                pack_answer(pack, 0)
            ]
        );
    }

    fn a_pack_the_client_failed_reaches_it_again_after_a_return(failure: i32) {
        let pack = Uuid::from_u128(5);
        let mut rig = Rig::in_play();
        rig.receive_from_backend(push(pack, "abc"));
        rig.receive_from_client(pack_answer(pack, 3));
        rig.receive_from_client(pack_answer(pack, failure));
        rig.lose_backend();
        rig.come_back();
        rig.clear();

        rig.receive_from_backend(push(pack, "abc"));
        assert_eq!(rig.client, vec![push(pack, "abc")]);
        assert!(rig.backend.is_empty());
    }

    #[test]
    fn a_pack_the_client_failed_to_download_reaches_it_again() {
        a_pack_the_client_failed_reaches_it_again_after_a_return(2);
    }

    #[test]
    fn a_pack_with_an_invalid_url_reaches_the_client_again() {
        a_pack_the_client_failed_reaches_it_again_after_a_return(5);
    }

    #[test]
    fn a_pack_the_client_failed_to_reload_reaches_it_again() {
        a_pack_the_client_failed_reaches_it_again_after_a_return(6);
    }

    #[test]
    fn a_pack_the_client_discarded_reaches_it_again() {
        a_pack_the_client_failed_reaches_it_again_after_a_return(7);
    }

    #[test]
    fn a_server_heard_every_second_is_never_counted_as_lost() {
        let mut rig = Rig::in_play();
        for _ in 0..30 {
            rig.tick(Duration::from_secs(1));
            rig.receive_from_backend(frame(play_cb::KEEP_ALIVE, |body| 1i64.encode(body)));
        }
        assert!(!rig.rest.contains(&Action::DropBackend));
        assert!(rig.events().is_empty());
    }

    #[test]
    fn a_pack_the_client_holds_is_answered_in_its_name_before_it_acknowledges() {
        let pack = Uuid::from_u128(5);
        let mut rig = Rig::in_play();
        rig.receive_from_backend(push(pack, "abc"));
        rig.receive_from_client(pack_answer(pack, 0));
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();

        rig.receive_from_backend(config_push(pack));
        assert_eq!(rig.backend, vec![config_pack_answer(pack, 0)]);
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        assert_eq!(rig.sent_to_client(config_cb::RESOURCE_PACK_PUSH), 0);
    }

    #[test]
    fn a_new_pack_reaches_the_client_and_the_old_one_goes_when_things_settle() {
        let (old, new) = (Uuid::from_u128(5), Uuid::from_u128(6));
        let mut rig = Rig::in_play();
        rig.receive_from_backend(push(old, "abc"));
        rig.receive_from_client(pack_answer(old, 0));
        rig.lose_backend();
        rig.come_back();
        rig.clear();
        rig.receive_from_backend(push(new, "def"));
        assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_PUSH), 1);
        rig.tick(Duration::from_secs(2));
        assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_POP), 0);
        rig.tick(Duration::from_secs(2));
        assert_eq!(
            rig.client.last(),
            Some(&frame(play_cb::RESOURCE_PACK_POP, |body| Some(old).encode(body)))
        );
    }

    #[test]
    fn a_new_pack_during_a_configuration_waits_for_the_client() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.receive_from_backend(config_push(Uuid::from_u128(9)));
        assert!(rig.backend.is_empty());
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        assert_eq!(rig.client, vec![config_push(Uuid::from_u128(9))]);
    }

    #[test]
    fn a_pack_taken_back_during_a_reconfiguration_is_no_longer_held() {
        let pack = Uuid::from_u128(5);
        let mut rig = Rig::in_play();
        rig.receive_from_backend(push(pack, "abc"));
        rig.receive_from_client(pack_answer(pack, 0));
        rig.lose_backend();
        rig.log_back_in();
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(frame(config_cb::RESOURCE_PACK_POP, |body| {
            Some(pack).encode(body);
        }));
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        rig.receive_from_client(known_packs_reply());
        rig.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        rig.receive_from_backend(login());
        rig.clear();

        rig.receive_from_backend(push(pack, "abc"));
        assert_eq!(rig.sent_to_client(play_cb::RESOURCE_PACK_PUSH), 1);
    }

    #[test]
    fn a_bundle_cut_by_a_crash_is_closed_before_anything_else() {
        let mut rig = Rig::in_play();
        rig.receive_from_backend(empty(play_cb::BUNDLE_DELIMITER));
        rig.clear();
        rig.lose_backend();
        assert_eq!(rig.client, vec![empty(play_cb::BUNDLE_DELIMITER)]);
    }

    #[test]
    fn a_closed_bundle_is_left_alone() {
        let mut rig = Rig::in_play();
        rig.receive_from_backend(empty(play_cb::BUNDLE_DELIMITER));
        rig.receive_from_backend(empty(play_cb::BUNDLE_DELIMITER));
        rig.clear();
        rig.lose_backend();
        assert!(rig.client.is_empty());
    }

    #[test]
    fn compression_stays_on_the_backend_side() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_backend(frame(login_cb::LOGIN_COMPRESSION, |body| {
            VarI32(256).encode(body);
        }));
        assert!(rig.rest.contains(&Action::Compress(256)));
        assert!(rig.client.is_empty());
    }

    #[test]
    fn compression_is_followed_on_a_return_too() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(settings().retry_every);
        rig.connected();
        rig.clear();
        rig.receive_from_backend(frame(login_cb::LOGIN_COMPRESSION, |body| {
            VarI32(64).encode(body);
        }));
        assert!(rig.rest.contains(&Action::Compress(64)));
    }

    #[test]
    fn a_server_in_online_mode_is_refused_with_a_reason() {
        let mut rig = Rig::fresh();
        rig.tick(Duration::ZERO);
        rig.connected();
        rig.receive_from_backend(empty(login_cb::HELLO));
        assert_eq!(rig.sent_to_client(login_cb::LOGIN_DISCONNECT), 1);
        assert!(rig.rest.contains(&Action::Close));
    }

    #[test]
    fn a_login_question_only_a_client_could_answer_is_answered_with_nothing() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(settings().retry_every);
        rig.connected();
        rig.receive_from_backend(frame(login_cb::CUSTOM_QUERY, |body| {
            VarI32(3).encode(body);
        }));
        assert_eq!(rig.sent_to_backend(login_sb::CUSTOM_QUERY_ANSWER), 1);
    }

    #[test]
    fn a_server_lost_after_ending_the_configuration_is_replayed_to_its_end() {
        let mut rig = Rig::in_configuration();
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(registry(1));
        rig.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        assert_eq!(rig.sent_to_client(config_cb::FINISH_CONFIGURATION), 0);
        assert_eq!(rig.sent_to_client(config_cb::DISCONNECT), 0);
    }

    #[test]
    fn keep_alives_of_a_configuration_stay_out_of_what_a_replay_follows() {
        let mut rig = Rig::in_configuration();
        rig.receive_from_backend(frame(config_cb::KEEP_ALIVE, |body| 9i64.encode(body)));
        rig.receive_from_backend(frame(config_cb::PING, |body| 4i32.encode(body)));
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(registry(1));
        rig.receive_from_backend(registry(2));
        assert_eq!(rig.client, vec![registry(2)]);
    }

    #[test]
    fn a_crash_after_the_server_asked_for_a_configuration_still_hands_its_frames_over() {
        let mut rig = Rig::in_play();
        rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.receive_from_backend(known_packs_question());
        assert!(rig.client.is_empty());
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        assert_eq!(rig.client, vec![known_packs_question()]);
    }

    #[test]
    fn a_configuration_the_server_asked_for_is_what_a_replay_follows() {
        let mut rig = Rig::in_play();
        rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_client(known_packs_reply());
        rig.receive_from_backend(registry(2));
        rig.lose_backend();
        rig.log_back_in();
        rig.clear();
        rig.receive_from_backend(known_packs_question());
        rig.receive_from_backend(registry(2));
        assert!(rig.client.is_empty());
        assert_eq!(rig.backend, vec![known_packs_reply()]);
    }

    #[test]
    fn a_second_crash_long_after_a_return_gets_the_whole_return_timeout() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.come_back();
        rig.tick(Duration::from_secs(30));
        rig.lose_backend();
        rig.clear();
        rig.tick(Duration::from_secs(9));
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 0);
        rig.tick(Duration::from_secs(2));
        assert_eq!(rig.sent_to_client(play_cb::DISCONNECT), 1);
    }

    #[test]
    fn a_login_cookie_is_answered_with_nothing() {
        let mut rig = Rig::in_play();
        rig.lose_backend();
        rig.tick(settings().retry_every);
        rig.connected();
        rig.clear();
        rig.receive_from_backend(frame(login_cb::COOKIE_REQUEST, |body| {
            "minecraft:token".to_string().encode(body);
        }));
        assert_eq!(
            rig.backend,
            vec![frame(login_sb::COOKIE_RESPONSE, |body| {
                "minecraft:token".to_string().encode(body);
                false.encode(body);
            })]
        );
    }

    #[test]
    fn a_server_initiated_configuration_passes_through() {
        let mut rig = Rig::in_play();
        rig.receive_from_backend(empty(play_cb::START_CONFIGURATION));
        assert_eq!(rig.sent_to_client(play_cb::START_CONFIGURATION), 1);
        rig.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        assert_eq!(rig.sent_to_backend(play_sb::CONFIGURATION_ACKNOWLEDGED), 1);
        rig.receive_from_backend(registry(3));
        assert_eq!(rig.client.last(), Some(&registry(3)));
    }

    #[test]
    fn frames_without_an_id_change_nothing() {
        let mut rig = Rig::in_play();
        rig.receive_from_backend(Bytes::from_static(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]));
        rig.receive_from_client(Bytes::from_static(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]));
        assert!(rig.client.is_empty() && rig.backend.is_empty());
    }
}
