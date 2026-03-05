use void_codec::{Encode, VarI32};

#[derive(Debug, Clone)]
pub struct RemoveEntities {
    pub entity_ids: Vec<i32>,
}

impl Encode for RemoveEntities {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.entity_ids.len() as i32).encode(buf);
        for &id in &self.entity_ids {
            VarI32(id).encode(buf);
        }
    }
}
