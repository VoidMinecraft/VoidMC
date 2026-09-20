//! Synched-data indices from Paper 26.1.2 (`Entity`, `Display` and its
//! subclasses), in `defineId` order.

use super::{EntityMetadataEntry, EntityMetadataValue as Value};

pub mod entity_index {
    pub const FLAGS: u8 = 0;
    pub const AIR_SUPPLY: u8 = 1;
    pub const CUSTOM_NAME: u8 = 2;
    pub const CUSTOM_NAME_VISIBLE: u8 = 3;
    pub const SILENT: u8 = 4;
    pub const NO_GRAVITY: u8 = 5;
    pub const POSE: u8 = 6;
    pub const TICKS_FROZEN: u8 = 7;
}

pub mod entity_flag {
    pub const ON_FIRE: u8 = 1 << 0;
    pub const SNEAKING: u8 = 1 << 1;
    pub const SPRINTING: u8 = 1 << 3;
    pub const SWIMMING: u8 = 1 << 4;
    pub const INVISIBLE: u8 = 1 << 5;
    pub const GLOWING: u8 = 1 << 6;
    pub const FALL_FLYING: u8 = 1 << 7;
}

pub mod item_entity_index {
    pub const ITEM: u8 = 8;
}

pub mod display_index {
    pub const TRANSFORMATION_START_DELTA_TICKS: u8 = 8;
    pub const TRANSFORMATION_DURATION: u8 = 9;
    pub const POS_ROT_DURATION: u8 = 10;
    pub const TRANSLATION: u8 = 11;
    pub const SCALE: u8 = 12;
    pub const LEFT_ROTATION: u8 = 13;
    pub const RIGHT_ROTATION: u8 = 14;
    pub const BILLBOARD: u8 = 15;
    pub const BRIGHTNESS: u8 = 16;
    pub const VIEW_RANGE: u8 = 17;
    pub const SHADOW_RADIUS: u8 = 18;
    pub const SHADOW_STRENGTH: u8 = 19;
    pub const WIDTH: u8 = 20;
    pub const HEIGHT: u8 = 21;
    pub const GLOW_COLOR: u8 = 22;
    pub const BLOCK_STATE: u8 = 23;
    pub const ITEM: u8 = 23;
    pub const ITEM_DISPLAY_CONTEXT: u8 = 24;
    pub const TEXT: u8 = 23;
    pub const TEXT_LINE_WIDTH: u8 = 24;
    pub const TEXT_BACKGROUND_COLOR: u8 = 25;
    pub const TEXT_OPACITY: u8 = 26;
    pub const TEXT_FLAGS: u8 = 27;
}

pub mod text_display_flag {
    pub const SHADOW: u8 = 1 << 0;
    pub const SEE_THROUGH: u8 = 1 << 1;
    pub const DEFAULT_BACKGROUND: u8 = 1 << 2;
    pub const ALIGN_CENTER: u8 = 0;
    pub const ALIGN_LEFT: u8 = 1 << 3;
    pub const ALIGN_RIGHT: u8 = 2 << 3;
}

/// Translation, left rotation, scale, right rotation — applied in that order
/// on the client. Block models occupy `[0, 1]³`; item models are centred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayTransform {
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub left_rotation: [f32; 4],
    pub right_rotation: [f32; 4],
}

impl Default for DisplayTransform {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            scale: [1.0; 3],
            left_rotation: [0.0, 0.0, 0.0, 1.0],
            right_rotation: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

impl DisplayTransform {
    pub fn translation(mut self, x: f32, y: f32, z: f32) -> Self {
        self.translation = [x, y, z];
        self
    }

    pub fn scale(mut self, x: f32, y: f32, z: f32) -> Self {
        self.scale = [x, y, z];
        self
    }

    pub fn uniform_scale(self, s: f32) -> Self {
        self.scale(s, s, s)
    }

    pub fn left_rotation(mut self, q: [f32; 4]) -> Self {
        self.left_rotation = q;
        self
    }

    pub fn right_rotation(mut self, q: [f32; 4]) -> Self {
        self.right_rotation = q;
        self
    }

    /// Entries for a new keyframe. `TRANSFORMATION_START_DELTA_TICKS` must be
    /// sent even when unchanged: receiving it restarts the client's
    /// interpolation clock.
    pub fn entries(&self, interpolation_ticks: u16) -> Vec<EntityMetadataEntry> {
        vec![
            entry(
                display_index::TRANSFORMATION_START_DELTA_TICKS,
                Value::Int(0),
            ),
            entry(
                display_index::TRANSFORMATION_DURATION,
                Value::Int(i32::from(interpolation_ticks)),
            ),
            entry(display_index::TRANSLATION, Value::Vector3(self.translation)),
            entry(display_index::SCALE, Value::Vector3(self.scale)),
            entry(
                display_index::LEFT_ROTATION,
                Value::Quaternion(self.left_rotation),
            ),
            entry(
                display_index::RIGHT_ROTATION,
                Value::Quaternion(self.right_rotation),
            ),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Billboard {
    #[default]
    Fixed = 0,
    Vertical = 1,
    Horizontal = 2,
    Center = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ItemDisplayContext {
    #[default]
    None = 0,
    ThirdPersonLeftHand = 1,
    ThirdPersonRightHand = 2,
    FirstPersonLeftHand = 3,
    FirstPersonRightHand = 4,
    Head = 5,
    Gui = 6,
    Ground = 7,
    Fixed = 8,
}

pub fn pack_brightness(brightness: Option<(u8, u8)>) -> i32 {
    brightness.map_or(-1, |(block, sky)| {
        (i32::from(block.min(15)) << 4) | (i32::from(sky.min(15)) << 20)
    })
}

/// Vanilla clamps `POS_ROT_DURATION` to 0..=59.
pub const MAX_TELEPORT_TICKS: u8 = 59;

pub fn entry(index: u8, value: Value) -> EntityMetadataEntry {
    EntityMetadataEntry { index, value }
}

#[cfg(test)]
mod tests {
    use voidmc_codec::{Decode, Encode};

    use super::super::SetEntityData;
    use super::*;

    #[test]
    fn transform_keyframe_wire_uses_vector_and_quaternion_serializers_and_resets_clock() {
        let packet = SetEntityData {
            entity_id: 300,
            entries: DisplayTransform::default().entries(2),
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        let mut expected = vec![0xac, 2, 8, 1, 0, 9, 1, 2, 11, 39];
        for x in [0.0f32; 3] {
            expected.extend(x.to_be_bytes());
        }
        expected.extend([12, 39]);
        for x in [1.0f32; 3] {
            expected.extend(x.to_be_bytes());
        }
        for index in [13, 14] {
            expected.extend([index, 40]);
            for x in [0.0f32, 0.0, 0.0, 1.0] {
                expected.extend(x.to_be_bytes());
            }
        }
        expected.push(0xff);
        assert_eq!(bytes, expected);
        let mut input = bytes.as_slice();
        assert_eq!(SetEntityData::decode(&mut input).unwrap(), packet);
        assert!(input.is_empty());
    }

    #[test]
    fn brightness_packs_block_and_sky_light() {
        assert_eq!(pack_brightness(None), -1);
        assert_eq!(pack_brightness(Some((15, 15))), 0x00f0_00f0);
        assert_eq!(pack_brightness(Some((99, 0))), 0xf0);
        assert_eq!(pack_brightness(Some((0, 1))), 1 << 20);
    }

    #[test]
    fn enum_ids_match_vanilla_ordinals() {
        assert_eq!(Billboard::Center as u8, 3);
        assert_eq!(ItemDisplayContext::Fixed as u8, 8);
        assert_eq!(ItemDisplayContext::Ground as u8, 7);
    }
}
