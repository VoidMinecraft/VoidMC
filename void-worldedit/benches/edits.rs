use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use voidmc_worldedit::{
    BlockPos, BlockState, Cuboid, EditJob, Ellipsoid, Fill, Mask, MemoryExtent, Pattern, Restore,
};

const HEIGHT: (i32, i32) = (-64, 319);

fn million() -> Cuboid {
    Cuboid::new(BlockPos::new(0, 0, 0), BlockPos::new(99, 99, 99))
}

fn state(input: &str) -> BlockState {
    BlockState::parse(input).unwrap()
}

fn terrain() -> MemoryExtent {
    let mut extent = MemoryExtent::default();
    let pattern = Pattern::parse("stone,dirt,granite,andesite").unwrap();
    EditJob::new(Fill::new(million(), pattern), HEIGHT).run(&mut extent);
    extent
}

fn set_one_million(c: &mut Criterion) {
    c.bench_function("set 1M blocks (uniform)", |b| {
        b.iter_batched(
            MemoryExtent::default,
            |mut extent| {
                let job = EditJob::new(Fill::new(million(), state("stone")), HEIGHT);
                black_box(job.run(&mut extent))
            },
            criterion::BatchSize::LargeInput,
        )
    });

    c.bench_function("set 1M blocks (random pattern)", |b| {
        let pattern = Pattern::parse("50%stone,30%dirt,20%gravel").unwrap();
        b.iter_batched(
            MemoryExtent::default,
            |mut extent| {
                let job = EditJob::new(Fill::new(million(), pattern.clone()), HEIGHT);
                black_box(job.run(&mut extent))
            },
            criterion::BatchSize::LargeInput,
        )
    });
}

fn replace_with_mask(c: &mut Criterion) {
    let base = terrain();
    let mask = Mask::parse("dirt,granite").unwrap();
    c.bench_function("replace 1M blocks with mask", |b| {
        b.iter_batched(
            || base.clone(),
            |mut extent| {
                let op = Fill::new(million(), state("glass")).masked(mask.clone());
                black_box(EditJob::new(op, HEIGHT).run(&mut extent))
            },
            criterion::BatchSize::LargeInput,
        )
    });
}

fn undo(c: &mut Criterion) {
    let mut base = terrain();
    let (changes, _) = EditJob::new(Fill::new(million(), state("stone")), HEIGHT).run(&mut base);
    let changes = Arc::new(changes);
    c.bench_function("undo 1M block edit", |b| {
        b.iter_batched(
            || base.clone(),
            |mut extent| {
                black_box(EditJob::new(Restore::undo(changes.clone()), HEIGHT).run(&mut extent))
            },
            criterion::BatchSize::LargeInput,
        )
    });
}

fn sphere(c: &mut Criterion) {
    c.bench_function("sphere radius 40 (~270k blocks)", |b| {
        b.iter_batched(
            MemoryExtent::default,
            |mut extent| {
                let op = Fill::new(
                    Ellipsoid::sphere(BlockPos::new(0, 64, 0), 40.0),
                    state("stone"),
                );
                black_box(EditJob::new(op, HEIGHT).run(&mut extent))
            },
            criterion::BatchSize::LargeInput,
        )
    });
}

criterion_group!(benches, set_one_million, replace_with_mask, undo, sphere);
criterion_main!(benches);
