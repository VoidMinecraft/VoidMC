use uuid::Uuid;
use voidmc_codec::{Decode, Encode};

/// Serverbound Chat Session Update (0x0A): the chat signing key of a client
/// signed in with a Mojang account, sent right after it joins, whatever the
/// server's online mode.
#[derive(Debug, Encode, Decode)]
pub struct ChatSessionUpdate {
    pub session_id: Uuid,
    /// Expiry of the public key, in epoch milliseconds.
    pub expires_at: i64,
    /// DER-encoded RSA public key.
    pub public_key: Vec<u8>,
    /// Mojang's signature over the key.
    pub key_signature: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_session_and_key_blobs() {
        let mut bytes = vec![0x11; 16];
        bytes.extend(1_700_000_000_000i64.to_be_bytes());
        bytes.extend([3, 1, 2, 3]);
        bytes.extend([0x80, 0x04]);
        bytes.extend([0x55; 512]);
        let packet = ChatSessionUpdate::decode(&mut bytes.as_slice()).unwrap();
        assert_eq!(packet.session_id, Uuid::from_bytes([0x11; 16]));
        assert_eq!(packet.expires_at, 1_700_000_000_000);
        assert_eq!(packet.public_key, [1, 2, 3]);
        assert_eq!(packet.key_signature, [0x55; 512]);
    }
}
