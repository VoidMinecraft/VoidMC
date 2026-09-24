use std::fmt;

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use ussr_nbt::endian::RawVec;
use ussr_nbt::owned::{Compound, List, Nbt, Tag};
use uuid::Uuid;
use voidmc_data::Version;
use voidmc_protocol::clientbound::{BlockEntityData, ChunkBlockEntity};
use voidmc_protocol::types::BlockPosition;

pub use voidmc_protocol::BlockEntityKind;

use super::{ChunkData, ChunkIndex, ChunkPos, DimensionId};
use crate::messages::truncate_text;

const VERSION: Version = Version::V26_1_2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DyeColor {
    White,
    Orange,
    Magenta,
    LightBlue,
    Yellow,
    Lime,
    Pink,
    Gray,
    LightGray,
    Cyan,
    Purple,
    Blue,
    Brown,
    Green,
    Red,
    #[default]
    Black,
}

impl DyeColor {
    pub const ALL: [DyeColor; 16] = [
        DyeColor::White,
        DyeColor::Orange,
        DyeColor::Magenta,
        DyeColor::LightBlue,
        DyeColor::Yellow,
        DyeColor::Lime,
        DyeColor::Pink,
        DyeColor::Gray,
        DyeColor::LightGray,
        DyeColor::Cyan,
        DyeColor::Purple,
        DyeColor::Blue,
        DyeColor::Brown,
        DyeColor::Green,
        DyeColor::Red,
        DyeColor::Black,
    ];

    pub fn name(self) -> &'static str {
        match self {
            DyeColor::White => "white",
            DyeColor::Orange => "orange",
            DyeColor::Magenta => "magenta",
            DyeColor::LightBlue => "light_blue",
            DyeColor::Yellow => "yellow",
            DyeColor::Lime => "lime",
            DyeColor::Pink => "pink",
            DyeColor::Gray => "gray",
            DyeColor::LightGray => "light_gray",
            DyeColor::Cyan => "cyan",
            DyeColor::Purple => "purple",
            DyeColor::Blue => "blue",
            DyeColor::Brown => "brown",
            DyeColor::Green => "green",
            DyeColor::Red => "red",
            DyeColor::Black => "black",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|color| color.name() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SignSide {
    pub lines: [String; 4],
    pub color: DyeColor,
    pub glowing: bool,
}

impl SignSide {
    pub fn lines(lines: [impl Into<String>; 4]) -> Self {
        Self {
            lines: lines.map(Into::into),
            ..Self::default()
        }
    }

    pub fn color(mut self, color: DyeColor) -> Self {
        self.color = color;
        self
    }

    pub fn glowing(mut self) -> Self {
        self.glowing = true;
        self
    }

    fn to_nbt(&self) -> Compound {
        Compound {
            tags: vec![
                (
                    "messages".into(),
                    Tag::List(List::String(
                        self.lines
                            .iter()
                            .map(|line| truncate_text(line).into())
                            .collect(),
                    )),
                ),
                ("color".into(), Tag::String(self.color.name().into())),
                ("has_glowing_text".into(), Tag::Byte(self.glowing as u8)),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sign {
    pub front: SignSide,
    pub back: SignSide,
    pub waxed: bool,
    pub hanging: bool,
}

impl Sign {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lines(lines: [impl Into<String>; 4]) -> Self {
        Self::new().front(SignSide::lines(lines))
    }

    pub fn front(mut self, side: SignSide) -> Self {
        self.front = side;
        self
    }

    pub fn back(mut self, side: SignSide) -> Self {
        self.back = side;
        self
    }

    pub fn waxed(mut self) -> Self {
        self.waxed = true;
        self
    }

    /// Targets `minecraft:hanging_sign` blocks instead of `minecraft:sign` ones.
    pub fn hanging(mut self) -> Self {
        self.hanging = true;
        self
    }
}

impl From<Sign> for BlockEntity {
    fn from(sign: Sign) -> Self {
        let kind = if sign.hanging {
            BlockEntityKind::hanging_sign()
        } else {
            BlockEntityKind::sign()
        };
        BlockEntity::raw(
            kind,
            Compound {
                tags: vec![
                    ("front_text".into(), Tag::Compound(sign.front.to_nbt())),
                    ("back_text".into(), Tag::Compound(sign.back.to_nbt())),
                    ("is_waxed".into(), Tag::Byte(sign.waxed as u8)),
                ],
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Skull {
    pub name: Option<String>,
    pub id: Option<Uuid>,
    pub texture: Option<String>,
}

impl Skull {
    /// A head resolved by the client from the player's name.
    pub fn player(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::default()
        }
    }

    /// A head with a fixed skin: `texture` is the base64 `textures` property value.
    pub fn texture(texture: impl Into<String>) -> Self {
        Self {
            texture: Some(texture.into()),
            ..Self::default()
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn id(mut self, id: Uuid) -> Self {
        self.id = Some(id);
        self
    }

    fn profile(&self) -> Option<Tag> {
        match (&self.name, self.id, &self.texture) {
            (None, None, None) => None,
            (Some(name), None, None) => Some(Tag::String(name.as_str().into())),
            (name, id, texture) => {
                let mut tags = Vec::new();
                if let Some(name) = name {
                    tags.push(("name".into(), Tag::String(name.as_str().into())));
                }
                if let Some(id) = id {
                    tags.push((
                        "id".into(),
                        Tag::IntArray(RawVec::from_vec(uuid_ints(id).to_vec())),
                    ));
                }
                if let Some(texture) = texture {
                    let property = Compound {
                        tags: vec![
                            ("name".into(), Tag::String("textures".into())),
                            ("value".into(), Tag::String(texture.as_str().into())),
                        ],
                    };
                    tags.push((
                        "properties".into(),
                        Tag::List(List::Compound(vec![property])),
                    ));
                }
                Some(Tag::Compound(Compound { tags }))
            }
        }
    }
}

fn uuid_ints(id: Uuid) -> [i32; 4] {
    let bytes = id.as_bytes();
    let int = |i: usize| i32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    [int(0), int(4), int(8), int(12)]
}

impl From<Skull> for BlockEntity {
    fn from(skull: Skull) -> Self {
        let tags = skull
            .profile()
            .map(|profile| vec![("profile".into(), profile)])
            .unwrap_or_default();
        BlockEntity::raw(BlockEntityKind::skull(), Compound { tags })
    }
}

/// An entry of the `minecraft:banner_pattern` registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BannerPattern(&'static str);

impl BannerPattern {
    pub fn named(name: &str) -> Option<Self> {
        let full;
        let name = if name.contains(':') {
            name
        } else {
            full = format!("minecraft:{name}");
            full.as_str()
        };
        voidmc_data::registry(VERSION, "minecraft:banner_pattern")?
            .iter()
            .find(|(id, _)| *id == name)
            .map(|(id, _)| Self(id))
    }

    pub fn name(self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BannerLayer {
    pub pattern: BannerPattern,
    pub color: DyeColor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub base: DyeColor,
    pub layers: Vec<BannerLayer>,
}

impl Banner {
    pub fn new(base: DyeColor) -> Self {
        Self {
            base,
            layers: Vec::new(),
        }
    }

    pub fn layer(mut self, pattern: BannerPattern, color: DyeColor) -> Self {
        self.layers.push(BannerLayer { pattern, color });
        self
    }

    /// The standing banner block state of `base`; the base colour lives in the block, not the NBT.
    pub fn block_state(&self) -> i32 {
        let item = format!("minecraft:{}_banner", self.base.name());
        voidmc_data::item_id(VERSION, &item)
            .and_then(|id| voidmc_data::item_default_block_state(VERSION, id))
            .expect("every dye colour has a banner block")
    }
}

impl From<Banner> for BlockEntity {
    fn from(banner: Banner) -> Self {
        let layers: Vec<Compound> = banner
            .layers
            .iter()
            .map(|layer| Compound {
                tags: vec![
                    ("pattern".into(), Tag::String(layer.pattern.name().into())),
                    ("color".into(), Tag::String(layer.color.name().into())),
                ],
            })
            .collect();
        let patterns = if layers.is_empty() {
            List::Empty
        } else {
            List::Compound(layers)
        };
        BlockEntity::raw(
            BlockEntityKind::banner(),
            Compound {
                tags: vec![("patterns".into(), Tag::List(patterns))],
            },
        )
    }
}

/// A block entity: its registry kind plus the NBT the client and the disk see
/// (without `id`/`x`/`y`/`z`, which come from the position it is stored at).
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntity {
    kind: BlockEntityKind,
    data: Compound,
}

impl BlockEntity {
    pub fn raw(kind: BlockEntityKind, data: Compound) -> Self {
        Self { kind, data }
    }

    pub fn kind(&self) -> BlockEntityKind {
        self.kind
    }

    pub fn data(&self) -> &Compound {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut Compound {
        &mut self.data
    }

    pub fn to_nbt(&self) -> Nbt {
        Nbt {
            name: "".into(),
            compound: self.data.clone(),
        }
    }

    pub fn chunk_entry(&self, position: BlockPosition) -> ChunkBlockEntity {
        ChunkBlockEntity {
            local_x: position.x.rem_euclid(16) as u8,
            local_z: position.z.rem_euclid(16) as u8,
            y: position.y,
            kind: self.kind,
            data: (!self.data.tags.is_empty()).then(|| self.to_nbt()),
        }
    }

    pub fn packet(&self, position: BlockPosition) -> BlockEntityData {
        BlockEntityData {
            position,
            kind: self.kind,
            data: self.to_nbt(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEntityError {
    ChunkNotLoaded,
    OutsideWorld,
    /// The block at that position does not host `kind`.
    WrongBlock {
        block_state: i32,
        kind: BlockEntityKind,
    },
}

impl fmt::Display for BlockEntityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockEntityError::ChunkNotLoaded => f.write_str("chunk is not loaded"),
            BlockEntityError::OutsideWorld => f.write_str("position is outside the world"),
            BlockEntityError::WrongBlock { block_state, kind } => {
                write!(f, "block state {block_state} does not host {}", kind.name())
            }
        }
    }
}

impl std::error::Error for BlockEntityError {}

pub fn chunk_of(position: BlockPosition) -> ChunkPos {
    ChunkPos::new(position.x.div_euclid(16), position.z.div_euclid(16))
}

fn chunk_entity(
    world: &World,
    dimension: DimensionId,
    position: BlockPosition,
) -> Option<(Entity, ChunkPos)> {
    let chunk = chunk_of(position);
    let entity = world.resource::<ChunkIndex>().0.get(&(dimension, chunk))?;
    Some((*entity, chunk))
}

pub fn block_entity_at(
    world: &World,
    dimension: DimensionId,
    position: BlockPosition,
) -> Option<&BlockEntity> {
    let (entity, chunk) = chunk_entity(world, dimension, position)?;
    world
        .get::<ChunkData>(entity)?
        .block_entity(chunk, position)
}

/// Stores `block_entity` at `position`, replacing any previous one. Players
/// seeing the chunk receive a Block Entity Data packet in the next `PostUpdate`.
pub fn set_block_entity(
    world: &mut World,
    dimension: DimensionId,
    position: BlockPosition,
    block_entity: impl Into<BlockEntity>,
) -> Result<Option<BlockEntity>, BlockEntityError> {
    let (entity, chunk) =
        chunk_entity(world, dimension, position).ok_or(BlockEntityError::ChunkNotLoaded)?;
    world
        .get_mut::<ChunkData>(entity)
        .ok_or(BlockEntityError::ChunkNotLoaded)?
        .set_block_entity(chunk, position, block_entity)
}

pub fn remove_block_entity(
    world: &mut World,
    dimension: DimensionId,
    position: BlockPosition,
) -> Option<BlockEntity> {
    let (entity, chunk) = chunk_entity(world, dimension, position)?;
    world
        .get_mut::<ChunkData>(entity)?
        .remove_block_entity(chunk, position)
}

/// The same operations as [`set_block_entity`] & co, from a system.
#[derive(SystemParam)]
pub struct BlockEntities<'w, 's> {
    index: Res<'w, ChunkIndex>,
    chunks: Query<'w, 's, &'static mut ChunkData>,
}

impl BlockEntities<'_, '_> {
    fn chunk(&self, dimension: DimensionId, position: BlockPosition) -> Option<(Entity, ChunkPos)> {
        let chunk = chunk_of(position);
        let entity = self.index.0.get(&(dimension, chunk))?;
        Some((*entity, chunk))
    }

    pub fn get(&self, dimension: DimensionId, position: BlockPosition) -> Option<&BlockEntity> {
        let (entity, chunk) = self.chunk(dimension, position)?;
        self.chunks.get(entity).ok()?.block_entity(chunk, position)
    }

    pub fn set(
        &mut self,
        dimension: DimensionId,
        position: BlockPosition,
        block_entity: impl Into<BlockEntity>,
    ) -> Result<Option<BlockEntity>, BlockEntityError> {
        let (entity, chunk) = self
            .chunk(dimension, position)
            .ok_or(BlockEntityError::ChunkNotLoaded)?;
        self.chunks
            .get_mut(entity)
            .map_err(|_| BlockEntityError::ChunkNotLoaded)?
            .set_block_entity(chunk, position, block_entity)
    }

    pub fn remove(
        &mut self,
        dimension: DimensionId,
        position: BlockPosition,
    ) -> Option<BlockEntity> {
        let (entity, chunk) = self.chunk(dimension, position)?;
        self.chunks
            .get_mut(entity)
            .ok()?
            .remove_block_entity(chunk, position)
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, PostUpdate};
    use flume::Receiver;
    use voidmc_codec::Encode;
    use voidmc_data::v26_1_2::blocks;
    use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady};
    use crate::network::{NetworkChannels, OutgoingPacket};
    use crate::plugins::block_entity::BlockEntityPlugin;
    use crate::schedule::VoidSystems;
    use crate::world::{ChunkDimension, ChunkDirty, ChunkPosition};

    // Network NBT produced by an independent writer from the Paper 26.1.2 codecs
    // (SignText.DIRECT_CODEC, ResolvableProfile.CODEC, BannerPatternLayers.CODEC).
    const SIGN: &[u8] = &[
        0x0a, 0x0a, 0x00, 0x0a, 0x66, 0x72, 0x6f, 0x6e, 0x74, 0x5f, 0x74, 0x65, 0x78, 0x74, 0x09,
        0x00, 0x08, 0x6d, 0x65, 0x73, 0x73, 0x61, 0x67, 0x65, 0x73, 0x08, 0x00, 0x00, 0x00, 0x04,
        0x00, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x00, 0x04, 0x76, 0x6f, 0x69, 0x64, 0x00, 0x00,
        0x00, 0x00, 0x08, 0x00, 0x05, 0x63, 0x6f, 0x6c, 0x6f, 0x72, 0x00, 0x03, 0x72, 0x65, 0x64,
        0x01, 0x00, 0x10, 0x68, 0x61, 0x73, 0x5f, 0x67, 0x6c, 0x6f, 0x77, 0x69, 0x6e, 0x67, 0x5f,
        0x74, 0x65, 0x78, 0x74, 0x01, 0x00, 0x0a, 0x00, 0x09, 0x62, 0x61, 0x63, 0x6b, 0x5f, 0x74,
        0x65, 0x78, 0x74, 0x09, 0x00, 0x08, 0x6d, 0x65, 0x73, 0x73, 0x61, 0x67, 0x65, 0x73, 0x08,
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x05,
        0x63, 0x6f, 0x6c, 0x6f, 0x72, 0x00, 0x05, 0x62, 0x6c, 0x61, 0x63, 0x6b, 0x01, 0x00, 0x10,
        0x68, 0x61, 0x73, 0x5f, 0x67, 0x6c, 0x6f, 0x77, 0x69, 0x6e, 0x67, 0x5f, 0x74, 0x65, 0x78,
        0x74, 0x00, 0x00, 0x01, 0x00, 0x08, 0x69, 0x73, 0x5f, 0x77, 0x61, 0x78, 0x65, 0x64, 0x01,
        0x00,
    ];
    const SKULL_NAME: &[u8] = &[
        0x0a, 0x08, 0x00, 0x07, 0x70, 0x72, 0x6f, 0x66, 0x69, 0x6c, 0x65, 0x00, 0x05, 0x4e, 0x6f,
        0x74, 0x63, 0x68, 0x00,
    ];
    const SKULL_FULL: &[u8] = &[
        0x0a, 0x0a, 0x00, 0x07, 0x70, 0x72, 0x6f, 0x66, 0x69, 0x6c, 0x65, 0x08, 0x00, 0x04, 0x6e,
        0x61, 0x6d, 0x65, 0x00, 0x05, 0x4e, 0x6f, 0x74, 0x63, 0x68, 0x0b, 0x00, 0x02, 0x69, 0x64,
        0x00, 0x00, 0x00, 0x04, 0x06, 0x9a, 0x79, 0xf4, 0x44, 0xe9, 0x4a, 0xc8, 0x9f, 0xc5, 0x4d,
        0x0e, 0x04, 0xff, 0x4d, 0xfa, 0x09, 0x00, 0x0a, 0x70, 0x72, 0x6f, 0x70, 0x65, 0x72, 0x74,
        0x69, 0x65, 0x73, 0x0a, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00, 0x04, 0x6e, 0x61, 0x6d, 0x65,
        0x00, 0x08, 0x74, 0x65, 0x78, 0x74, 0x75, 0x72, 0x65, 0x73, 0x08, 0x00, 0x05, 0x76, 0x61,
        0x6c, 0x75, 0x65, 0x00, 0x0c, 0x64, 0x47, 0x56, 0x34, 0x64, 0x48, 0x56, 0x79, 0x5a, 0x51,
        0x3d, 0x3d, 0x00, 0x00, 0x00,
    ];
    const SKULL_TEXTURE: &[u8] = &[
        0x0a, 0x0a, 0x00, 0x07, 0x70, 0x72, 0x6f, 0x66, 0x69, 0x6c, 0x65, 0x09, 0x00, 0x0a, 0x70,
        0x72, 0x6f, 0x70, 0x65, 0x72, 0x74, 0x69, 0x65, 0x73, 0x0a, 0x00, 0x00, 0x00, 0x01, 0x08,
        0x00, 0x04, 0x6e, 0x61, 0x6d, 0x65, 0x00, 0x08, 0x74, 0x65, 0x78, 0x74, 0x75, 0x72, 0x65,
        0x73, 0x08, 0x00, 0x05, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x00, 0x0c, 0x64, 0x47, 0x56, 0x34,
        0x64, 0x48, 0x56, 0x79, 0x5a, 0x51, 0x3d, 0x3d, 0x00, 0x00, 0x00,
    ];
    const BANNER: &[u8] = &[
        0x0a, 0x09, 0x00, 0x08, 0x70, 0x61, 0x74, 0x74, 0x65, 0x72, 0x6e, 0x73, 0x0a, 0x00, 0x00,
        0x00, 0x02, 0x08, 0x00, 0x07, 0x70, 0x61, 0x74, 0x74, 0x65, 0x72, 0x6e, 0x00, 0x14, 0x6d,
        0x69, 0x6e, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x3a, 0x73, 0x74, 0x72, 0x69, 0x70, 0x65,
        0x5f, 0x74, 0x6f, 0x70, 0x08, 0x00, 0x05, 0x63, 0x6f, 0x6c, 0x6f, 0x72, 0x00, 0x03, 0x72,
        0x65, 0x64, 0x00, 0x08, 0x00, 0x07, 0x70, 0x61, 0x74, 0x74, 0x65, 0x72, 0x6e, 0x00, 0x10,
        0x6d, 0x69, 0x6e, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x3a, 0x62, 0x6f, 0x72, 0x64, 0x65,
        0x72, 0x08, 0x00, 0x05, 0x63, 0x6f, 0x6c, 0x6f, 0x72, 0x00, 0x05, 0x77, 0x68, 0x69, 0x74,
        0x65, 0x00, 0x00,
    ];
    const BANNER_EMPTY: &[u8] = &[
        0x0a, 0x09, 0x00, 0x08, 0x70, 0x61, 0x74, 0x74, 0x65, 0x72, 0x6e, 0x73, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00,
    ];

    fn network_bytes(block_entity: &BlockEntity) -> Vec<u8> {
        let mut buf = Vec::new();
        block_entity.to_nbt().encode(&mut buf);
        buf
    }

    #[test]
    fn sign_nbt_matches_golden_bytes() {
        let sign = Sign::lines(["Hello", "void", "", ""])
            .front(
                SignSide::lines(["Hello", "void", "", ""])
                    .color(DyeColor::Red)
                    .glowing(),
            )
            .waxed();
        let block_entity: BlockEntity = sign.into();
        assert_eq!(block_entity.kind(), BlockEntityKind::sign());
        assert_eq!(network_bytes(&block_entity), SIGN);
        assert_eq!(
            BlockEntity::from(Sign::new().hanging()).kind(),
            BlockEntityKind::hanging_sign()
        );
    }

    #[test]
    fn oversized_sign_line_round_trips_below_the_nbt_limit() {
        let text = "😀".repeat(11000);
        let sign: BlockEntity = Sign::new()
            .front(SignSide::lines([text.as_str(), "", "", ""]))
            .into();
        crate::messages::assert_guarded(&sign.to_nbt(), &text);
    }

    #[test]
    fn skull_nbt_matches_golden_bytes() {
        let by_name: BlockEntity = Skull::player("Notch").into();
        assert_eq!(by_name.kind(), BlockEntityKind::skull());
        assert_eq!(network_bytes(&by_name), SKULL_NAME);

        let full: BlockEntity = Skull::texture("dGV4dHVyZQ==")
            .name("Notch")
            .id(Uuid::parse_str("069a79f4-44e9-4ac8-9fc5-4d0e04ff4dfa").unwrap())
            .into();
        assert_eq!(network_bytes(&full), SKULL_FULL);

        let texture_only: BlockEntity = Skull::texture("dGV4dHVyZQ==").into();
        assert_eq!(network_bytes(&texture_only), SKULL_TEXTURE);

        let bare: BlockEntity = Skull::default().into();
        assert!(bare.data().tags.is_empty());
    }

    #[test]
    fn banner_nbt_matches_golden_bytes() {
        let banner = Banner::new(DyeColor::Blue)
            .layer(BannerPattern::named("stripe_top").unwrap(), DyeColor::Red)
            .layer(
                BannerPattern::named("minecraft:border").unwrap(),
                DyeColor::White,
            );
        assert_eq!(banner.block_state(), blocks::BLUE_BANNER);
        let block_entity: BlockEntity = banner.into();
        assert_eq!(block_entity.kind(), BlockEntityKind::banner());
        assert_eq!(network_bytes(&block_entity), BANNER);
        assert_eq!(
            network_bytes(&Banner::new(DyeColor::Black).into()),
            BANNER_EMPTY
        );
        assert_eq!(BannerPattern::named("minecraft:not_a_pattern"), None);
    }

    #[test]
    fn dye_colors_round_trip_by_name() {
        for color in DyeColor::ALL {
            assert_eq!(DyeColor::from_name(color.name()), Some(color));
        }
        assert_eq!(DyeColor::from_name("plaid"), None);
    }

    fn chunk_with(position: BlockPosition, block_state: i32) -> ChunkData {
        let mut data = ChunkData::new(
            (0..24).map(|_| ChunkSection::empty()).collect(),
            ChunkHeightmaps::empty(),
            LightData::empty(),
        );
        data.set_block(
            position.x.rem_euclid(16) as u8,
            position.y as i32,
            position.z.rem_euclid(16) as u8,
            block_state,
        )
        .unwrap();
        data
    }

    const POS: BlockPosition = BlockPosition {
        x: -3,
        y: 70,
        z: 21,
    };
    const CHUNK: ChunkPos = ChunkPos { x: -1, z: 1 };

    #[test]
    fn storage_set_get_remove() {
        let mut data = chunk_with(POS, blocks::OAK_SIGN);
        assert_eq!(data.block_entity(CHUNK, POS), None);

        let first = data
            .set_block_entity(CHUNK, POS, Sign::lines(["a", "", "", ""]))
            .unwrap();
        assert_eq!(first, None);
        assert_eq!(
            data.block_entity(CHUNK, POS).unwrap().kind(),
            BlockEntityKind::sign()
        );

        let replaced = data
            .set_block_entity(CHUNK, POS, Sign::lines(["b", "", "", ""]))
            .unwrap();
        assert_eq!(replaced, Some(Sign::lines(["a", "", "", ""]).into()));
        assert_eq!(
            data.block_entities(CHUNK).collect::<Vec<_>>(),
            vec![(POS, &BlockEntity::from(Sign::lines(["b", "", "", ""])))]
        );

        assert_eq!(
            data.remove_block_entity(CHUNK, POS),
            Some(Sign::lines(["b", "", "", ""]).into())
        );
        assert_eq!(data.remove_block_entity(CHUNK, POS), None);
        assert_eq!(data.block_entity(CHUNK, POS), None);
    }

    #[test]
    fn set_rejects_a_block_that_cannot_host_the_kind() {
        let mut data = chunk_with(POS, blocks::STONE);
        assert_eq!(
            data.set_block_entity(CHUNK, POS, Sign::new()),
            Err(BlockEntityError::WrongBlock {
                block_state: blocks::STONE,
                kind: BlockEntityKind::sign()
            })
        );
        let mut sign_block = chunk_with(POS, blocks::OAK_SIGN);
        assert!(matches!(
            sign_block.set_block_entity(CHUNK, POS, Skull::player("x")),
            Err(BlockEntityError::WrongBlock { .. })
        ));
        assert_eq!(
            sign_block.set_block_entity(CHUNK, BlockPosition { y: 400, ..POS }, Sign::new()),
            Err(BlockEntityError::OutsideWorld)
        );
        assert!(!sign_block.has_pending_block_entities());
    }

    #[test]
    fn block_change_drops_a_block_entity_the_new_block_cannot_host() {
        let mut data = chunk_with(POS, blocks::OAK_SIGN);
        data.set_block_entity(CHUNK, POS, Sign::new()).unwrap();
        data.take_pending_block_entities(CHUNK);

        data.set_block(13, 70, 5, blocks::SPRUCE_WALL_SIGN).unwrap();
        assert!(
            data.block_entity(CHUNK, POS).is_some(),
            "sign to sign keeps the text"
        );

        data.set_block(13, 70, 5, blocks::OAK_HANGING_SIGN).unwrap();
        assert_eq!(data.block_entity(CHUNK, POS), None);
        assert!(!data.has_pending_block_entities());

        data.set_block(13, 70, 5, blocks::OAK_SIGN).unwrap();
        data.set_block_entity(CHUNK, POS, Sign::new()).unwrap();
        data.set_block(13, 70, 5, blocks::AIR).unwrap();
        assert_eq!(data.block_entity(CHUNK, POS), None);
        assert!(!data.has_pending_block_entities());
    }

    #[test]
    fn chunk_packet_lists_block_entities() {
        let mut data = chunk_with(POS, blocks::OAK_SIGN);
        assert!(data.to_packet(CHUNK.x, CHUNK.z).block_entities.is_empty());
        data.set_block_entity(CHUNK, POS, Sign::lines(["a", "", "", ""]))
            .unwrap();
        let packet = data.to_packet(CHUNK.x, CHUNK.z);
        let sign: BlockEntity = Sign::lines(["a", "", "", ""]).into();
        assert_eq!(packet.block_entities, vec![sign.chunk_entry(POS)]);
        assert_eq!(
            (
                packet.block_entities[0].local_x,
                packet.block_entities[0].local_z
            ),
            (13, 5)
        );
        assert_eq!(packet.block_entities[0].y, 70);
    }

    #[test]
    fn empty_data_is_omitted_from_chunk_entries() {
        let bare: BlockEntity = Skull::default().into();
        assert_eq!(bare.chunk_entry(POS).data, None);
        let named: BlockEntity = Skull::player("Notch").into();
        assert!(named.chunk_entry(POS).data.is_some());
    }

    #[derive(Resource)]
    struct Outgoing(Receiver<OutgoingPacket>);

    fn app() -> (App, Entity) {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_resource(Outgoing(outgoing_rx))
        .init_resource::<ChunkIndex>()
        .configure_sets(PostUpdate, VoidSystems::ChunkStreaming)
        .add_plugins(BlockEntityPlugin);
        let chunk = app
            .world_mut()
            .spawn((
                ChunkPosition(CHUNK),
                ChunkDimension(DimensionId::Overworld),
                chunk_with(POS, blocks::OAK_SIGN),
            ))
            .id();
        app.world_mut()
            .resource_mut::<ChunkIndex>()
            .0
            .insert((DimensionId::Overworld, CHUNK), chunk);
        let seeing = LoadedChunks([CHUNK].into_iter().collect());
        app.world_mut().spawn((
            ClientId(1),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
            seeing,
        ));
        app.world_mut().spawn((
            ClientId(2),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
            LoadedChunks(Default::default()),
        ));
        (app, chunk)
    }

    fn drain(app: &App) -> Vec<(u32, BlockEntityData)> {
        app.world()
            .resource::<Outgoing>()
            .0
            .drain()
            .map(|out| match out.packet {
                ClientboundPacket::Play(PlayPacket::BlockEntityData(packet)) => {
                    (out.client_id, packet)
                }
                other => panic!("unexpected packet {other:?}"),
            })
            .collect()
    }

    #[test]
    fn changes_reach_only_players_seeing_the_chunk_and_only_once() {
        let (mut app, chunk) = app();
        app.update();
        assert!(drain(&app).is_empty());

        let sign: BlockEntity = Sign::lines(["hi", "", "", ""]).into();
        set_block_entity(app.world_mut(), DimensionId::Overworld, POS, sign.clone()).unwrap();
        assert_eq!(
            block_entity_at(app.world(), DimensionId::Overworld, POS),
            Some(&sign)
        );
        app.update();
        assert_eq!(drain(&app), vec![(1, sign.packet(POS))]);
        assert!(app.world().get::<ChunkDirty>(chunk).is_some());

        app.update();
        assert!(
            drain(&app).is_empty(),
            "unchanged block entities send nothing"
        );

        assert_eq!(
            remove_block_entity(app.world_mut(), DimensionId::Overworld, POS),
            Some(sign)
        );
        app.update();
        let sent = drain(&app);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, 1);
        assert_eq!(sent[0].1.kind, BlockEntityKind::sign());
        assert!(sent[0].1.data.compound.tags.is_empty());
    }

    #[test]
    fn world_api_reports_unloaded_chunks() {
        let (mut app, _) = app();
        let far = BlockPosition {
            x: 500,
            y: 70,
            z: 500,
        };
        assert_eq!(
            set_block_entity(app.world_mut(), DimensionId::Overworld, far, Sign::new()),
            Err(BlockEntityError::ChunkNotLoaded)
        );
        assert_eq!(
            block_entity_at(app.world(), DimensionId::Overworld, far),
            None
        );
        assert_eq!(
            remove_block_entity(app.world_mut(), DimensionId::Overworld, far),
            None
        );
    }

    #[test]
    fn system_param_shares_the_storage() {
        let (mut app, _) = app();
        let mut state = bevy_ecs::system::SystemState::<BlockEntities>::new(app.world_mut());
        let mut block_entities = state.get_mut(app.world_mut());
        assert_eq!(block_entities.get(DimensionId::Overworld, POS), None);
        block_entities
            .set(DimensionId::Overworld, POS, Skull::player("x"))
            .unwrap_err();
        block_entities
            .set(DimensionId::Overworld, POS, Sign::new())
            .unwrap();
        assert_eq!(
            block_entities.get(DimensionId::Overworld, POS),
            Some(&Sign::new().into())
        );
        assert_eq!(
            block_entities.remove(DimensionId::Overworld, POS),
            Some(Sign::new().into())
        );
        app.update();
        assert_eq!(drain(&app).len(), 1);
    }
}
