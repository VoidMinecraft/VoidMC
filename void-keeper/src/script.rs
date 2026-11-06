//! What a configuration looks like, so that a new server can be told from the last one.
//!
//! The script is a fingerprint of each clientbound frame, in order, plus the reply only a
//! client can give. As long as a backend follows it frame by frame, the proxy can stand in
//! for the client and the client's registries stay valid.

use std::hash::{DefaultHasher, Hasher};

use bytes::Bytes;
use voidmc_data::v26_1_2::packets::configuration::clientbound as cb;

use crate::wire::split;

#[derive(Debug, Default)]
pub struct Script {
    fingerprints: Vec<u64>,
    known_packs_reply: Option<Bytes>,
}

fn fingerprint(frame: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(frame);
    hasher.finish()
}

/// Why a proxy cannot answer this packet in the client's name, if it cannot.
pub fn needs_a_client(frame: &[u8]) -> Option<&'static str> {
    match split(frame)?.0 {
        cb::COOKIE_REQUEST => Some("the server asks for a cookie"),
        cb::CODE_OF_CONDUCT => Some("the server shows a code of conduct"),
        cb::SHOW_DIALOG => Some("the server shows a dialog"),
        cb::TRANSFER => Some("the server transfers the client"),
        cb::RESOURCE_PACK_POP => Some("the server takes a pack back"),
        _ => None,
    }
}

impl Script {
    pub fn record(&mut self, frame: &[u8]) {
        self.fingerprints.push(fingerprint(frame));
    }

    pub fn known_packs_reply(&self) -> Option<&Bytes> {
        self.known_packs_reply.as_ref()
    }

    pub fn set_known_packs_reply(&mut self, frame: Bytes) {
        self.known_packs_reply = Some(frame);
    }

    /// Whether `frame` is what this script holds at `step`.
    pub fn follows(&self, step: usize, frame: &[u8]) -> bool {
        self.fingerprints.get(step) == Some(&fingerprint(frame))
    }

    pub fn is_finished_at(&self, step: usize) -> bool {
        step == self.fingerprints.len()
    }

    /// One value for the whole configuration, for the logs.
    pub fn digest(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for fingerprint in &self.fingerprints {
            hasher.write_u64(*fingerprint);
        }
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::frame;

    #[test]
    fn a_script_is_followed_frame_by_frame() {
        let mut script = Script::default();
        script.record(&[1, 2]);
        script.record(&[3]);
        assert!(script.follows(0, &[1, 2]));
        assert!(!script.follows(1, &[1, 2]));
        assert!(!script.follows(2, &[3]));
        assert!(script.is_finished_at(2));
    }

    #[test]
    fn a_digest_sees_order_and_content() {
        let build = |frames: &[&[u8]]| {
            let mut script = Script::default();
            frames.iter().for_each(|frame| script.record(frame));
            script.digest()
        };
        assert_eq!(build(&[&[1], &[2]]), build(&[&[1], &[2]]));
        assert_ne!(build(&[&[1], &[2]]), build(&[&[2], &[1]]));
    }

    #[test]
    fn a_question_only_a_client_can_answer_is_named() {
        assert!(needs_a_client(&frame(cb::COOKIE_REQUEST, |_| {})).is_some());
        assert!(needs_a_client(&frame(cb::REGISTRY_DATA, |_| {})).is_none());
    }
}
