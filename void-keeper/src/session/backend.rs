//! What the server says: relayed to the client, held for it, or answered in its name.

use std::time::Instant;

use bytes::Bytes;
use uuid::Uuid;
use voidmc_codec::{Decode, Encode, VarI32};
use voidmc_data::v26_1_2::packets::configuration::{
    clientbound as config_cb, serverbound as config_sb,
};
use voidmc_data::v26_1_2::packets::login::{clientbound as login_cb, serverbound as login_sb};
use voidmc_data::v26_1_2::packets::play::{clientbound as play_cb, serverbound as play_sb};
use voidmc_protocol::clientbound::ResourcePackPush;

use super::{Action, Phase, Reads, Session, Stage, action_bar};
use crate::frame::{self, Packet};
use crate::script::needs_a_client;

impl Session {
    pub fn on_backend(&mut self, frame: Bytes, now: Instant) {
        let Some(packet) = Packet::read(frame) else {
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

    pub(super) fn relay_from_backend(&mut self, packet: Packet) {
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
        self.give_up_on_backend(
            "The server is in online mode: the keeper cannot sit in an encrypted link.",
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
            self.send_to_backend(frame::build(response_id, |body| {
                push.id.encode(body);
                VarI32(status).encode(body);
            }));
        }
        true
    }

    fn log_in(&mut self, packet: &Packet, now: Instant) {
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
                    self.send_to_backend(frame::build(login_sb::CUSTOM_QUERY_ANSWER, |answer| {
                        transaction.encode(answer);
                        false.encode(answer);
                    }));
                }
            }
            login_cb::COOKIE_REQUEST => {
                if let Ok(key) = String::decode(&mut body) {
                    self.send_to_backend(frame::build(login_sb::COOKIE_RESPONSE, |answer| {
                        key.encode(answer);
                        false.encode(answer);
                    }));
                }
            }
            login_cb::LOGIN_FINISHED => self.logged_in(),
            _ => {}
        }
    }

    fn start_configuration(&mut self) {
        tracing::info!("client goes through a reconfiguration");
        self.send_to_client(frame::build(play_cb::START_CONFIGURATION, |_| {}));
        self.phase = Phase::LeavingPlay;
    }

    /// A client in play is only taken out of its world once the new server has logged it in: a
    /// frozen server accepts connections too, and never gets that far.
    fn logged_in(&mut self) {
        if self.phase == Phase::Play {
            self.start_configuration();
        }
        self.send_to_backend(frame::build(login_sb::LOGIN_ACKNOWLEDGED, |_| {}));
        if let Some(information) = &self.client_information {
            let information = information.clone();
            self.send_to_backend(frame::build(config_sb::CLIENT_INFORMATION, |body| {
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
    fn replay(&mut self, packet: Packet, now: Instant) {
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
                self.give_up_on_backend(
                    "The server changes its packs for a client that already left its \
                     configuration. Connect again.",
                );
                return;
            }
            self.relay_from_backend(packet);
            return;
        }
        if let Some(reason) = needs_a_client(id) {
            self.give_up_on_backend(format!(
                "The server came back during the configuration and {reason}: only the client \
                 can answer that. Connect again."
            ));
            return;
        }
        if !self.script.follows(step, packet.bytes()) {
            self.give_up_on_backend(format!(
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
    fn answer_for_client(&mut self, packet: &Packet, now: Instant) -> bool {
        let body = packet.body();
        match packet.id() {
            config_cb::KEEP_ALIVE => {
                self.send_to_backend(frame::build(config_sb::KEEP_ALIVE, |out| {
                    out.extend_from_slice(body);
                }));
                true
            }
            config_cb::PING => {
                self.send_to_backend(frame::build(config_sb::PONG, |out| {
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

    fn hold_for_client(&mut self, packet: Packet, now: Instant) {
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
