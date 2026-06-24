use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use voidmc_codec::{Decode, Encode};
use voidmc_protocol::{
    clientbound::{
        Chunk, KeepAlive, ManualPlayPacket, PlayPacket as ClientboundPlayPacket, blocks,
    },
    serverbound::{PlayPacket as ServerboundPlayPacket, SetPlayerPos},
};

fn encode_to_vec<T: Encode>(value: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    value.encode(&mut buf);
    buf
}

fn representative_chunk() -> Chunk {
    Chunk::superflat(
        0,
        0,
        &[
            (1, blocks::GRASS_BLOCK),
            (4, blocks::DIRT),
            (59, blocks::STONE),
        ],
    )
}

fn bench_chunk_packet(c: &mut Criterion) {
    let chunk = representative_chunk();
    let chunk_packet = chunk.to_packet();
    let encoded_chunk_packet = encode_to_vec(&chunk_packet);
    let manual_packet = ManualPlayPacket::ChunkDataAndLight(chunk_packet.clone());
    let encoded_manual_packet = encode_to_vec(&manual_packet);

    let mut group = c.benchmark_group("chunk_packet");
    group.throughput(Throughput::Bytes(encoded_chunk_packet.len() as u64));

    group.bench_function("chunk_to_packet", |b| {
        b.iter(|| black_box(&chunk).to_packet());
    });

    group.bench_with_input(
        BenchmarkId::new("encode_chunk_data_and_light", encoded_chunk_packet.len()),
        &chunk_packet,
        |b, packet| {
            b.iter(|| encode_to_vec(black_box(packet)));
        },
    );

    group.bench_with_input(
        BenchmarkId::new("encode_manual_play_chunk", encoded_manual_packet.len()),
        &manual_packet,
        |b, packet| {
            b.iter(|| encode_to_vec(black_box(packet)));
        },
    );

    group.finish();
}

fn bench_representative_packets(c: &mut Criterion) {
    let keep_alive = ClientboundPlayPacket::KeepAlive(KeepAlive {
        keep_alive_id: 42_424_242,
    });
    let encoded_keep_alive = encode_to_vec(&keep_alive);

    let serverbound_position = ServerboundPlayPacket::SetPlayerPos(SetPlayerPos {
        x: 12.5,
        y: 64.0,
        z: -31.25,
        flags: 0,
    });
    let encoded_position = encode_to_vec(&serverbound_position);

    let mut group = c.benchmark_group("representative_packets");

    group.bench_with_input(
        BenchmarkId::new("encode_clientbound_keep_alive", encoded_keep_alive.len()),
        &keep_alive,
        |b, packet| {
            b.iter(|| encode_to_vec(black_box(packet)));
        },
    );

    group.bench_with_input(
        BenchmarkId::new("encode_serverbound_position", encoded_position.len()),
        &serverbound_position,
        |b, packet| {
            b.iter(|| encode_to_vec(black_box(packet)));
        },
    );

    group.bench_with_input(
        BenchmarkId::new("decode_serverbound_position", encoded_position.len()),
        &encoded_position,
        |b, bytes| {
            b.iter(|| {
                let mut input = black_box(bytes.as_slice());
                ServerboundPlayPacket::decode(&mut input).expect("position packet should decode")
            });
        },
    );

    group.finish();
}

criterion_group!(benches, bench_chunk_packet, bench_representative_packets);
criterion_main!(benches);
