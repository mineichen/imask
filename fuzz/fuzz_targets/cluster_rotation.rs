#![no_main]

use std::num::NonZero;

use imask::{BitmapToSpanIter, ImaskSet};
use libfuzzer_sys::fuzz_target;

const SIDE: usize = 21;
const PIXELS: usize = SIDE * SIDE;

/// Cluster the given `SIDE x SIDE` pixel grid and return the sorted pixel
/// counts of every cluster.
fn cluster_sizes(pixels: &[bool; PIXELS]) -> Vec<usize> {
    let iter = BitmapToSpanIter::from_bool_iter(
        pixels.iter().copied(),
        NonZero::new(SIDE as u32).unwrap(),
        NonZero::new(SIDE as u32).unwrap(),
    );
    let mut sizes: Vec<usize> = iter
        .cluster::<u32>()
        .map(|cluster| cluster.map(|s| (s.x.end - s.x.start) as usize).sum())
        .collect();
    sizes.sort_unstable();
    sizes
}

/// 90° clockwise rotation of the `SIDE x SIDE` grid about its centre.
fn rotate90(pixels: &[bool; PIXELS]) -> [bool; PIXELS] {
    let mut out = [false; PIXELS];
    for y in 0..SIDE {
        for x in 0..SIDE {
            out[x * SIDE + (SIDE - 1 - y)] = pixels[y * SIDE + x];
        }
    }
    out
}

// 8-connectivity is invariant under rotation, so the four orientations of
// every bitmap must produce identical clusters.
fuzz_target!(|data: &[u8]| {
    let mut pixels = [false; PIXELS];
    for i in 0..PIXELS {
        let byte = data.get(i / 8).copied().unwrap_or(0);
        pixels[i] = byte & (1 << (i % 8)) != 0;
    }
    let base = cluster_sizes(&pixels);
    let r90 = rotate90(&pixels);
    let r180 = rotate90(&r90);
    let r270 = rotate90(&r180);
    assert_eq!(base, cluster_sizes(&r90), "90° rotation changed clusters");
    assert_eq!(base, cluster_sizes(&r180), "180° rotation changed clusters");
    assert_eq!(base, cluster_sizes(&r270), "270° rotation changed clusters");
});
