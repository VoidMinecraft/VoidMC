// Layouts pinned against Paper 26.1.2:
// ClientboundBlockEntityDataPacket.STREAM_CODEC = BlockPos, registry(BLOCK_ENTITY_TYPE), TRUSTED_COMPOUND_TAG
// ClientboundLevelChunkPacketData.BlockEntityInfo.write = byte packedXZ, short y, registry id, writeNbt(nullable)

use ussr_nbt::owned::{Nbt, Tag};
use voidmc_codec::{Decode, Encode};
use voidmc_data::{Version, block_entity_type_id};
use voidmc_protocol::BlockEntityKind;
use voidmc_protocol::clientbound::{
    BlockEntityData, Chunk, ChunkBlockEntity, ChunkDataAndLight, ChunkHeightmaps, LightData,
};
use voidmc_protocol::types::BlockPosition;

fn waxed_nbt() -> Nbt {
    Nbt {
        name: "".into(),
        compound: vec![("is_waxed".into(), Tag::Byte(1))].into(),
    }
}

const WAXED_NETWORK_NBT: &[u8] = &[
    0x0A, 0x01, 0x00, 0x08, b'i', b's', b'_', b'w', b'a', b'x', b'e', b'd', 0x01, 0x00,
];

#[test]
fn block_entity_data_matches_paper_layout() {
    let packet = BlockEntityData {
        position: BlockPosition { x: 1, y: 64, z: -2 },
        kind: BlockEntityKind::sign(),
        data: waxed_nbt(),
    };
    let mut buf = Vec::new();
    packet.encode(&mut buf);

    let packed: i64 = (1i64 << 38) | ((-2i64 & 0x3FFFFFF) << 12) | 64;
    let mut expected = packed.to_be_bytes().to_vec();
    expected.push(block_entity_type_id(Version::V26_1_2, "minecraft:sign").unwrap() as u8);
    expected.extend_from_slice(WAXED_NETWORK_NBT);
    assert_eq!(buf, expected);

    let decoded = BlockEntityData::decode(&mut buf.as_slice()).unwrap();
    assert_eq!(decoded, packet);
}

#[test]
fn chunk_block_entity_entry_matches_paper_layout() {
    let entry = ChunkBlockEntity {
        local_x: 3,
        local_z: 12,
        y: -60,
        kind: BlockEntityKind::skull(),
        data: Some(waxed_nbt()),
    };
    let mut buf = Vec::new();
    entry.encode(&mut buf);

    let mut expected = vec![(3u8 << 4) | 12];
    expected.extend_from_slice(&(-60i16).to_be_bytes());
    expected.push(block_entity_type_id(Version::V26_1_2, "minecraft:skull").unwrap() as u8);
    expected.extend_from_slice(WAXED_NETWORK_NBT);
    assert_eq!(buf, expected);
    assert_eq!(
        ChunkBlockEntity::decode(&mut buf.as_slice()).unwrap(),
        entry
    );
}

#[test]
fn chunk_block_entity_without_data_is_a_bare_tag_end() {
    let entry = ChunkBlockEntity {
        local_x: 15,
        local_z: 0,
        y: 70,
        kind: BlockEntityKind::chest(),
        data: None,
    };
    let mut buf = Vec::new();
    entry.encode(&mut buf);
    assert_eq!(
        buf,
        vec![
            0xF0,
            0x00,
            70,
            block_entity_type_id(Version::V26_1_2, "minecraft:chest").unwrap() as u8,
            0x00
        ]
    );
    assert_eq!(
        ChunkBlockEntity::decode(&mut buf.as_slice()).unwrap(),
        entry
    );
}

#[test]
fn chunk_packet_carries_its_block_entities_between_data_and_light() {
    let chunk = Chunk::empty(0, 0);
    let mut packet: ChunkDataAndLight = chunk.to_packet();
    let mut without = Vec::new();
    packet.encode(&mut without);

    packet.block_entities.push(ChunkBlockEntity {
        local_x: 1,
        local_z: 2,
        y: 5,
        kind: BlockEntityKind::banner(),
        data: None,
    });
    let mut with = Vec::new();
    packet.encode(&mut with);

    let mut prefix = 8 + 1;
    prefix += 1 + 1 + ChunkHeightmaps::empty().motion_blocking.len() * 8;
    let data_len = packet.data.len();
    prefix += varint_len(data_len as i32) + data_len;
    assert_eq!(without[prefix], 0x00);
    assert_eq!(with[prefix], 0x01);
    assert_eq!(
        &with[prefix + 1..prefix + 6],
        &[0x12, 0x00, 0x05, BlockEntityKind::banner().id() as u8, 0x00]
    );
    assert_eq!(&with[prefix + 6..], &without[prefix + 1..]);
    assert_eq!(with.len(), without.len() + 5);
    let _ = LightData::empty();
}

fn varint_len(value: i32) -> usize {
    let mut v = value as u32;
    let mut n = 1;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}
