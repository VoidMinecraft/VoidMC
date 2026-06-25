use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use voidmc_codec::{Decode, Encode};

#[derive(Clone, Debug, PartialEq, Decode, Encode)]
struct RepresentativePacket {
    #[codec(varint32)]
    entity_id: i32,
    username: String,
    x: f64,
    y: f64,
    z: f64,
    on_ground: bool,
    payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
struct GenericPrototypePacket {
    entity_id: i32,
    username: String,
    x: f64,
    y: f64,
    z: f64,
    on_ground: bool,
    payload: Vec<u8>,
}

fn packet() -> RepresentativePacket {
    RepresentativePacket {
        entity_id: 42,
        username: "benchmark_player".to_string(),
        x: 128.25,
        y: 64.0,
        z: -512.75,
        on_ground: true,
        payload: (0..128).map(|i| (i % 251) as u8).collect(),
    }
}

fn generic_packet() -> GenericPrototypePacket {
    let packet = packet();
    GenericPrototypePacket {
        entity_id: packet.entity_id,
        username: packet.username,
        x: packet.x,
        y: packet.y,
        z: packet.z,
        on_ground: packet.on_ground,
        payload: packet.payload,
    }
}

fn encode_generic_fixed(packet: &GenericPrototypePacket, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&packet.entity_id.to_be_bytes());
    buf.extend_from_slice(&(packet.username.len() as u32).to_be_bytes());
    buf.extend_from_slice(packet.username.as_bytes());
    buf.extend_from_slice(&packet.x.to_be_bytes());
    buf.extend_from_slice(&packet.y.to_be_bytes());
    buf.extend_from_slice(&packet.z.to_be_bytes());
    buf.push(u8::from(packet.on_ground));
    buf.extend_from_slice(&(packet.payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(&packet.payload);
}

fn decode_generic_fixed(mut buf: &[u8]) -> Option<GenericPrototypePacket> {
    let entity_id = read_i32(&mut buf)?;
    let username_len = read_u32(&mut buf)? as usize;
    if buf.len() < username_len {
        return None;
    }
    let username = std::str::from_utf8(&buf[..username_len]).ok()?.to_string();
    buf = &buf[username_len..];
    let x = read_f64(&mut buf)?;
    let y = read_f64(&mut buf)?;
    let z = read_f64(&mut buf)?;
    let on_ground = *buf.first()? != 0;
    buf = &buf[1..];
    let payload_len = read_u32(&mut buf)? as usize;
    if buf.len() < payload_len {
        return None;
    }
    let payload = buf[..payload_len].to_vec();

    Some(GenericPrototypePacket {
        entity_id,
        username,
        x,
        y,
        z,
        on_ground,
        payload,
    })
}

fn read_i32(buf: &mut &[u8]) -> Option<i32> {
    let bytes = take::<4>(buf)?;
    Some(i32::from_be_bytes(bytes))
}

fn read_u32(buf: &mut &[u8]) -> Option<u32> {
    let bytes = take::<4>(buf)?;
    Some(u32::from_be_bytes(bytes))
}

fn read_f64(buf: &mut &[u8]) -> Option<f64> {
    let bytes = take::<8>(buf)?;
    Some(f64::from_be_bytes(bytes))
}

fn take<const N: usize>(buf: &mut &[u8]) -> Option<[u8; N]> {
    if buf.len() < N {
        return None;
    }
    let (head, tail) = buf.split_at(N);
    *buf = tail;
    head.try_into().ok()
}

fn codec_comparison(c: &mut Criterion) {
    let protocol_packet = packet();
    let generic_packet = generic_packet();
    let mut protocol_bytes = Vec::new();
    protocol_packet.encode(&mut protocol_bytes);
    let mut generic_bytes = Vec::new();
    encode_generic_fixed(&generic_packet, &mut generic_bytes);

    let mut group = c.benchmark_group("codec_comparison");
    group.throughput(Throughput::Elements(1));

    group.bench_function(
        BenchmarkId::new("encode", "void_codec_protocol_shape"),
        |b| {
            b.iter(|| {
                let mut buf = Vec::with_capacity(protocol_bytes.len());
                black_box(&protocol_packet).encode(&mut buf);
                black_box(buf);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("encode", "generic_fixed_width_prototype"),
        |b| {
            b.iter(|| {
                let mut buf = Vec::with_capacity(generic_bytes.len());
                encode_generic_fixed(black_box(&generic_packet), &mut buf);
                black_box(buf);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("decode", "void_codec_protocol_shape"),
        |b| {
            b.iter(|| {
                let mut slice = black_box(protocol_bytes.as_slice());
                let decoded = RepresentativePacket::decode(&mut slice).unwrap();
                black_box(decoded);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("decode", "generic_fixed_width_prototype"),
        |b| {
            b.iter(|| {
                let decoded = decode_generic_fixed(black_box(generic_bytes.as_slice())).unwrap();
                black_box(decoded);
            });
        },
    );

    group.finish();
}

criterion_group!(benches, codec_comparison);
criterion_main!(benches);
