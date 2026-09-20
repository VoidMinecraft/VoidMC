use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, DecodeError, Decoder, Encode, LimitKind, VarI32};

use crate::slot::Slot;

/// `EntityDataSerializers` registration order, Paper 26.1.2.
pub mod serializer {
    pub const BYTE: i32 = 0;
    pub const INT: i32 = 1;
    pub const LONG: i32 = 2;
    pub const FLOAT: i32 = 3;
    pub const STRING: i32 = 4;
    pub const COMPONENT: i32 = 5;
    pub const OPTIONAL_COMPONENT: i32 = 6;
    pub const ITEM_STACK: i32 = 7;
    pub const BOOLEAN: i32 = 8;
    pub const BLOCK_STATE: i32 = 14;
    pub const OPTIONAL_UNSIGNED_INT: i32 = 19;
    pub const POSE: i32 = 20;
    pub const VECTOR3: i32 = 39;
    pub const QUATERNION: i32 = 40;
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntityMetadataValue {
    Byte(i8),
    Int(i32),
    Long(i64),
    Float(f32),
    String(String),
    Component(Nbt),
    OptionalComponent(Option<Nbt>),
    ItemStack(Slot),
    Boolean(bool),
    BlockState(i32),
    /// `None` on the wire is 0; `Some(n)` is `n + 1`.
    OptionalUnsignedInt(Option<u32>),
    Pose(i32),
    Vector3([f32; 3]),
    /// x, y, z, w.
    Quaternion([f32; 4]),
}

impl EntityMetadataValue {
    pub fn serializer_id(&self) -> i32 {
        match self {
            Self::Byte(_) => serializer::BYTE,
            Self::Int(_) => serializer::INT,
            Self::Long(_) => serializer::LONG,
            Self::Float(_) => serializer::FLOAT,
            Self::String(_) => serializer::STRING,
            Self::Component(_) => serializer::COMPONENT,
            Self::OptionalComponent(_) => serializer::OPTIONAL_COMPONENT,
            Self::ItemStack(_) => serializer::ITEM_STACK,
            Self::Boolean(_) => serializer::BOOLEAN,
            Self::BlockState(_) => serializer::BLOCK_STATE,
            Self::OptionalUnsignedInt(_) => serializer::OPTIONAL_UNSIGNED_INT,
            Self::Pose(_) => serializer::POSE,
            Self::Vector3(_) => serializer::VECTOR3,
            Self::Quaternion(_) => serializer::QUATERNION,
        }
    }

    fn encode_value(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Byte(value) => value.encode(buf),
            Self::Int(value) | Self::BlockState(value) | Self::Pose(value) => {
                VarI32(*value).encode(buf)
            }
            Self::Long(value) => value.encode(buf),
            Self::Float(value) => value.encode(buf),
            Self::String(value) => value.encode(buf),
            Self::Component(value) => value.encode(buf),
            Self::OptionalComponent(value) => value.encode(buf),
            Self::ItemStack(slot) => slot.encode(buf),
            Self::Boolean(value) => value.encode(buf),
            Self::OptionalUnsignedInt(value) => {
                VarI32(value.map_or(0, |v| v.wrapping_add(1) as i32)).encode(buf)
            }
            Self::Vector3(value) => {
                for v in value {
                    v.encode(buf);
                }
            }
            Self::Quaternion(value) => {
                for v in value {
                    v.encode(buf);
                }
            }
        }
    }

    fn decode_value(serializer_id: i32, decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(match serializer_id {
            serializer::BYTE => Self::Byte(decoder.decode::<i8>()?),
            serializer::INT => Self::Int(decoder.decode::<VarI32>()?.0),
            serializer::LONG => Self::Long(decoder.decode::<i64>()?),
            serializer::FLOAT => Self::Float(decoder.decode::<f32>()?),
            serializer::STRING => Self::String(decoder.decode::<String>()?),
            serializer::COMPONENT => Self::Component(decoder.decode::<Nbt>()?),
            serializer::OPTIONAL_COMPONENT => {
                Self::OptionalComponent(decoder.decode::<Option<Nbt>>()?)
            }
            serializer::ITEM_STACK => Self::ItemStack(decoder.decode::<Slot>()?),
            serializer::BOOLEAN => Self::Boolean(decoder.decode::<bool>()?),
            serializer::BLOCK_STATE => Self::BlockState(decoder.decode::<VarI32>()?.0),
            serializer::OPTIONAL_UNSIGNED_INT => {
                let raw = decoder.decode::<VarI32>()?.0;
                Self::OptionalUnsignedInt((raw != 0).then(|| (raw as u32).wrapping_sub(1)))
            }
            serializer::POSE => Self::Pose(decoder.decode::<VarI32>()?.0),
            serializer::VECTOR3 => {
                let mut v = [0.0; 3];
                for x in &mut v {
                    *x = decoder.decode::<f32>()?;
                }
                Self::Vector3(v)
            }
            serializer::QUATERNION => {
                let mut v = [0.0; 4];
                for x in &mut v {
                    *x = decoder.decode::<f32>()?;
                }
                Self::Quaternion(v)
            }
            _ => return Err(DecodeError::InvalidLength),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntityMetadataEntry {
    pub index: u8,
    pub value: EntityMetadataValue,
}

/// Wire format: `(index: u8, serializer: VarInt, value)*` then `0xFF`.
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
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let entity_id = decoder.decode::<VarI32>()?.0;
        let mut entries = Vec::new();
        loop {
            let index = decoder.decode::<u8>()?;
            if index == EOF_MARKER {
                break;
            }
            let next_len = entries
                .len()
                .checked_add(1)
                .ok_or(DecodeError::InvalidLength)?;
            decoder.checked_len(
                i32::try_from(next_len).map_err(|_| DecodeError::InvalidLength)?,
                LimitKind::CollectionElements,
                decoder.limits().max_collection_elements,
            )?;
            decoder.charge_elements(1)?;
            decoder.charge_allocation(std::mem::size_of::<EntityMetadataEntry>())?;
            entries
                .try_reserve(1)
                .map_err(|_| DecodeError::AllocationFailed {
                    requested: std::mem::size_of::<EntityMetadataEntry>(),
                })?;
            let serializer_id = decoder.decode::<VarI32>()?.0;
            let value = EntityMetadataValue::decode_value(serializer_id, decoder)?;
            entries.push(EntityMetadataEntry { index, value });
        }
        Ok(SetEntityData { entity_id, entries })
    }
}

impl SetEntityData {
    pub fn new(entity_id: i32) -> Self {
        Self {
            entity_id,
            entries: Vec::new(),
        }
    }

    pub fn with(mut self, index: u8, value: EntityMetadataValue) -> Self {
        self.entries.push(EntityMetadataEntry { index, value });
        self
    }

    pub fn item(entity_id: i32, item: Slot) -> Self {
        Self::new(entity_id).with(8, EntityMetadataValue::ItemStack(item))
    }
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Tag;

    use super::*;

    fn roundtrip(packet: &SetEntityData) -> Vec<u8> {
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        let mut slice = buf.as_slice();
        assert_eq!(&SetEntityData::decode(&mut slice).unwrap(), packet);
        assert!(slice.is_empty());
        buf
    }

    fn text(s: &str) -> Nbt {
        Nbt {
            name: "".into(),
            compound: vec![("text".into(), Tag::String(s.into()))].into(),
        }
    }

    #[test]
    fn item_metadata_roundtrips() {
        let buf = roundtrip(&SetEntityData::item(42, Slot::simple(1, 5)));
        assert_eq!(buf[..3], [42, 8, 7]);
        assert_eq!(buf.last(), Some(&0xFF));
    }

    #[test]
    fn every_serializer_roundtrips_with_its_registration_id() {
        use EntityMetadataValue as V;
        let values = [
            (V::Byte(-3), 0),
            (V::Int(300), 1),
            (V::Long(-1), 2),
            (V::Float(1.5), 3),
            (V::String("hi".into()), 4),
            (V::Component(text("a")), 5),
            (V::OptionalComponent(None), 6),
            (V::OptionalComponent(Some(text("b"))), 6),
            (V::ItemStack(Slot::simple(2, 1)), 7),
            (V::Boolean(true), 8),
            (V::BlockState(9), 14),
            (V::OptionalUnsignedInt(None), 19),
            (V::OptionalUnsignedInt(Some(0)), 19),
            (V::Pose(6), 20),
            (V::Vector3([1.0, 2.0, 3.0]), 39),
            (V::Quaternion([0.0, 0.0, 0.0, 1.0]), 40),
        ];
        for (index, (value, id)) in values.into_iter().enumerate() {
            assert_eq!(value.serializer_id(), id);
            let packet = SetEntityData::new(1).with(index as u8, value);
            let buf = roundtrip(&packet);
            assert_eq!(buf[1], index as u8);
            assert_eq!(buf[2], id as u8);
        }
    }

    #[test]
    fn exact_bytes_for_flags_name_and_optional_uint() {
        use EntityMetadataValue as V;
        let packet = SetEntityData::new(5)
            .with(0, V::Byte(0x60))
            .with(5, V::Boolean(true))
            .with(19, V::OptionalUnsignedInt(Some(4)));
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        assert_eq!(buf, [5, 0, 0, 0x60, 5, 8, 1, 19, 19, 5, 0xFF]);
    }

    #[test]
    fn unknown_serializer_is_rejected() {
        let bytes = [1u8, 0, 60, 0, 0xFF];
        assert!(SetEntityData::decode(&mut &bytes[..]).is_err());
    }
}
