use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use ussr_nbt::owned::{Compound, List, Nbt, Tag};
use voidmc_codec::Encode;

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
        text(
            " [rules]",
            vec![
                ("underlined", Tag::Byte(1)),
                (
                    "click_event",
                    Tag::Compound(Compound {
                        tags: vec![
                            ("action".into(), Tag::String("open_url".into())),
                            (
                                "url".into(),
                                Tag::String("https://example.com/rules".into()),
                            ),
                        ],
                    }),
                ),
            ],
        ),
    ];
    Nbt {
        name: "".into(),
        compound: text("", vec![("extra", Tag::List(List::Compound(children)))]),
    }
}

fn bench_nbt_encode(c: &mut Criterion) {
    let nbt = chat();
    let mut buf = Vec::with_capacity(512);
    c.bench_function("nbt_encode_chat", |b| {
        b.iter(|| {
            buf.clear();
            black_box(&nbt).encode(&mut buf);
            black_box(buf.len())
        })
    });
}

criterion_group!(benches, bench_nbt_encode);
criterion_main!(benches);
