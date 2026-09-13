use std::ops::Range;
use std::{
    hint::black_box,
    num::{NonZero, NonZeroU32},
};

use criterion::{Criterion, criterion_group, criterion_main};

use imask::*;

const W: NonZeroU32 = NonZero::new(1024).unwrap();
const H: NonZeroU32 = NonZero::new(1024).unwrap();

fn consume_span<I: Iterator<Item = Span<u32>>>(iter: I) {
    for item in iter {
        black_box(item);
    }
}
fn consume_range<I: Iterator<Item = Range<u32>>>(iter: I) {
    for item in iter {
        black_box(item);
    }
}

fn bench_union(c: &mut Criterion) {
    let mut group = c.benchmark_group("union");

    group.bench_function("overlapping_500x500", |bencher| {
        let a = Roi::new(0u32..500, 0..500).into_spans();
        let b = Roi::new(250u32..750, 0..500).into_spans();
        bencher.iter(|| consume_span(Union::new(a.clone(), b.clone())));
    });

    group.bench_function("overlapping_1000x1000", |bencher| {
        let a = Roi::new(0..1000, 0..1000).into_spans();
        let b = Roi::new(500..1000, 0..1000).into_spans();
        bencher.iter(|| consume_span(Union::new(a.clone(), b.clone())));
    });

    group.bench_function("non_overlapping_500x500", |bencher| {
        let a = Roi::new(0..500, 0..500).into_spans();
        let b = Roi::new(0..500, 500..1000).into_spans();
        bencher.iter(|| consume_span(Union::new(a.clone(), b.clone())));
    });

    group.bench_function("interleaved_500rows", |bencher| {
        let a: Vec<Span<u32>> = (0..500).map(|y| Span::new(0..200, y)).collect();
        let b: Vec<Span<u32>> = (0..500).map(|y| Span::new(100..300, y)).collect();
        bencher.iter(|| {
            consume_span(Union::new(a.clone().into_iter(), b.clone().into_iter()));
        });
    });

    group.finish();
}

fn bench_subtract(c: &mut Criterion) {
    let mut group = c.benchmark_group("subtract");

    group.bench_function("partial_overlap_500x500", |bencher| {
        let a = Roi::new(0..500, 0..500).into_spans();
        let b = Roi::new(250..750, 0..500).into_spans();
        bencher.iter(|| consume_span(Subtract::new(a.clone(), b.clone())));
    });

    group.bench_function("partial_overlap_1000x1000", |bencher| {
        let a = Roi::new(0..1000, 0..1000).into_spans();
        let b = Roi::new(500..1000, 0..1000).into_spans();
        bencher.iter(|| consume_span(Subtract::new(a.clone(), b.clone())));
    });

    group.bench_function("subtract_middle_500rows", |bencher| {
        let a: Vec<Span<u32>> = (0..500).map(|y| Span::new(0..500, y)).collect();
        let b: Vec<Span<u32>> = (0..500).map(|y| Span::new(100..400, y)).collect();
        bencher.iter(|| {
            consume_span(Subtract::new(a.clone().into_iter(), b.clone().into_iter()));
        });
    });

    group.bench_function("no_overlap_500x500", |bencher| {
        let a = Roi::new(0..500, 0..500).into_spans();
        let b = Roi::new(0..500, 500..1000).into_spans();
        bencher.iter(|| consume_span(Subtract::new(a.clone(), b.clone())));
    });

    group.finish();
}

fn dilate_union<I>(iter: I, radius: NonZero<u32>) -> DilateSpanIter<WithRoi<I>, u32>
where
    I: Iterator<Item = Span<u32>> + Clone + ImageDimension,
{
    let roi = iter.roi().expand_saturating(radius.get());
    DilateSpanIter::new(iter.with_roi(roi), radius).unwrap()
}

fn dilate_acc<I>(iter: I, radius: NonZero<u32>) -> DilateSpanIterAcc<WithRoi<I>, u32>
where
    I: Iterator<Item = Span<u32>> + ImageDimension,
{
    let roi = iter.roi().expand_saturating(radius.get());
    DilateSpanIterAcc::new(iter.with_roi(roi), radius).unwrap()
}

fn bench_dilate(c: &mut Criterion) {
    let mut group = c.benchmark_group("dilate");

    for radius in [1u32, 3, 5] {
        group.bench_function(format!("50x50_r{radius}_union"), |bencher| {
            let r = Roi::new(50..100, 50..100);
            let radius = NonZero::new(radius).unwrap();
            bencher.iter(|| {
                consume_span(dilate_union(r.into_spans().with_bounds(W, H), radius));
            });
        });
    }

    for radius in [1u32, 3, 5] {
        group.bench_function(format!("200x200_r{radius}_union"), |bencher| {
            let r = Roi::new(100..300, 100..300);
            let radius = NonZero::new(radius).unwrap();
            bencher.iter(|| {
                consume_span(dilate_union(r.into_spans().with_bounds(W, H), radius));
            });
        });
    }

    group.bench_function("edge_touching_50x50_r2_union", |bencher| {
        let r = Roi::new(0..50, 0..50);
        let radius = NonZero::new(2).unwrap();
        bencher.iter(|| {
            consume_span(dilate_union(r.into_spans().with_bounds(W, H), radius));
        });
    });

    for radius in [1u32, 3, 5] {
        group.bench_function(format!("50x50_r{radius}_acc"), |bencher| {
            let r = Roi::new(50..100, 50..100);
            let radius = NonZero::new(radius).unwrap();
            bencher.iter(|| {
                consume_span(dilate_acc(r.into_spans().with_bounds(W, H), radius));
            });
        });
    }

    for radius in [1u32, 3, 5] {
        group.bench_function(format!("200x200_r{radius}_acc"), |bencher| {
            let r = Roi::new(100..300, 100..300);
            let radius = NonZero::new(radius).unwrap();
            bencher.iter(|| {
                consume_span(dilate_acc(r.into_spans().with_bounds(W, H), radius));
            });
        });
    }

    group.bench_function("edge_touching_50x50_r2_acc", |bencher| {
        let r = Roi::new(0..50, 0..50);
        let radius = NonZero::new(2).unwrap();
        bencher.iter(|| {
            consume_span(dilate_acc(r.into_spans().with_bounds(W, H), radius));
        });
    });

    group.bench_function("box_800x800_r200_union", |bencher| {
        let r = Roi::new(200..1000, 200..1000);
        let radius = NonZero::new(200).unwrap();
        bencher.iter(|| {
            consume_span(dilate_union(r.into_spans().with_bounds(W, H), radius));
        });
    });

    group.bench_function("box_800x800_r200_acc", |bencher| {
        let r = Roi::new(200..1000, 200..1000);
        let radius = NonZero::new(200).unwrap();
        bencher.iter(|| {
            consume_span(dilate_acc(r.into_spans().with_bounds(W, H), radius));
        });
    });

    group.finish();
}

fn bench_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline");

    group.bench_function("dilate_clip_ranges", |bencher| {
        let r = Roi::new(50..100, 50..100);
        let clip_bounds = Roi::new(0..200, 0..200);
        let radius = NonZero::new(3).unwrap();
        bencher.iter(|| {
            consume_range(
                ClipSpanIter::new(
                    dilate_union(r.into_spans().with_bounds(W, H), radius),
                    clip_bounds,
                )
                .into_ranges::<Range<u32>>(),
            );
        });
    });

    group.bench_function("union_dilate_clip_ranges", |bencher| {
        let a = Roi::new(10..40, 10..40).into_spans();
        let b = Roi::new(60..90, 60..90).into_spans();
        let clip_bounds = Roi::new(0..200, 0..200);
        let radius = NonZero::new(2).unwrap();
        bencher.iter(|| {
            consume_range(
                ClipSpanIter::new(
                    dilate_union(Union::new(a.clone(), b.clone()).with_bounds(W, H), radius),
                    clip_bounds,
                )
                .into_ranges::<Range<u32>>(),
            );
        });
    });

    group.bench_function("union_subtract_clip_ranges", |bencher| {
        let a = Roi::new(0..100, 0..100).into_spans();
        let b = Roi::new(50..150, 50..150).into_spans();
        let hole = Roi::new(30..50, 30..50).into_spans();
        let clip_bounds = Roi::new(0..150, 0..150);
        bencher.iter(|| {
            consume_range(
                ClipSpanIter::new(
                    Subtract::new(Union::new(a.clone(), b.clone()), hole.clone()),
                    clip_bounds,
                )
                .into_ranges::<Range<u32>>(),
            );
        });
    });

    group.bench_function("dilate_clip_ranges_large", |bencher| {
        let r = Roi::new(100..300, 100..300);
        let clip_bounds = Roi::new(0..500, 0..500);
        let radius = NonZero::new(5).unwrap();
        bencher.iter(|| {
            consume_range(
                ClipSpanIter::new(
                    dilate_acc(r.into_spans().with_bounds(W, H), radius),
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
