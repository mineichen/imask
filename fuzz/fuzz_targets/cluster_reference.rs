#![no_main]

use std::num::NonZero;

use imask::{BitmapToSpanIter, ImaskSet, Span};
use libfuzzer_sys::fuzz_target;

/// Reference 8-connectivity clustering via union-find over single pixels.
fn reference_clusters(width: usize, height: usize, set: &[bool]) -> Vec<Vec<Span<u32>>> {
    let n = width * height;
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[rb] = ra;
        }
    }
    let idx = |y: usize, x: usize| y * width + x;
    let is_set = |y: usize, x: usize| set[idx(y, x)];
    for y in 0..height {
        for x in 0..width {
            if !is_set(y, x) {
                continue;
            }
            let i = idx(y, x);
            if x > 0 && is_set(y, x - 1) {
                union(&mut parent, i, idx(y, x - 1));
            }
            if y > 0 {
                if x > 0 && is_set(y - 1, x - 1) {
                    union(&mut parent, i, idx(y - 1, x - 1));
                }
                if is_set(y - 1, x) {
                    union(&mut parent, i, idx(y - 1, x));
                }
                if x + 1 < width && is_set(y - 1, x + 1) {
                    union(&mut parent, i, idx(y - 1, x + 1));
                }
            }
        }
    }
    let mut clusters: std::collections::HashMap<usize, Vec<(usize, usize)>> = Default::default();
    for y in 0..height {
        for x in 0..width {
            if is_set(y, x) {
                clusters.entry(find(&mut parent, idx(y, x))).or_default().push((y, x));
            }
        }
    }
    let mut out = Vec::new();
    for mut pixels in clusters.into_values() {
        pixels.sort_unstable();
        let mut spans: Vec<Span<u32>> = Vec::new();
        for (y, x) in pixels {
            let extend = spans
                .last_mut()
                .is_some_and(|l| l.y == y as u32 && l.x.end as usize == x);
            if extend {
                let last = spans.last_mut().unwrap();
                let start = last.x.start;
                *last = Span::new(start..x as u32 + 1, y as u32);
            } else {
                spans.push(Span::new(x as u32..x as u32 + 1, y as u32));
            }
        }
        out.push(spans);
    }
    out.sort_by(|a, b| {
        a[0].y
            .cmp(&b[0].y)
            .then_with(|| a[0].x.start.cmp(&b[0].x.start))
    });
    out
}

fuzz_target!(|data: &[u8]| {
    let width = (data.first().copied().unwrap_or(0) as usize % 24) + 1;
    let height = (data.get(1).copied().unwrap_or(0) as usize % 24) + 1;
    let n = width * height;
    let set: Vec<bool> = (0..n)
        .map(|i| data.get(i + 2).copied().unwrap_or(0) != 0)
        .collect();

    let iter = BitmapToSpanIter::from_bool_iter(
        set.iter().copied(),
        NonZero::new(width as u32).unwrap(),
        NonZero::new(height as u32).unwrap(),
    );
    let mut got: Vec<Vec<Span<u32>>> = iter
        .cluster::<u32>()
        .map(|cluster| cluster.collect())
        .collect();
    got.sort_by(|a, b| {
        a[0].y
            .cmp(&b[0].y)
            .then_with(|| a[0].x.start.cmp(&b[0].x.start))
    });

    let want = reference_clusters(width, height, &set);
    assert_eq!(
        got, want,
        "cluster mismatch for {width}x{height} bitmap from {} bytes",
        data.len()
    );
});
