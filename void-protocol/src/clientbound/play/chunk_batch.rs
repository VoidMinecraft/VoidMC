use voidmc_codec::{Decode, Encode};

/// Opens a batch of chunk packets. The vanilla client times the batch to
/// measure how many chunks per tick it can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct ChunkBatchStart {}

/// Closes a batch opened by [`ChunkBatchStart`]. The client answers with
/// `ChunkBatchReceived`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct ChunkBatchFinished {
    /// Number of chunk packets sent since the matching [`ChunkBatchStart`].
    #[codec(varint32)]
    pub batch_size: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn tagged_packet_ids_and_layout() {
        let mut buf = Vec::new();
        PlayPacket::ChunkBatchStart(ChunkBatchStart {}).encode(&mut buf);
        assert_eq!(buf, [0x0C]);

        let mut buf = Vec::new();
        PlayPacket::ChunkBatchFinished(ChunkBatchFinished { batch_size: 300 }).encode(&mut buf);
        assert_eq!(buf, [0x0B, 0xAC, 0x02]);
    }
}
