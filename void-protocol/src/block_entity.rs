use std::sync::OnceLock;

use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI32};
use voidmc_data::Version;

const VERSION: Version = Version::V26_1_2;

/// An entry of the `minecraft:block_entity_type` registry. Only resolvable
/// through the registry data, so an unknown name or id cannot be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockEntityKind {
    id: i32,
    name: &'static str,
}

impl BlockEntityKind {
    pub fn from_name(name: &str) -> Option<Self> {
        let id = voidmc_data::block_entity_type_id(VERSION, name)?;
        let name = voidmc_data::block_entity_type_name(VERSION, id)?;
        Some(Self { id, name })
    }

    pub fn from_id(id: i32) -> Option<Self> {
        let name = voidmc_data::block_entity_type_name(VERSION, id)?;
        Some(Self { id, name })
    }

    /// The kind hosted by a block state, or `None` when the block carries no block entity.
    pub fn for_block_state(block_state_id: i32) -> Option<Self> {
        voidmc_data::block_entity_type_for_state(VERSION, block_state_id).and_then(Self::from_name)
    }

    pub fn hosted_by(self, block_state_id: i32) -> bool {
        Self::for_block_state(block_state_id) == Some(self)
    }

    pub fn id(self) -> i32 {
        self.id
    }

    pub fn name(self) -> &'static str {
        self.name
    }
}

fn resolve(cell: &OnceLock<BlockEntityKind>, name: &str) -> BlockEntityKind {
    *cell.get_or_init(|| {
        BlockEntityKind::from_name(name)
            .unwrap_or_else(|| panic!("block entity type {name} is not in the registry"))
    })
}

macro_rules! vanilla_kind {
    ($fn:ident, $name:literal) => {
        pub fn $fn() -> BlockEntityKind {
            static KIND: OnceLock<BlockEntityKind> = OnceLock::new();
            resolve(&KIND, $name)
        }
    };
}

impl BlockEntityKind {
    vanilla_kind!(sign, "minecraft:sign");
    vanilla_kind!(hanging_sign, "minecraft:hanging_sign");
    vanilla_kind!(skull, "minecraft:skull");
    vanilla_kind!(banner, "minecraft:banner");
    vanilla_kind!(chest, "minecraft:chest");
}

impl Encode for BlockEntityKind {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.id).encode(buf);
    }
}

impl Decode for BlockEntityKind {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let id = decoder.decode::<VarI32>()?.0;
        Self::from_id(id).ok_or(DecodeError::InvalidLength)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_resolve_from_the_registry() {
        assert_eq!(BlockEntityKind::sign().name(), "minecraft:sign");
        assert_eq!(
            BlockEntityKind::from_id(BlockEntityKind::skull().id()),
            Some(BlockEntityKind::skull())
        );
        assert_eq!(BlockEntityKind::from_name("minecraft:not_real"), None);
        assert_eq!(BlockEntityKind::from_id(-1), None);
    }

    #[test]
    fn kind_follows_the_block_state() {
        use voidmc_data::v26_1_2::blocks;
        assert_eq!(
            BlockEntityKind::for_block_state(blocks::OAK_SIGN),
            Some(BlockEntityKind::sign())
        );
        assert!(BlockEntityKind::sign().hosted_by(blocks::SPRUCE_WALL_SIGN));
        assert!(!BlockEntityKind::sign().hosted_by(blocks::OAK_HANGING_SIGN));
        assert!(BlockEntityKind::hanging_sign().hosted_by(blocks::OAK_HANGING_SIGN));
        assert!(BlockEntityKind::skull().hosted_by(blocks::PLAYER_WALL_HEAD));
        assert!(BlockEntityKind::banner().hosted_by(blocks::LIME_BANNER));
        assert!(BlockEntityKind::chest().hosted_by(blocks::CHEST));
        assert_eq!(BlockEntityKind::for_block_state(blocks::STONE), None);
    }

    #[test]
    fn kind_is_a_varint_of_the_registry_id() {
        let mut buf = Vec::new();
        BlockEntityKind::banner().encode(&mut buf);
        assert_eq!(buf, vec![BlockEntityKind::banner().id() as u8]);
        let decoded = BlockEntityKind::decode(&mut buf.as_slice()).unwrap();
        assert_eq!(decoded, BlockEntityKind::banner());
        assert!(BlockEntityKind::decode(&mut [0x7f_u8].as_slice()).is_err());
    }
}
