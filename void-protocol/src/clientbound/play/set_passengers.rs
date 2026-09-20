use voidmc_codec::{Encode, VarI32};

/// An empty list dismounts every passenger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetPassengers {
    pub entity_id: i32,
    pub passengers: Vec<i32>,
}

impl Encode for SetPassengers {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.entity_id).encode(buf);
        VarI32(self.passengers.len() as i32).encode(buf);
        for &id in &self.passengers {
            VarI32(id).encode(buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::ManualPlayPacket;

    #[test]
    fn passenger_ids_and_count_are_varints() {
        let mut bytes = Vec::new();
        ManualPlayPacket::SetPassengers(SetPassengers {
            entity_id: 300,
            passengers: vec![1, 128],
        })
        .encode(&mut bytes);
        assert_eq!(bytes, [0x6b, 0xac, 0x02, 2, 1, 0x80, 1]);
    }

    #[test]
    fn empty_list_dismounts() {
        let mut bytes = Vec::new();
        SetPassengers {
            entity_id: 1,
            passengers: vec![],
        }
        .encode(&mut bytes);
        assert_eq!(bytes, [1, 0]);
    }
}
