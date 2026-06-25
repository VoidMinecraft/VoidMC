use voidmc_codec::{Decode, DecodeError, Encode, VarI32};

use crate::slot::Slot;

/// `minecraft:data_component_type`-style serializer ids from
/// `EntityDataSerializers` (26.1.2 registration order).
mod serializer {
    pub const ITEM_STACK: i32 = 7;
}

/// A single tracked-data value in an entity's synced metadata.
///
/// Only the variants the server emits are modelled; extend as needed.
#[derive(Debug, Clone, PartialEq)]
pub enum EntityMetadataValue {
    /// `EntityDataSerializers.ITEM_STACK` — an item slot (e.g. a dropped item).
    ItemStack(Slot),
}

impl EntityMetadataValue {
    fn serializer_id(&self) -> i32 {
        match self {
            EntityMetadataValue::ItemStack(_) => serializer::ITEM_STACK,
        }
    }

    fn encode_value(&self, buf: &mut Vec<u8>) {
        match self {
            EntityMetadataValue::ItemStack(slot) => slot.encode(buf),
        }
    }

    fn decode_value(serializer_id: i32, buf: &mut &[u8]) -> Result<Self, DecodeError> {
        match serializer_id {
            serializer::ITEM_STACK => Ok(EntityMetadataValue::ItemStack(Slot::decode(buf)?)),
            _ => Err(DecodeError::InvalidLength),
        }
    }
}

/// One indexed metadata entry (`index` is the entity's data-field id).
#[derive(Debug, Clone, PartialEq)]
pub struct EntityMetadataEntry {
    pub index: u8,
    pub value: EntityMetadataValue,
}

/// Updates an entity's synced metadata (`CLIENTBOUND_SET_ENTITY_DATA`).
///
/// The wire format is a sequence of `(index: u8, serializer: VarInt, value)`
/// entries terminated by the `0xFF` end marker.
#[derive(Debug, Clone, PartialEq)]
pub struct SetEntityData {
    pub entity_id: i32,
    pub entries: Vec<EntityMetadataEntry>,
}

const EOF_MARKER: u8 = 0xFF;

impl Encode for SetEntityData {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.entity_id).encode(buf);
        for entry in &self.entries {
            buf.push(entry.index);
            VarI32(entry.value.serializer_id()).encode(buf);
            entry.value.encode_value(buf);
        }
        buf.push(EOF_MARKER);
    }
}

impl Decode for SetEntityData {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let entity_id = VarI32::decode(buf)?.0;
        let mut entries = Vec::new();
        loop {
            let index = u8::decode(buf)?;
            if index == EOF_MARKER {
                break;
            }
            let serializer_id = VarI32::decode(buf)?.0;
            let value = EntityMetadataValue::decode_value(serializer_id, buf)?;
            entries.push(EntityMetadataEntry { index, value });
        }
        Ok(SetEntityData { entity_id, entries })
    }
}

impl SetEntityData {
    /// Builds the metadata packet for a dropped-item entity (its `Item` field is
    /// index 8).
    pub fn item(entity_id: i32, item: Slot) -> Self {
        SetEntityData {
            entity_id,
            entries: vec![EntityMetadataEntry {
                index: 8,
                value: EntityMetadataValue::ItemStack(item),
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_metadata_roundtrips() {
        let packet = SetEntityData::item(42, Slot::simple(1, 5));
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        // entity_id(42) + index(8) + serializer(7) + slot(count1,id1) + 0xFF
        assert_eq!(buf.last(), Some(&0xFF));
        let mut slice = buf.as_slice();
        let decoded = SetEntityData::decode(&mut slice).unwrap();
        assert_eq!(decoded, packet);
        assert_eq!(slice.len(), 0);
    }
}
