use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, DecodeError, Decoder, Encode, LimitKind, VarI32, VarI64};

use crate::clientbound::Particle;
use crate::slot::Slot;
use crate::types::BlockPosition;

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
    pub const OPTIONAL_BLOCK_POS: i32 = 11;
    pub const BLOCK_STATE: i32 = 14;
    pub const PARTICLES: i32 = 17;
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
    OptionalBlockPos(Option<BlockPosition>),
    BlockState(i32),
    Particles(Vec<Particle>),
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
            Self::OptionalBlockPos(_) => serializer::OPTIONAL_BLOCK_POS,
            Self::BlockState(_) => serializer::BLOCK_STATE,
            Self::Particles(_) => serializer::PARTICLES,
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
            Self::Long(value) => VarI64(*value).encode(buf),
            Self::Float(value) => value.encode(buf),
            Self::String(value) => value.encode(buf),
            Self::Component(value) => value.encode(buf),
            Self::OptionalComponent(value) => value.encode(buf),
            Self::ItemStack(slot) => slot.encode(buf),
            Self::Boolean(value) => value.encode(buf),
            Self::OptionalBlockPos(value) => value.encode(buf),
            Self::Particles(value) => value.encode(buf),
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
            serializer::LONG => Self::Long(decoder.decode::<VarI64>()?.0),
            serializer::FLOAT => Self::Float(decoder.decode::<f32>()?),
            serializer::STRING => Self::String(decoder.decode::<String>()?),
            serializer::COMPONENT => Self::Component(decoder.decode::<Nbt>()?),
            serializer::OPTIONAL_COMPONENT => {
                Self::OptionalComponent(decoder.decode::<Option<Nbt>>()?)
            }
            serializer::ITEM_STACK => Self::ItemStack(decoder.decode::<Slot>()?),
            serializer::BOOLEAN => Self::Boolean(decoder.decode::<bool>()?),
            serializer::OPTIONAL_BLOCK_POS => {
                Self::OptionalBlockPos(decoder.decode::<Option<BlockPosition>>()?)
            }
            serializer::BLOCK_STATE => Self::BlockState(decoder.decode::<VarI32>()?.0),
            serializer::PARTICLES => Self::Particles(decoder.decode::<Vec<Particle>>()?),
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
        Self::new(entity_id).with(
            super::entity_metadata::item_entity_index::ITEM,
            EntityMetadataValue::ItemStack(item),
        )
    }
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Tag;

    use super::*;
    use crate::clientbound::ParticleColor;
    use crate::clientbound::entity_metadata::{end_crystal_index, living_entity_index};

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
            (V::OptionalBlockPos(None), 11),
            (
                V::OptionalBlockPos(Some(BlockPosition { x: -1, y: 64, z: 3 })),
                11,
            ),
            (V::BlockState(9), 14),
            (V::Particles(vec![]), 17),
            (
                V::Particles(vec![
                    Particle::Flame,
                    Particle::EntityEffect {
                        color: ParticleColor(0x7F00_FF00),
                    },
                ]),
                17,
            ),
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
    fn exact_bytes_for_end_crystal_indices() {
        use EntityMetadataValue as V;
        let packet = SetEntityData::new(45)
            .with(
                end_crystal_index::BEAM_TARGET,
                V::OptionalBlockPos(Some(BlockPosition { x: 1, y: 2, z: 3 })),
            )
            .with(end_crystal_index::SHOW_BOTTOM, V::Boolean(false));
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        assert_eq!(
            buf,
            [45, 8, 11, 1, 0, 0, 0, 0x40, 0, 0, 0x30, 0x02, 9, 8, 0, 0xFF]
        );
        roundtrip(&packet);
    }

    #[test]
    fn exact_bytes_for_living_entity_effect_indices() {
        use EntityMetadataValue as V;
        let entity_effect =
            voidmc_data::particle_type_id(voidmc_data::Version::V26_1_2, "minecraft:entity_effect")
                .unwrap() as u8;
        let packet = SetEntityData::new(9)
            .with(
                living_entity_index::EFFECT_PARTICLES,
                V::Particles(vec![Particle::EntityEffect {
                    color: ParticleColor(0xFF33_EEFF_u32 as i32),
                }]),
            )
            .with(living_entity_index::EFFECT_AMBIENCE, V::Boolean(true));
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        assert_eq!(
            buf,
            [
                9,
                10,
                17,
                1,
                entity_effect,
                0xFF,
                0x33,
                0xEE,
                0xFF,
                11,
                8,
                1,
                0xFF
            ]
        );
        roundtrip(&packet);
    }

    #[test]
    fn long_is_a_varint_on_the_wire() {
        let packet = SetEntityData::new(1).with(7, EntityMetadataValue::Long(300));
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        assert_eq!(buf, [1, 7, 2, 0xac, 0x02, 0xFF]);
        let packet = SetEntityData::new(1).with(7, EntityMetadataValue::Long(-1));
        buf.clear();
        packet.encode(&mut buf);
        assert_eq!(buf.len(), 3 + 10 + 1);
        roundtrip(&packet);
    }

    #[test]
    fn unknown_serializer_is_rejected() {
        let bytes = [1u8, 0, 60, 0, 0xFF];
        assert!(SetEntityData::decode(&mut &bytes[..]).is_err());
    }
}
