use voidmc_codec::{Decode, Encode};

/// `ClientboundUpdateAttributesPacket`: full snapshots (base + every
/// modifier) of the listed attributes; attributes not listed are untouched.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct UpdateAttributes {
    #[codec(varint32)]
    pub entity_id: i32,
    pub attributes: Vec<AttributeSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct AttributeSnapshot {
    #[codec(varint32)]
    pub attribute_id: i32,
    pub base: f64,
    pub modifiers: Vec<AttributeModifier>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct AttributeModifier {
    pub id: String,
    pub amount: f64,
    pub operation: ModifierOperation,
}

/// `AttributeModifier.Operation`, applied in this order on top of the base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Encode, Decode)]
#[repr(u8)]
pub enum ModifierOperation {
    #[default]
    AddValue = 0,
    AddMultipliedBase = 1,
    AddMultipliedTotal = 2,
}

#[cfg(test)]
mod tests {
    use voidmc_data::Version;

    use super::*;
    use crate::clientbound::PlayPacket;

    fn attribute_id(name: &str) -> i32 {
        voidmc_data::protocol_registry_index(Version::V26_1_2, "minecraft:attribute", name).unwrap()
    }

    #[test]
    fn matches_paper_layout_with_several_modifiers() {
        let speed = attribute_id("minecraft:movement_speed");
        let packet = UpdateAttributes {
            entity_id: 300,
            attributes: vec![AttributeSnapshot {
                attribute_id: speed,
                base: 0.1,
                modifiers: vec![
                    AttributeModifier {
                        id: "voidmc:kart_boost".into(),
                        amount: 0.5,
                        operation: ModifierOperation::AddMultipliedTotal,
                    },
                    AttributeModifier {
                        id: "minecraft:effect.speed".into(),
                        amount: -0.25,
                        operation: ModifierOperation::AddValue,
                    },
                ],
            }],
        };
        let mut buf = Vec::new();
        PlayPacket::UpdateAttributes(packet.clone()).encode(&mut buf);

        let mut expected = vec![0x83, 0x01, 0xAC, 0x02, 0x01, speed as u8];
        expected.extend_from_slice(&0.1f64.to_be_bytes());
        expected.push(0x02);
        expected.push(17);
        expected.extend_from_slice(b"voidmc:kart_boost");
        expected.extend_from_slice(&0.5f64.to_be_bytes());
        expected.push(0x02);
        expected.push(22);
        expected.extend_from_slice(b"minecraft:effect.speed");
        expected.extend_from_slice(&(-0.25f64).to_be_bytes());
        expected.push(0x00);
        assert_eq!(buf, expected);

        let mut slice = &buf[2..];
        let decoded = UpdateAttributes::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded, packet);
    }

    #[test]
    fn empty_modifier_list_and_several_attributes() {
        let scale = attribute_id("minecraft:scale");
        let gravity = attribute_id("minecraft:gravity");
        let mut buf = Vec::new();
        UpdateAttributes {
            entity_id: 7,
            attributes: vec![
                AttributeSnapshot {
                    attribute_id: scale,
                    base: 2.0,
                    modifiers: vec![],
                },
                AttributeSnapshot {
                    attribute_id: gravity,
                    base: 0.04,
                    modifiers: vec![AttributeModifier {
                        id: "a:b".into(),
                        amount: 1.0,
                        operation: ModifierOperation::AddMultipliedBase,
                    }],
                },
            ],
        }
        .encode(&mut buf);

        let mut expected = vec![0x07, 0x02, scale as u8];
        expected.extend_from_slice(&2.0f64.to_be_bytes());
        expected.push(0x00);
        expected.push(gravity as u8);
        expected.extend_from_slice(&0.04f64.to_be_bytes());
        expected.extend_from_slice(&[0x01, 0x03, b'a', b':', b'b']);
        expected.extend_from_slice(&1.0f64.to_be_bytes());
        expected.push(0x01);
        assert_eq!(buf, expected);
    }

    #[test]
    fn operation_ids_match_paper() {
        assert_eq!(ModifierOperation::AddValue as u8, 0);
        assert_eq!(ModifierOperation::AddMultipliedBase as u8, 1);
        assert_eq!(ModifierOperation::AddMultipliedTotal as u8, 2);
        let mut buf = Vec::new();
        ModifierOperation::AddMultipliedTotal.encode(&mut buf);
        assert_eq!(buf, [2]);
    }
}
