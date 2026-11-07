use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use ussr_nbt::owned::{Compound, List, Nbt, Tag};
use voidmc_codec::{Decode, DecodeLimits, Encode, VarI32, VarI64};

#[derive(Clone, Debug, PartialEq, Decode, Encode)]
struct MovePlayerPosRot {
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    flags: u8,
}

fn text(value: &str, extra: Vec<(&str, Tag)>) -> Compound {
    let mut tags = vec![("text".into(), Tag::String(value.into()))];
    tags.extend(extra.into_iter().map(|(name, tag)| (name.into(), tag)));
    Compound { tags }
}

fn chat() -> Nbt {
    let hover = text(
        "Click to message",
        vec![("color", Tag::String("gray".into()))],
    );
    let children = vec![
        text(
            "[Server] ",
            vec![
                ("color", Tag::String("gold".into())),
                ("bold", Tag::Byte(1)),
            ],
        ),
        text(
            "Steve",
            vec![
                ("color", Tag::String("aqua".into())),
                (
                    "hover_event",
                    Tag::Compound(Compound {
                        tags: vec![
                            ("action".into(), Tag::String("show_text".into())),
                            ("value".into(), Tag::Compound(hover)),
                        ],
                    }),
                ),
            ],
        ),
        text(
            ": the race starts in ten seconds, line up at the gate",
            vec![],
        ),
        text(" [rules]", vec![("underlined", Tag::Byte(1))]),
    ];
    let root = text("", vec![("extra", Tag::List(List::Compound(children)))]);
    Nbt {
        name: String::new().into(),
        compound: root,
    }
}

fn encoded<T: Encode>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    value.encode(&mut bytes);
    bytes
}

fn decode<T: Decode>(bytes: &[u8]) -> T {
    match DecodeLimits::default().decode_exact::<T>(bytes) {
        Ok(value) => value,
        Err(error) => panic!("bench input does not decode: {error}"),
    }
}

fn decode_paths(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");

    let varints: Vec<Vec<u8>> = [0, 300, 70_000, -1]
        .into_iter()
        .map(|n| encoded(&VarI32(n)))
        .collect();
    group.bench_function("varint", |b| {
        b.iter(|| {
            for bytes in &varints {
                black_box(decode::<VarI32>(black_box(bytes)));
            }
        })
    });

    let varlongs: Vec<Vec<u8>> = [0, 300, 1 << 40, -1]
        .into_iter()
        .map(|n| encoded(&VarI64(n)))
        .collect();
    group.bench_function("varlong", |b| {
        b.iter(|| {
            for bytes in &varlongs {
                black_box(decode::<VarI64>(black_box(bytes)));
            }
        })
    });

    let movement = encoded(&MovePlayerPosRot {
        x: 128.25,
        y: 64.0,
        z: -512.75,
        yaw: 90.5,
        pitch: -12.0,
        flags: 1,
    });
    group.bench_function("move_player_pos_rot", |b| {
        b.iter(|| black_box(decode::<MovePlayerPosRot>(black_box(&movement))))
    });

    group.bench_function("move_player_pos_rot_scoped", |b| {
        b.iter(|| {
            black_box(
                DecodeLimits::default()
                    .with_decoder(black_box(&movement), |d| d.decode::<MovePlayerPosRot>()),
            )
        })
    });

    let chat = encoded(&chat());
    group.bench_function("nbt_chat", |b| {
        b.iter(|| black_box(decode::<Nbt>(black_box(&chat))))
    });

    group.finish();
}

criterion_group!(benches, decode_paths);
criterion_main!(benches);
