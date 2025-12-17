use void_codec::{Decode, Encode};

extern crate void_codec_macros;

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct SimplePacket {
    pub value: u8,
}

#[test]
fn test_simple() {
    let packet = SimplePacket { value: 42 };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    assert_eq!(buf, vec![42]);

    let mut slice = buf.as_slice();
    let decoded = SimplePacket::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct MultiFieldPacket {
    pub a: u8,
    pub b: i32,
    pub c: bool,
}

#[test]
fn test_multi_field() {
    let packet = MultiFieldPacket {
        a: 255,
        b: 12345,
        c: true,
    };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = MultiFieldPacket::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct VarI32Packet {
    pub prefix: u8,
    #[codec(vari32)]
    pub value: i32,
}

#[test]
fn test_vari32_field() {
    let packet = VarI32Packet {
        prefix: 1,
        value: 12345,
    };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    assert!(buf.len() > 1, "Should have encoded both fields");

    let mut slice = buf.as_slice();
    let decoded = VarI32Packet::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct Inner {
    pub value: u8,
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct Nested {
    pub inner: Inner,
    pub extra: i32,
}

#[test]
fn test_nested_structs() {
    let packet = Nested {
        inner: Inner { value: 42 },
        extra: 12345,
    };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = Nested::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct Packet1 {
    pub data: u8,
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct Packet2 {
    pub value: i32,
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode)]
#[codec(tagged)]
pub enum StatePacket {
    #[codec(packet_id = 0)]
    First(Packet1),
    #[codec(packet_id = 1)]
    Second(Packet2),
}

#[test]
fn test_tagged_enum_first() {
    let packet = StatePacket::First(Packet1 { data: 42 });

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    assert_eq!(buf[0], 0, "First variant should have ID 0");

    let mut slice = buf.as_slice();
    let decoded = StatePacket::decode(&mut slice).unwrap();

    match decoded {
        StatePacket::First(p) => assert_eq!(p.data, 42),
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_tagged_enum_second() {
    let packet = StatePacket::Second(Packet2 { value: 12345 });

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    assert_eq!(buf[0], 1, "Second variant should have ID 1");

    let mut slice = buf.as_slice();
    let decoded = StatePacket::decode(&mut slice).unwrap();

    match decoded {
        StatePacket::Second(p) => assert_eq!(p.value, 12345),
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_tagged_enum_invalid_id() {
    let buf = vec![255];
    let mut slice = buf.as_slice();
    let result = StatePacket::decode(&mut slice);

    assert!(result.is_err(), "Should fail with invalid packet ID");
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct MixedPacket {
    pub regular: u8,
    #[codec(vari32)]
    pub compact: i32,
    pub regular2: i32,
    #[codec(vari32)]
    pub compact2: i32,
}

#[test]
fn test_mixed_vari32_and_regular() {
    let packet = MixedPacket {
        regular: 255,
        compact: 100,
        regular2: 0x12345678,
        compact2: 50000,
    };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = MixedPacket::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct UnitStruct;

#[test]
fn test_unit_struct() {
    let packet = UnitStruct;

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    assert_eq!(buf.len(), 0, "Unit struct should encode to nothing");

    let mut slice = buf.as_slice();
    let decoded = UnitStruct::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct LoginPacket {
    pub username: u8,
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct PlayPacket {
    pub entity_id: i32,
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode)]
#[codec(tagged)]
pub enum ProtocolPacket {
    #[codec(packet_id = 0x00)]
    Login(LoginPacket),
    #[codec(packet_id = 0x20)]
    Play(PlayPacket),
}

#[test]
fn test_custom_packet_ids() {
    let packet = ProtocolPacket::Login(LoginPacket { username: 42 });

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    assert_eq!(buf[0], 0x00, "Login should use custom ID 0x00");

    let mut slice = buf.as_slice();
    let decoded = ProtocolPacket::decode(&mut slice).unwrap();

    match decoded {
        ProtocolPacket::Login(p) => assert_eq!(p.username, 42),
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_custom_packet_ids_play() {
    let packet = ProtocolPacket::Play(PlayPacket { entity_id: 123 });

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    assert_eq!(buf[0], 0x20, "Play should use custom ID 0x20");

    let mut slice = buf.as_slice();
    let decoded = ProtocolPacket::decode(&mut slice).unwrap();

    match decoded {
        ProtocolPacket::Play(p) => assert_eq!(p.entity_id, 123),
        _ => panic!("Wrong variant"),
    }
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct ChunkData {
    pub chunk_x: i32,
    pub blocks: Vec<u8>,
}

#[test]
fn test_vec_in_struct() {
    let packet = ChunkData {
        chunk_x: 42,
        blocks: vec![1, 2, 3, 4, 5],
    };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = ChunkData::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
    assert_eq!(decoded.chunk_x, 42);
    assert_eq!(decoded.blocks, vec![1, 2, 3, 4, 5]);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct EntityList {
    pub entities: Vec<i32>,
}

#[test]
fn test_vec_empty_in_struct() {
    let packet = EntityList { entities: vec![] };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = EntityList::decode(&mut slice).unwrap();

    assert_eq!(decoded.entities.len(), 0);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct MultipleVecs {
    pub ids: Vec<u8>,
    pub values: Vec<i32>,
    pub flags: Vec<bool>,
}

#[test]
fn test_multiple_vecs_in_struct() {
    let packet = MultipleVecs {
        ids: vec![1, 2, 3],
        values: vec![100, 200, 300],
        flags: vec![true, false, true],
    };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = MultipleVecs::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
}
#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq, Clone, Copy)]
#[repr(u8)]
pub enum SimpleState {
    Idle = 0,
    Running = 1,
    Paused = 2,
}

#[test]
fn test_repr_u8_enum() {
    for state in [SimpleState::Idle, SimpleState::Running, SimpleState::Paused] {
        let mut buf = Vec::new();
        state.encode(&mut buf);

        assert_eq!(buf.len(), 1);

        let mut slice = buf.as_slice();
        let decoded = SimpleState::decode(&mut slice).unwrap();

        assert_eq!(decoded, state);
        assert_eq!(slice.len(), 0);
    }
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq, Clone, Copy)]
#[repr(i32)]
pub enum DetailedStatus {
    Unknown = -1,
    Init = 0,
    Ready = 100,
    Complete = 1000,
}

#[test]
fn test_repr_i32_enum() {
    let status = DetailedStatus::Complete;

    let mut buf = Vec::new();
    status.encode(&mut buf);

    assert_eq!(buf.len(), 4);

    let mut slice = buf.as_slice();
    let decoded = DetailedStatus::decode(&mut slice).unwrap();

    assert_eq!(decoded, status);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq, Clone, Copy)]
#[codec(varint32)]
#[repr(i32)]
pub enum CompressedState {
    Zero = 0,
    Small = 50,
    Medium = 500,
    Large = 50000,
}

#[test]
fn test_repr_i32_enum_with_varint32() {
    let mut buf = Vec::new();
    CompressedState::Medium.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = CompressedState::decode(&mut slice).unwrap();

    assert_eq!(decoded, CompressedState::Medium);
    assert!(buf.len() < 4);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct StatePacketRepr {
    pub state: SimpleState,
    pub counter: u32,
}

#[test]
fn test_struct_with_repr_enum_field() {
    let packet = StatePacketRepr {
        state: SimpleState::Running,
        counter: 12345,
    };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = StatePacketRepr::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
#[codec(varint64)]
#[repr(i64)]
pub enum CompressedLongState {
    Zero = 0,
    Small = 100,
    Medium = 10000,
    Large = 1000000,
}

#[test]
fn test_repr_i64_enum_with_varint64() {
    let mut buf = Vec::new();
    CompressedLongState::Large.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = CompressedLongState::decode(&mut slice).unwrap();

    assert_eq!(decoded, CompressedLongState::Large);
    assert!(buf.len() < 8);
}

#[derive(void_codec_macros::Encode, void_codec_macros::Decode, Debug, PartialEq)]
pub struct PacketWithVarI64 {
    #[codec(varint64)]
    pub long_value: i64,
    pub id: u32,
}

#[test]
fn test_struct_field_with_varint64() {
    let packet = PacketWithVarI64 {
        long_value: 1000000000000i64,
        id: 42,
    };

    let mut buf = Vec::new();
    packet.encode(&mut buf);

    let mut slice = buf.as_slice();
    let decoded = PacketWithVarI64::decode(&mut slice).unwrap();

    assert_eq!(decoded, packet);
    // VarI64 should use fewer bytes than i64's 8 bytes for this value
    assert!(buf.len() < 12);
}
