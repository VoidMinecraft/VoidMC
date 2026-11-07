//! What the client says: relayed to the live server, or kept by the session.

use std::time::Instant;

use bytes::Bytes;
use uuid::Uuid;
use voidmc_codec::{Decode, VarI32};
use voidmc_data::v26_1_2::packets::configuration::serverbound as config_sb;
use voidmc_data::v26_1_2::packets::login::serverbound as login_sb;
use voidmc_data::v26_1_2::packets::play::serverbound as play_sb;

use super::{Action, Event, OWN_KEEP_ALIVE, OWN_KEEP_ALIVE_MASK, Phase, Session, Stage};
use crate::frame;
use crate::script::Script;

impl Session {
    pub fn on_client(&mut self, frame: Bytes, now: Instant) {
        let Some((id, body)) = frame::id_and_body(&frame) else {
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
}

fn is_own_keep_alive(mut body: &[u8]) -> bool {
    i64::decode(&mut body).is_ok_and(|id| id & OWN_KEEP_ALIVE_MASK == OWN_KEEP_ALIVE)
}
