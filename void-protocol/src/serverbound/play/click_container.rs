use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI32};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum ContainerInput {
    Pickup = 0,
    QuickMove = 1,
    Swap = 2,
    Clone = 3,
    Throw = 4,
    QuickCraft = 5,
    PickupAll = 6,
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
#[derive(Debug, Encode, Decode)]
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
    fn rejects_unknown_input() {
        let buf = [0x00, 0x01, 0x00, 0x24, 0x00, 0x07, 0x00, 0x00];
        let mut slice = &buf[..];
        assert!(ClickContainer::decode(&mut slice).is_err());
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
