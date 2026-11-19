use voidmc_codec::{Decode, Encode};

/// The client's answer to a `ChunkBatchFinished`: how many chunks per tick it
/// would like to receive.
#[derive(Debug, Encode, Decode)]
pub struct ChunkBatchReceived {
    pub desired_chunks_per_tick: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverbound::PlayPacket;

    #[test]
    fn decodes_the_tagged_packet() {
        let mut buf = vec![0x0B];
        buf.extend_from_slice(&25.0f32.to_be_bytes());
        let mut slice = buf.as_slice();
        let PlayPacket::ChunkBatchReceived(packet) = PlayPacket::decode(&mut slice).unwrap() else {
            panic!("decoded another packet");
        };
        assert!(slice.is_empty());
        assert_eq!(packet.desired_chunks_per_tick, 25.0);
    }
}
