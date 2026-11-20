use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI32};

use crate::slot::Slot;

pub use voidmc_data::EquipmentSlot;

/// Shows what an entity holds and wears (`CLIENTBOUND_SET_EQUIPMENT`): one
/// or more `(slot, item)` pairs. On the wire each pair is the slot ordinal in
/// a byte, whose high bit says another pair follows, then the item.
#[derive(Debug, Clone, PartialEq)]
pub struct SetEquipment {
    pub entity_id: i32,
    /// Never empty: vanilla always sends at least one pair.
    pub equipment: Vec<(EquipmentSlot, Slot)>,
}

const MORE_FOLLOWS: u8 = 0x80;

impl Encode for SetEquipment {
    fn encode(&self, buf: &mut Vec<u8>) {
        debug_assert!(!self.equipment.is_empty(), "Set Equipment without a slot");
        VarI32(self.entity_id).encode(buf);
        let last = self.equipment.len().saturating_sub(1);
        for (index, (slot, item)) in self.equipment.iter().enumerate() {
            let more = if index == last { 0 } else { MORE_FOLLOWS };
            (slot.ordinal() | more).encode(buf);
            item.encode(buf);
        }
    }
}

impl Decode for SetEquipment {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let entity_id = decoder.decode::<VarI32>()?.0;
        let mut equipment = Vec::new();
        loop {
            let byte = decoder.decode::<u8>()?;
            let slot = EquipmentSlot::from_ordinal(byte & !MORE_FOLLOWS)
                .ok_or(DecodeError::InvalidPacketId(None))?;
            equipment.push((slot, decoder.decode::<Slot>()?));
            if byte & MORE_FOLLOWS == 0 {
                break;
            }
        }
        Ok(SetEquipment {
            entity_id,
            equipment,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    fn item(item_id: i32, count: i32) -> Slot {
        Slot {
            count,
            item_id,
            ..Slot::EMPTY
        }
    }

    #[test]
    fn flags_every_pair_but_the_last_behind_its_packet_id() {
        let packet = SetEquipment {
            entity_id: 300,
            equipment: vec![
                (EquipmentSlot::MainHand, item(7, 2)),
                (EquipmentSlot::Head, Slot::EMPTY),
                (EquipmentSlot::OffHand, item(1, 1)),
            ],
        };
        let mut buf = Vec::new();
        PlayPacket::SetEquipment(packet.clone()).encode(&mut buf);
        assert_eq!(
            buf,
            [
                0x66, 0xAC, 0x02, // packet id, entity id 300
                0x80, 0x02, 0x07, 0x00, 0x00, // main hand: 2 × item 7, no components
                0x85, 0x00, // head: empty
                0x01, 0x01, 0x01, 0x00, 0x00, // off hand, last: 1 × item 1
            ]
        );

        let mut slice = &buf[1..];
        assert_eq!(SetEquipment::decode(&mut slice).unwrap(), packet);
        assert!(slice.is_empty());
    }

    #[test]
    fn an_unknown_slot_ordinal_does_not_decode() {
        let mut slice: &[u8] = &[0x01, 0x08, 0x00];
        assert!(SetEquipment::decode(&mut slice).is_err());
    }
}
