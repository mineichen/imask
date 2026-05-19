use std::num::{NonZero, NonZeroU32};
use std::ops::Range;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use imask::*;

const W: NonZeroU32 = NonZero::new(1024).unwrap();
const H: NonZeroU32 = NonZero::new(1024).unwrap();

fn rect(x: u32, y: u32, w: u32, h: u32) -> Rect<u32> {
    Rect::new(x, y, NonZero::new(w).unwrap(), NonZero::new(h).unwrap())
}

fn consume<I: Iterator>(iter: I) {
    for item in iter {
        black_box(item);
    }
}

fn bench_union(c: &mut Criterion) {
    let mut group = c.benchmark_group("union");

    group.bench_function("overlapping_500x500", |bencher| {
        let a = rect(0, 0, 500, 500).into_spans();
        let b = rect(250, 0, 500, 500).into_spans();
        bencher.iter(|| consume(Union::new(a.clone(), b.clone())));
    });

    group.bench_function("overlapping_1000x1000", |bencher| {
        let a = rect(0, 0, 1000, 1000).into_spans();
        let b = rect(500, 0, 500, 1000).into_spans();
        bencher.iter(|| consume(Union::new(a.clone(), b.clone())));
    });

    group.bench_function("non_overlapping_500x500", |bencher| {
        let a = rect(0, 0, 500, 500).into_spans();
        let b = rect(0, 500, 500, 500).into_spans();
        bencher.iter(|| consume(Union::new(a.clone(), b.clone())));
    });

    group.bench_function("interleaved_500rows", |bencher| {
        let a: Vec<Span<u32>> = (0..500).map(|y| Span::new(0..200, y)).collect();
        let b: Vec<Span<u32>> = (0..500).map(|y| Span::new(100..300, y)).collect();
        bencher.iter(|| {
            consume(Union::new(a.clone().into_iter(), b.clone().into_iter()));
        });
    });

    group.finish();
}

fn bench_subtract(c: &mut Criterion) {
    let mut group = c.benchmark_group("subtract");

    group.bench_function("partial_overlap_500x500", |bencher| {
        let a = rect(0, 0, 500, 500).into_spans();
        let b = rect(250, 0, 500, 500).into_spans();
        bencher.iter(|| consume(Subtract::new(a.clone(), b.clone())));
    });

    group.bench_function("partial_overlap_1000x1000", |bencher| {
        let a = rect(0, 0, 1000, 1000).into_spans();
        let b = rect(500, 0, 500, 1000).into_spans();
        bencher.iter(|| consume(Subtract::new(a.clone(), b.clone())));
    });

    group.bench_function("subtract_middle_500rows", |bencher| {
        let a: Vec<Span<u32>> = (0..500).map(|y| Span::new(0..500, y)).collect();
        let b: Vec<Span<u32>> = (0..500).map(|y| Span::new(100..400, y)).collect();
        bencher.iter(|| {
            consume(Subtract::new(a.clone().into_iter(), b.clone().into_iter()));
        });
    });

    group.bench_function("no_overlap_500x500", |bencher| {
        let a = rect(0, 0, 500, 500).into_spans();
        let b = rect(0, 500, 500, 500).into_spans();
        bencher.iter(|| consume(Subtract::new(a.clone(), b.clone())));
    });

    group.finish();
}

fn bench_dilate(c: &mut Criterion) {
    let mut group = c.benchmark_group("dilate");

    for radius in [1u32, 3, 5] {
        group.bench_function(format!("50x50_r{radius}"), |bencher| {
            let r = rect(50, 50, 50, 50);
            bencher.iter(|| {
                consume(
                    r.into_spans()
                        .with_bounds(W, H)
                        .dilate::<u32>(NonZero::new(radius).unwrap())
                        .unwrap(),
                );
            });
        });
    }

    for radius in [1u32, 3, 5] {
        group.bench_function(format!("200x200_r{radius}"), |bencher| {
            let r = rect(100, 100, 200, 200);
            bencher.iter(|| {
                consume(
                    r.into_spans()
                        .with_bounds(W, H)
                        .dilate::<u32>(NonZero::new(radius).unwrap())
                        .unwrap(),
                );
            });
        });
    }

    group.bench_function("edge_touching_50x50_r2", |bencher| {
        let r = rect(0, 0, 50, 50);
        bencher.iter(|| {
            consume(
                r.into_spans()
                    .with_bounds(W, H)
                    .dilate::<u32>(NonZero::new(2).unwrap())
                    .unwrap(),
            );
        });
    });

    group.finish();
}

fn bench_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline");

    group.bench_function("dilate_clip_ranges", |bencher| {
        let r = rect(50, 50, 50, 50);
        let clip_bounds = rect(0, 0, 200, 200);
        bencher.iter(|| {
            consume(
                ClipSpanIter::new(
                    r.into_spans()
                        .with_bounds(W, H)
                        .dilate::<u32>(NonZero::new(3).unwrap())
                        .unwrap(),
                    clip_bounds,
                )
                .into_ranges::<Range<u32>>(),
            );
        });
    });

    group.bench_function("union_dilate_clip_ranges", |bencher| {
        let a = rect(10, 10, 30, 30).into_spans();
        let b = rect(60, 60, 30, 30).into_spans();
        let clip_bounds = rect(0, 0, 200, 200);
        bencher.iter(|| {
            consume(
                ClipSpanIter::new(
                    Union::new(a.clone(), b.clone())
                        .with_bounds(W, H)
                        .dilate::<u32>(NonZero::new(2).unwrap())
                        .unwrap(),
                    clip_bounds,
                )
                .into_ranges::<Range<u32>>(),
            );
        });
    });

    group.bench_function("union_subtract_clip_ranges", |bencher| {
        let a = rect(0, 0, 100, 100).into_spans();
        let b = rect(50, 50, 100, 100).into_spans();
        let hole = rect(30, 30, 20, 20).into_spans();
        let clip_bounds = rect(0, 0, 150, 150);
        bencher.iter(|| {
            consume(
                ClipSpanIter::new(
                    Subtract::new(Union::new(a.clone(), b.clone()), hole.clone()),
                    clip_bounds,
                )
                .into_ranges::<Range<u32>>(),
            );
        });
    });

    group.bench_function("dilate_clip_ranges_large", |bencher| {
        let r = rect(100, 100, 200, 200);
        let clip_bounds = rect(0, 0, 500, 500);
        bencher.iter(|| {
            consume(
                ClipSpanIter::new(
                    r.into_spans()
                        .with_bounds(W, H)
                        .dilate::<u32>(NonZero::new(5).unwrap())
                        .unwrap(),
                    clip_bounds,
                )
                .into_ranges::<Range<u32>>(),
            );
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_union,
    bench_subtract,
    bench_dilate,
    bench_pipeline,
);
criterion_main!(benches);
