use uuid::Uuid;
use voidmc_codec::{Encode, VarI32};

#[derive(Debug, Clone)]
pub struct PlayerInfoRemove {
    pub uuids: Vec<Uuid>,
}

impl Encode for PlayerInfoRemove {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.uuids.len() as i32).encode(buf);
        for uuid in &self.uuids {
            uuid.encode(buf);
        }
    }
}
