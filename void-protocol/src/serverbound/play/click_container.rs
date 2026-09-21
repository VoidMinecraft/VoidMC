use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI32};

/// Unknown ids decode as `Pickup`, matching vanilla's `ByIdMap` fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerInput {
    Pickup,
    QuickMove,
    Swap,
    Clone,
    Throw,
    QuickCraft,
    PickupAll,
}

impl ContainerInput {
    pub fn id(self) -> i32 {
        match self {
            ContainerInput::Pickup => 0,
            ContainerInput::QuickMove => 1,
            ContainerInput::Swap => 2,
            ContainerInput::Clone => 3,
            ContainerInput::Throw => 4,
            ContainerInput::QuickCraft => 5,
            ContainerInput::PickupAll => 6,
        }
    }

    pub fn from_id(id: i32) -> Self {
        match id {
            1 => ContainerInput::QuickMove,
            2 => ContainerInput::Swap,
            3 => ContainerInput::Clone,
            4 => ContainerInput::Throw,
            5 => ContainerInput::QuickCraft,
            6 => ContainerInput::PickupAll,
            _ => ContainerInput::Pickup,
        }
    }
}

impl Encode for ContainerInput {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.id()).encode(buf);
    }
}

impl Decode for ContainerInput {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(ContainerInput::from_id(decoder.decode::<VarI32>()?.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct HashedComponent {
    #[codec(varint32)]
    pub type_id: i32,
    pub hash: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct HashedItem {
    #[codec(varint32)]
    pub item_id: i32,
    #[codec(varint32)]
    pub count: i32,
    pub added: Vec<HashedComponent>,
    pub removed: Vec<VarI32>,
}

/// What the client believes a slot holds after its own prediction: only the
/// item, count and CRC32C hashes of components, never the components
/// themselves. The server is authoritative and recomputes the true result.
pub type HashedStack = Option<HashedItem>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedSlot {
    pub slot: i16,
    pub item: HashedStack,
}

impl Encode for ChangedSlot {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.slot.encode(buf);
        self.item.encode(buf);
    }
}

impl Decode for ChangedSlot {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(ChangedSlot {
            slot: decoder.decode::<i16>()?,
            item: decoder.decode::<HashedStack>()?,
        })
    }
}

/// `slot` is the window slot, or `-999` for a click outside the window.
/// `changed_slots` holds at most 128 entries, as in vanilla.
#[derive(Debug, Encode)]
pub struct ClickContainer {
    #[codec(varint32)]
    pub container_id: i32,
    #[codec(varint32)]
    pub state_id: i32,
    pub slot: i16,
    pub button: i8,
    pub input: ContainerInput,
    pub changed_slots: Vec<ChangedSlot>,
    pub carried: HashedStack,
}

impl ClickContainer {
    pub const SLOT_OUTSIDE: i16 = -999;
    pub const MAX_CHANGED_SLOTS: usize = 128;
}

impl Decode for ClickContainer {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let container_id = decoder.decode::<VarI32>()?.0;
        let state_id = decoder.decode::<VarI32>()?.0;
        let slot = decoder.decode::<i16>()?;
        let button = decoder.decode::<i8>()?;
        let input = decoder.decode::<ContainerInput>()?;
        let changed_slots = decoder.decode::<Vec<ChangedSlot>>()?;
        if changed_slots.len() > Self::MAX_CHANGED_SLOTS {
            return Err(DecodeError::InvalidLength);
        }
        let carried = decoder.decode::<HashedStack>()?;
        Ok(ClickContainer {
            container_id,
            state_id,
            slot,
            button,
            input,
            changed_slots,
            carried,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_minimal_click() {
        let buf = [0x00, 0x01, 0x00, 0x24, 0x00, 0x00, 0x00, 0x00];
        let mut slice = &buf[..];
        let packet = ClickContainer::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(packet.container_id, 0);
        assert_eq!(packet.state_id, 1);
        assert_eq!(packet.slot, 36);
        assert_eq!(packet.button, 0);
        assert_eq!(packet.input, ContainerInput::Pickup);
        assert!(packet.changed_slots.is_empty());
        assert_eq!(packet.carried, None);
    }

    #[test]
    fn decodes_changed_slots_and_hashed_carried_item() {
        let buf = [
            0x05, 0x07, 0xFC, 0x19, 0x01, 0x00, 0x02, 0x00, 0x03, 0x01, 0x01, 0x20, 0x01, 0x03,
            0x12, 0x34, 0x56, 0x78, 0x01, 0x19, 0x00, 0x04, 0x00, 0x01, 0x01, 0x0A, 0x00, 0x00,
        ];
        let mut slice = &buf[..];
        let packet = ClickContainer::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(packet.container_id, 5);
        assert_eq!(packet.state_id, 7);
        assert_eq!(packet.slot, -999);
        assert_eq!(packet.button, 1);
        assert_eq!(packet.input, ContainerInput::Pickup);
        assert_eq!(
            packet.changed_slots,
            vec![
                ChangedSlot {
                    slot: 3,
                    item: Some(HashedItem {
                        item_id: 1,
                        count: 32,
                        added: vec![HashedComponent {
                            type_id: 3,
                            hash: 0x1234_5678,
                        }],
                        removed: vec![VarI32(25)],
                    }),
                },
                ChangedSlot {
                    slot: 4,
                    item: None,
                },
            ]
        );
        assert_eq!(
            packet.carried,
            Some(HashedItem {
                item_id: 1,
                count: 10,
                added: vec![],
                removed: vec![],
            })
        );
    }

    #[test]
    fn unknown_input_falls_back_to_pickup() {
        let buf = [0x00, 0x01, 0x00, 0x24, 0x00, 0x07, 0x00, 0x00];
        let mut slice = &buf[..];
        let packet = ClickContainer::decode(&mut slice).unwrap();
        assert_eq!(packet.input, ContainerInput::Pickup);
        assert!(slice.is_empty());
    }

    #[test]
    fn rejects_more_than_128_changed_slots() {
        let mut buf = vec![0x00, 0x01, 0x00, 0x24, 0x00, 0x00, 0x81, 0x01];
        for i in 0..129i16 {
            buf.extend_from_slice(&i.to_be_bytes());
            buf.push(0x00);
        }
        buf.push(0x00);
        let mut slice = &buf[..];
        assert!(matches!(
            ClickContainer::decode(&mut slice),
            Err(DecodeError::InvalidLength)
        ));
        buf[6] = 0x80;
        buf.truncate(8 + 128 * 3);
        buf.push(0x00);
        let mut slice = &buf[..];
        assert_eq!(
            ClickContainer::decode(&mut slice)
                .unwrap()
                .changed_slots
                .len(),
            128
        );
    }

    #[test]
    fn round_trips() {
        let packet = ClickContainer {
            container_id: 2,
            state_id: 3,
            slot: 10,
            button: 0,
            input: ContainerInput::QuickMove,
            changed_slots: vec![ChangedSlot {
                slot: 10,
                item: None,
            }],
            carried: None,
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        let mut slice = buf.as_slice();
        let decoded = ClickContainer::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded.input, ContainerInput::QuickMove);
        assert_eq!(decoded.changed_slots, packet.changed_slots);
    }
}
