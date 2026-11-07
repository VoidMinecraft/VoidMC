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

mod backend;
mod client;

use std::time::{Duration, Instant};

use bytes::Bytes;
use voidmc_codec::Encode;
use voidmc_data::v26_1_2::packets::configuration::clientbound as config_cb;
use voidmc_data::v26_1_2::packets::login::clientbound as login_cb;
use voidmc_data::v26_1_2::packets::play::clientbound as play_cb;
use voidmc_protocol::clientbound::{Disconnect, KeepAlive};

use crate::frame::{self, MAX_PLAIN_FRAME_BYTES, Packet, text};
use crate::packs::PackBook;
use crate::script::Script;

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
        held: Vec<Packet>,
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
            self.give_up_on_backend(format!(
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
            self.give_up("The server did not come back in time.");
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
                self.send_to_client(frame::build(play_cb::KEEP_ALIVE, |body| {
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
                self.send_to_client(frame::build(config_cb::KEEP_ALIVE, |body| {
                    keep_alive.encode(body)
                }));
            }
            Phase::Login | Phase::Joining | Phase::LeavingConfiguration | Phase::LeavingPlay => {}
        }
    }

    fn give_up_on_backend(&mut self, reason: impl Into<String>) {
        self.actions.push(Action::DropBackend);
        self.give_up(reason);
    }

    fn give_up(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        let message = format!("[keeper] {reason}");
        let disconnect = Disconnect {
            reason: text(&message),
        };
        let goodbye = match self.client_reads() {
            Reads::Login => frame::build(login_cb::LOGIN_DISCONNECT, |body| {
                let json = serde_json::json!({ "text": message });
                json.to_string().encode(body);
            }),
            Reads::Configuration => {
                frame::build(config_cb::DISCONNECT, |body| disconnect.encode(body))
            }
            Reads::Play => {
                self.close_bundle();
                frame::build(play_cb::DISCONNECT, |body| disconnect.encode(body))
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
            self.send_to_client(frame::build(play_cb::RESOURCE_PACK_POP, |body| {
                Some(pack).encode(body);
            }));
        }
    }

    fn close_bundle(&mut self) {
        if std::mem::take(&mut self.bundle_open) {
            self.send_to_client(frame::build(play_cb::BUNDLE_DELIMITER, |_| {}));
        }
    }
}

fn action_bar(message: &str) -> Bytes {
    frame::build(play_cb::SET_ACTION_BAR_TEXT, |body| {
        text(message).encode(body)
    })
}

#[cfg(test)]
mod tests;
