use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::marker::PhantomData;
use std::num::NonZero;

use nalgebra::{Matrix3, Vector3};

use crate::{CreateRange, ImageDimension, NonZeroRange, Rect, Span};

fn transform_point(m: &Matrix3<f64>, x: f64, y: f64) -> (f64, f64) {
    let v = m * Vector3::new(x, y, 1.0);
    (v[0], v[1])
}

fn transform_bounds_rect(parent: Rect<u32>, matrix: &Matrix3<f64>) -> Option<Rect<u32>> {
    let left = parent.x as f64 - 0.5;
    let right = (parent.x + parent.width.get()) as f64 - 0.5;
    let top = parent.y as f64 - 0.5;
    let bottom = (parent.y + parent.height.get()) as f64 - 0.5;

    let corners = [
        transform_point(matrix, left, top),
        transform_point(matrix, right, top),
        transform_point(matrix, right, bottom),
        transform_point(matrix, left, bottom),
    ];

    let min_x = corners.iter().map(|c| c.0).fold(f64::MAX, f64::min);
    let min_y = corners.iter().map(|c| c.1).fold(f64::MAX, f64::min);
    let max_x = corners.iter().map(|c| c.0).fold(f64::MIN, f64::max);
    let max_y = corners.iter().map(|c| c.1).fold(f64::MIN, f64::max);

    if max_x < 0.0 || max_y < 0.0 {
        return None;
    }

    let bx = min_x.max(0.0).ceil() as u32;
    let by = min_y.max(0.0).ceil() as u32;
    let bx_end = max_x.floor() as u32 + 1;
    let by_end = max_y.floor() as u32 + 1;

    let width = NonZero::new(bx_end.saturating_sub(bx))?;
    let height = NonZero::new(by_end.saturating_sub(by))?;

    Some(Rect::new(bx, by, width, height))
}

fn quad_corners(
    matrix: &Matrix3<f64>,
    col: u64,
    row: u64,
    w: u64,
) -> [(f64, f64); 4] {
    let left = col as f64 - 0.5;
    let right = col as f64 + w as f64 - 0.5;
    let top = row as f64 - 0.5;
    let bottom = row as f64 + 0.5;
    [
        transform_point(matrix, left, top),
        transform_point(matrix, right, top),
        transform_point(matrix, right, bottom),
        transform_point(matrix, left, bottom),
    ]
}

const FP_SHIFT: u32 = 8;
const FP_SCALE: i32 = 1i32 << FP_SHIFT;

fn to_fp(f: f64) -> i32 {
    (f * FP_SCALE as f64).round() as i32
}

fn fp_floor(v: i32) -> i32 {
    v >> FP_SHIFT
}

fn fp_ceil(v: i32) -> i32 {
    (v + FP_SCALE - 1) >> FP_SHIFT
}

fn floor_div_i64(a: i64, b: i64) -> i64 {
    let d = a / b;
    let r = a % b;
    if r != 0 && (a < 0) != (b < 0) {
        d - 1
    } else {
        d
    }
}

struct QuadSpanIter {
    x_fp: [i32; 4],
    y: [i32; 4],
    y_end: [i32; 4],
    accum: [i32; 4],
    step: [i64; 4],
    dy_fp: [i32; 4],
    row_y: i32,
    row_y_end: i32,
    x_left: i32,
    x_right: i32,
}

impl QuadSpanIter {
    fn new(corners: &[(f64, f64); 4]) -> Self {
        let c = [
            (to_fp(corners[0].0), to_fp(corners[0].1)),
            (to_fp(corners[1].0), to_fp(corners[1].1)),
            (to_fp(corners[2].0), to_fp(corners[2].1)),
            (to_fp(corners[3].0), to_fp(corners[3].1)),
        ];

        let edge_inputs: [(i32, i32, i32, i32); 4] = [
            (c[0].0, c[0].1, c[1].0, c[1].1),
            (c[1].0, c[1].1, c[2].0, c[2].1),
            (c[2].0, c[2].1, c[3].0, c[3].1),
            (c[3].0, c[3].1, c[0].0, c[0].1),
        ];

        let mut x_fp = [0i32; 4];
        let mut ey = [0i32; 4];
        let mut ey_end = [0i32; 4];
        let mut accum = [0i32; 4];
        let mut step = [0i64; 4];
        let mut dy_fp = [1i32; 4];

        for (i, &(x0, y0, x1, y1)) in edge_inputs.iter().enumerate() {
            let (x0, y0, x1, y1) = if y0 <= y1 {
                (x0, y0, x1, y1)
            } else {
                (x1, y1, x0, y0)
            };

            let dy = (y1 - y0) as i64;
            if dy == 0 {
                x_fp[i] = x0;
                ey[i] = i32::MAX;
                ey_end[i] = i32::MIN;
                continue;
            }

            let y_start = fp_ceil(y0);
            let y_end_val = fp_floor(y1);
            let dx = (x1 - x0) as i64;
            let s = dx * FP_SCALE as i64;

            let yf = y_start as i64 * FP_SCALE as i64;
            let init_num = (yf - y0 as i64) * dx;
            let q = floor_div_i64(init_num, dy);
            x_fp[i] = x0 + q as i32;
            accum[i] = (init_num - q * dy) as i32;
            ey[i] = y_start;
            ey_end[i] = y_end_val;
            step[i] = s;
            dy_fp[i] = dy as i32;
        }

        let min_y = c.iter().map(|v| v.1).min().unwrap_or(0);
        let max_y = c.iter().map(|v| v.1).max().unwrap_or(0);
        let y_start = fp_ceil(min_y);
        let y_end = fp_floor(max_y);

        let mut iter = Self {
            x_fp,
            y: ey,
            y_end: ey_end,
            accum,
            step,
            dy_fp,
            row_y: y_start,
            row_y_end: y_end,
            x_left: i32::MAX,
            x_right: i32::MIN,
        };

        iter.skip_to_valid();
        iter
    }

    fn exhausted(&self) -> bool {
        self.row_y > self.row_y_end
    }

    fn current(&self) -> Span<u32> {
        debug_assert!(!self.exhausted());
        let cs = self.x_left.max(0) as u32;
        let ce = self.x_right as u32 + 1;
        debug_assert!(cs < ce);
        Span {
            y: self.row_y as u32,
            x: NonZeroRange::new_debug_checked_zeroable(cs, ce),
        }
    }

    #[inline]
    fn compute_x_bounds(&self) -> (i32, i32) {
        let mut x_left = i32::MAX;
        let mut x_right = i32::MIN;
        for i in 0..4 {
            if self.y[i] == self.row_y && self.y[i] <= self.y_end[i] {
                x_left = x_left.min(fp_ceil(self.x_fp[i]));
                x_right = x_right.max(fp_floor(self.x_fp[i]));
            }
        }
        (x_left, x_right)
    }

    #[inline]
    fn advance_edge(&mut self, i: usize) {
        self.y[i] += 1;
        let s = self.step[i];
        let d = self.dy_fp[i] as i64;
        let mut a = self.accum[i] as i64 + s;
        let q = floor_div_i64(a, d);
        self.x_fp[i] += q as i32;
        a -= q * d;
        self.accum[i] = a as i32;
    }

    #[inline]
    fn advance_row(&mut self) {
        self.row_y += 1;
        if self.row_y > self.row_y_end {
            return;
        }
        for i in 0..4 {
            if self.y[i] < self.row_y && self.y[i] <= self.y_end[i] {
                self.advance_edge(i);
            }
        }
    }

    fn skip_to_valid(&mut self) {
        while self.row_y <= self.row_y_end {
            let (xl, xr) = self.compute_x_bounds();
            if xl <= xr && self.row_y >= 0 && xr >= 0 {
                self.x_left = xl;
                self.x_right = xr;
                return;
            }
            self.advance_row();
        }
    }

    fn advance(&mut self) -> bool {
        self.advance_row();
        self.skip_to_valid();
        !self.exhausted()
    }
}

struct HeapEntry {
    iter: QuadSpanIter,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.iter.row_y == other.iter.row_y && self.iter.x_left == other.iter.x_left
    }
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        match other.iter.row_y.cmp(&self.iter.row_y) {
            Ordering::Equal => other.iter.x_left.cmp(&self.iter.x_left),
            ord => ord,
        }
    }
}

pub struct AffineTransformHeap<I> {
    heap: BinaryHeap<HeapEntry>,
    bounds: Rect<u32>,
    pending: Option<Span<u32>>,
    _phantom: PhantomData<I>,
}

impl<I: Iterator<Item = Span<u32>> + ImageDimension> AffineTransformHeap<I> {
    pub fn new(spans: I, matrix: &Matrix3<f64>) -> Option<Self> {
        let parent_bounds = spans.bounds();
        let bounds = transform_bounds_rect(parent_bounds, matrix)?;

        let mut entries: Vec<HeapEntry> = Vec::new();
        for span in spans {
            let col = span.x.start as u64;
            let row = span.y as u64;
            let seg_width = (span.x.end - span.x.start) as u64;

            let corners = quad_corners(matrix, col, row, seg_width);
            let iter = QuadSpanIter::new(&corners);
            if !iter.exhausted() {
                entries.push(HeapEntry { iter });
            }
        }

        Some(Self {
            heap: BinaryHeap::from(entries),
            bounds,
            pending: None,
            _phantom: PhantomData,
        })
    }

    fn pop_span(&mut self) -> Option<Span<u32>> {
        let mut entry = self.heap.pop()?;
        let result = entry.iter.current();

        if entry.iter.advance() {
            self.heap.push(entry);
        }

        debug_assert!(result.x.end <= self.bounds.x + self.bounds.width.get());
        debug_assert!(result.y < self.bounds.y + self.bounds.height.get());

        Some(result)
    }
}

impl<I: Iterator<Item = Span<u32>> + ImageDimension> ImageDimension for AffineTransformHeap<I> {
    fn bounds(&self) -> Rect<u32> {
        self.bounds
    }

    fn width(&self) -> NonZero<u32> {
        self.bounds.width
    }
}

impl<I: Iterator<Item = Span<u32>> + ImageDimension> Iterator for AffineTransformHeap<I> {
    type Item = Span<u32>;

    fn next(&mut self) -> Option<Span<u32>> {
        let first = self.pending.take().or_else(|| self.pop_span())?;
        let y = first.y;
        let mut merged_end = first.x.end;

        loop {
            match self.pop_span() {
                Some(s) if s.y == y && s.x.start <= merged_end => {
                    merged_end = merged_end.max(s.x.end);
                }
                Some(s) => {
                    self.pending = Some(s);
                    break;
                }
                None => break,
            }
        }

        Some(Span {
            y,
            x: NonZeroRange::new_debug_checked_zeroable(first.x.start, merged_end),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nz(n: u32) -> NonZero<u32> {
        NonZero::new(n).unwrap()
    }

    fn print_bitmap(w: u32, h: u32, spans: &[Span<u32>], label: &str) {
        let mut bitmap = vec![false; (w * h) as usize];
        for span in spans {
            for x in span.x.start..span.x.end {
                if ((span.y * w + x) as usize) < bitmap.len() {
                    bitmap[(span.y * w + x) as usize] = true;
                }
            }
        }
        eprintln!("\n{}:", label);
        for y in 0..h {
            let row: String = (0..w)
                .map(|x| if bitmap[(y * w + x) as usize] { '#' } else { '.' })
                .collect();
            eprintln!("  {}", row);
        }
    }

    #[test]
    fn rotate_l_90deg_cw_about_center() {
        let l_spans: Vec<Span<u32>> = vec![
            Span::new(0..1, 0),
            Span::new(0..1, 1),
            Span::new(0..1, 2),
            Span::new(0..1, 3),
            Span::new(0..6, 4),
        ];

        print_bitmap(7, 7, &l_spans, "Input L-shape");

        let cx = 3.0_f64;
        let cy = 3.0_f64;
        let matrix = Matrix3::new(
            0.0, 1.0, cx - cy, -1.0, 0.0, cx + cy, 0.0, 0.0, 1.0,
        );

        let roi = Rect::new(0, 0, nz(6), nz(5));
        let wrapped = crate::WithRoi::new(l_spans.into_iter(), roi);
        let heap = AffineTransformHeap::new(wrapped, &matrix).unwrap();
        let result: Vec<Span<u32>> = heap.collect();

        let expected: Vec<Span<u32>> = vec![
            Span::new(4..5, 1),
            Span::new(4..5, 2),
            Span::new(4..5, 3),
            Span::new(4..5, 4),
            Span::new(4..5, 5),
            Span::new(0..5, 6),
        ];
        assert_eq!(result, expected);

        print_bitmap(7, 7, &result, "Output after 90° CW rotation");
    }

    #[test]
    fn translate_completely_negative_returns_none() {
        let rect = crate::Rect::new(1u32, 1, nz(3), nz(3));
        let spans = rect.into_spans();
        let matrix = Matrix3::new(1.0, 0.0, -100.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0);
        assert!(AffineTransformHeap::new(spans, &matrix).is_none());
    }

    #[test]
    fn scale_from_center_no_gaps() {
        let rect = crate::Rect::new(2u32, 2, nz(3), nz(3));
        let spans = rect.into_spans();

        let cx = 3.0_f64;
        let cy = 3.0_f64;
        let scale = 2.0_f64;
        let matrix = Matrix3::new(
            scale,
            0.0,
            cx * (1.0 - scale),
            0.0,
            scale,
            cy * (1.0 - scale),
            0.0,
            0.0,
            1.0,
        );

        let heap = AffineTransformHeap::new(spans, &matrix).unwrap();
        let result: Vec<Span<u32>> = heap.collect();

        let expected: Vec<Span<u32>> = (0..7).map(|y| Span::new(0..7, y)).collect();
        assert_eq!(result, expected);
    }

    fn flood_fill_connected(w: u32, h: u32, bitmap: &[bool]) -> bool {
        let total = bitmap.iter().filter(|&&b| b).count();
        if total == 0 {
            return true;
        }
        let first = bitmap.iter().position(|&b| b).unwrap();
        let mut visited = vec![false; (w * h) as usize];
        let mut stack = vec![first as u32];
        visited[first] = true;
        let mut count = 1usize;
        while let Some(idx) = stack.pop() {
            let px = idx % w;
            let py = idx / w;
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let nx = px as i32 + dx;
                let ny = py as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let ni = (ny as u32 * w + nx as u32) as usize;
                if !visited[ni] && bitmap[ni] {
                    visited[ni] = true;
                    count += 1;
                    stack.push(ni as u32);
                }
            }
        }
        count == total
    }

    fn each_row_is_contiguous(w: u32, h: u32, bitmap: &[bool]) -> bool {
        for y in 0..h {
            let mut transitions = 0u32;
            let mut prev = false;
            for x in 0..w {
                let cur = bitmap[(y * w + x) as usize];
                if cur != prev {
                    transitions += 1;
                }
                prev = cur;
            }
            let runs = (transitions + 1) / 2;
            if runs > 1 {
                return false;
            }
        }
        true
    }

    fn rotation_matrix(cx: f64, cy: f64, angle_deg: f64) -> Matrix3<f64> {
        let angle = angle_deg.to_radians();
        let cos = angle.cos();
        let sin = angle.sin();
        Matrix3::new(
            cos,
            -sin,
            cx * (1.0 - cos) + cy * sin,
            sin,
            cos,
            cy * (1.0 - cos) - cx * sin,
            0.0,
            0.0,
            1.0,
        )
    }

    fn spans_to_bitmap(w: u32, h: u32, spans: &[Span<u32>]) -> Vec<bool> {
        let mut bitmap = vec![false; (w * h) as usize];
        for span in spans {
            for x in span.x.start..span.x.end {
                if ((span.y * w + x) as usize) < bitmap.len() {
                    bitmap[(span.y * w + x) as usize] = true;
                }
            }
        }
        bitmap
    }

    fn save_debug_image(w: u32, h: u32, spans: &[Span<u32>], filename: &str) {
        let mut img = image::GrayImage::new(w, h);
        for span in spans {
            for x in span.x.start..span.x.end {
                if x < w && span.y < h {
                    img.put_pixel(x, span.y, image::Luma([255u8]));
                }
            }
        }
        let output_dir =
            std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());
        let path = format!("{}/{}", output_dir, filename);
        let _ = img.save(&path);
    }

    fn assert_rotated_rect_no_gaps(rect_side: u32, canvas_side: u32, angle_deg: f64) {
        let offset = (canvas_side - rect_side) / 2;
        let rect = crate::Rect::new(offset, offset, nz(rect_side), nz(rect_side));
        let spans = rect.into_spans();

        let cx = (canvas_side as f64) / 2.0;
        let cy = (canvas_side as f64) / 2.0;
        let matrix = rotation_matrix(cx, cy, angle_deg);

        let heap = AffineTransformHeap::new(spans, &matrix).unwrap();
        let result: Vec<Span<u32>> = heap.collect();

        save_debug_image(
            canvas_side,
            canvas_side,
            &result,
            &format!("rotate_{}x{}_{}deg.png", rect_side, rect_side, angle_deg as u32),
        );

        for window in result.windows(2) {
            let (a, b) = (&window[0], &window[1]);
            assert!(
                a.y < b.y || (a.y == b.y && a.x.end <= b.x.start),
                "overlapping or out-of-order spans at {angle_deg}°: {:?}",
                window
            );
        }

        let bitmap = spans_to_bitmap(canvas_side, canvas_side, &result);
        let pixel_count: u32 = result.iter().map(|s| s.x.end - s.x.start).sum();
        let expected_area = (rect_side * rect_side) as f64;
        let tolerance = expected_area * 0.15;
        assert!(
            (pixel_count as f64 - expected_area).abs() <= tolerance,
            "pixel count {pixel_count} too far from expected area {expected_area} at {angle_deg}°"
        );

        assert!(
            each_row_is_contiguous(canvas_side, canvas_side, &bitmap),
            "non-contiguous row found at {angle_deg}°\n{}",
            {
                let mut s = String::new();
                for y in 0..canvas_side {
                    let row: String = (0..canvas_side)
                        .map(|x| {
                            if bitmap[(y * canvas_side + x) as usize] {
                                '#'
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    s.push_str(&format!("  {row}\n"));
                }
                s
            }
        );

        assert!(
            flood_fill_connected(canvas_side, canvas_side, &bitmap),
            "disconnected pixels at {angle_deg}°"
        );
    }

    fn scale_and_rotation_matrix(
        cx: f64,
        cy: f64,
        scale: f64,
        angle_deg: f64,
    ) -> Matrix3<f64> {
        let s = Matrix3::new(
            scale,
            0.0,
            cx * (1.0 - scale),
            0.0,
            scale,
            cy * (1.0 - scale),
            0.0,
            0.0,
            1.0,
        );
        let angle = angle_deg.to_radians();
        let cos = angle.cos();
        let sin = angle.sin();
        let r = Matrix3::new(
            cos,
            -sin,
            cx * (1.0 - cos) + cy * sin,
            sin,
            cos,
            cy * (1.0 - cos) - cx * sin,
            0.0,
            0.0,
            1.0,
        );
        r * s
    }

    fn assert_scaled_rotated_no_gaps(
        rect_side: u32,
        scale: f64,
        angle_deg: f64,
        canvas_side: u32,
    ) {
        let offset = (canvas_side - rect_side) / 2;
        let rect = crate::Rect::new(offset, offset, nz(rect_side), nz(rect_side));
        let spans = rect.into_spans();

        let cx = (canvas_side as f64) / 2.0;
        let cy = (canvas_side as f64) / 2.0;
        let matrix = scale_and_rotation_matrix(cx, cy, scale, angle_deg);

        let heap = AffineTransformHeap::new(spans, &matrix).unwrap();
        let result: Vec<Span<u32>> = heap.collect();

        let tag = format!(
            "scale{}_rotate_{}x{}_{}deg",
            scale as u32,
            rect_side,
            rect_side,
            angle_deg as u32
        );
        save_debug_image(canvas_side, canvas_side, &result, &format!("{tag}.png"));
        print_bitmap(canvas_side, canvas_side, &result, &tag);

        for window in result.windows(2) {
            let (a, b) = (&window[0], &window[1]);
            assert!(
                a.y < b.y || (a.y == b.y && a.x.end <= b.x.start),
                "overlapping or out-of-order spans ({tag}): {:?}",
                window
            );
        }

        let bitmap = spans_to_bitmap(canvas_side, canvas_side, &result);
        let pixel_count: u32 = result.iter().map(|s| s.x.end - s.x.start).sum();
        let expected_area = (rect_side * rect_side) as f64 * scale * scale;
        let tolerance = expected_area * 0.15;
        assert!(
            (pixel_count as f64 - expected_area).abs() <= tolerance,
            "pixel count {pixel_count} too far from expected area {expected_area} ({tag})"
        );

        assert!(
            each_row_is_contiguous(canvas_side, canvas_side, &bitmap),
            "non-contiguous row ({tag})"
        );

        assert!(
            flood_fill_connected(canvas_side, canvas_side, &bitmap),
            "disconnected pixels ({tag})"
        );
    }

    #[test]
    fn scale2x_rotate_10x10_30deg() {
        assert_scaled_rotated_no_gaps(10, 2.0, 30.0, 80);
    }

    #[test]
    fn scale2x_rotate_15x15_60deg() {
        assert_scaled_rotated_no_gaps(15, 2.0, 60.0, 120);
    }

    #[test]
    fn scale2x_rotate_10x10_269deg() {
        assert_scaled_rotated_no_gaps(10, 2.0, 269.0, 80);
    }

    #[test]
    fn rotate_rectangle_45deg_no_gaps() {
        assert_rotated_rect_no_gaps(20, 60, 45.0);
    }

    #[test]
    fn rotate_rectangle_150deg_no_gaps() {
        assert_rotated_rect_no_gaps(20, 60, 150.0);
    }

    #[test]
    fn rotate_rectangle_181deg_no_gaps() {
        assert_rotated_rect_no_gaps(20, 60, 181.0);
    }

    #[test]
    fn rotate_rectangle_269deg_no_gaps() {
        assert_rotated_rect_no_gaps(20, 60, 269.0);
    }

    #[test]
    fn rotate_rectangle_271deg_no_gaps() {
        assert_rotated_rect_no_gaps(20, 60, 271.0);
    }

    #[test]
    fn rotate_rectangle_10x10_37deg_no_gaps() {
        assert_rotated_rect_no_gaps(10, 40, 37.0);
    }

    #[test]
    fn rotate_20x20_square_30deg_sorted_disjoint() {
        let rect = crate::Rect::new(15u32, 15, nz(20), nz(20));
        let spans = rect.into_spans();

        let cx = 24.5_f64;
        let cy = 24.5_f64;
        let angle = 30.0_f64.to_radians();
        let cos = angle.cos();
        let sin = angle.sin();

        let matrix = Matrix3::new(
            cos,
            -sin,
            cx * (1.0 - cos) + cy * sin,
            sin,
            cos,
            cy * (1.0 - cos) - cx * sin,
            0.0,
            0.0,
            1.0,
        );

        let heap = AffineTransformHeap::new(spans, &matrix).unwrap();
        let result: Vec<Span<u32>> = heap.collect();

        for window in result.windows(2) {
            let (a, b) = (&window[0], &window[1]);
            assert!(
                a.y < b.y || (a.y == b.y && a.x.end <= b.x.start),
                "overlapping or out-of-order spans: {:?}",
                window
            );
        }

        let pixel_count: u32 = result.iter().map(|s| s.x.end - s.x.start).sum();
        assert!(pixel_count > 300, "too few pixels: {}", pixel_count);
        assert!(pixel_count < 600, "too many pixels: {}", pixel_count);

        print_bitmap(50, 50, &result, "20×20 square rotated 30°");
    }

    #[test]
    fn translate_partially_out_left() {
        let rect = crate::Rect::new(1u32, 1, nz(3), nz(3));
        let spans = rect.into_spans();
        let matrix = Matrix3::new(1.0, 0.0, -2.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0);
        let result: Vec<Span<u32>> =
            AffineTransformHeap::new(spans, &matrix).unwrap().collect();
        assert_eq!(
            result,
            vec![Span::new(0..2, 1), Span::new(0..2, 2), Span::new(0..2, 3)]
        );
    }

    #[test]
    fn translate_partially_out_right() {
        let rect = crate::Rect::new(1u32, 1, nz(3), nz(3));
        let spans = rect.into_spans();
        let matrix = Matrix3::new(1.0, 0.0, 4.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0);
        let result: Vec<Span<u32>> =
            AffineTransformHeap::new(spans, &matrix).unwrap().collect();
        assert_eq!(
            result,
            vec![Span::new(5..8, 1), Span::new(5..8, 2), Span::new(5..8, 3)]
        );
    }

    #[test]
    fn translate_partially_out_top() {
        let rect = crate::Rect::new(1u32, 1, nz(3), nz(3));
        let spans = rect.into_spans();
        let matrix = Matrix3::new(1.0, 0.0, 0.0, 0.0, 1.0, -2.0, 0.0, 0.0, 1.0);
        let result: Vec<Span<u32>> =
            AffineTransformHeap::new(spans, &matrix).unwrap().collect();
        assert_eq!(result, vec![Span::new(1..4, 0), Span::new(1..4, 1)]);
    }

    #[test]
    fn translate_partially_out_bottom() {
        let rect = crate::Rect::new(1u32, 1, nz(3), nz(3));
        let spans = rect.into_spans();
        let matrix = Matrix3::new(1.0, 0.0, 0.0, 0.0, 1.0, 4.0, 0.0, 0.0, 1.0);
        let result: Vec<Span<u32>> =
            AffineTransformHeap::new(spans, &matrix).unwrap().collect();
        assert_eq!(
            result,
            vec![Span::new(1..4, 5), Span::new(1..4, 6), Span::new(1..4, 7)]
        );
    }

    #[test]
    fn translate_partially_out_corner() {
        let rect = crate::Rect::new(1u32, 1, nz(3), nz(3));
        let spans = rect.into_spans();
        let matrix = Matrix3::new(1.0, 0.0, -2.0, 0.0, 1.0, -2.0, 0.0, 0.0, 1.0);
        let result: Vec<Span<u32>> =
            AffineTransformHeap::new(spans, &matrix).unwrap().collect();
        assert_eq!(result, vec![Span::new(0..2, 0), Span::new(0..2, 1)]);
    }

    #[test]
    fn rotate_100x100_at_offset() {
        let rect = crate::Rect::new(100u32, 100, nz(100), nz(100));
        let spans = rect.into_spans();

        let cx = 150.0_f64;
        let cy = 150.0_f64;
        let matrix = rotation_matrix(cx, cy, 45.0);

        let heap = AffineTransformHeap::new(spans, &matrix).unwrap();
        let bounds = heap.bounds();

        assert!(bounds.x >= 78 && bounds.x <= 82, "bounds.x={}", bounds.x);
        assert!(bounds.y >= 78 && bounds.y <= 82, "bounds.y={}", bounds.y);
        assert!(
            bounds.x + bounds.width.get() >= 218 && bounds.x + bounds.width.get() <= 224,
            "right edge={}",
            bounds.x + bounds.width.get()
        );
        assert!(
            bounds.y + bounds.height.get() >= 218 && bounds.y + bounds.height.get() <= 224,
            "bottom edge={}",
            bounds.y + bounds.height.get()
        );

        let result: Vec<Span<u32>> = heap.collect();

        for span in &result {
            assert!(
                span.x.start >= bounds.x,
                "span {:?} starts before bounds.x={}",
                span,
                bounds.x
            );
            assert!(
                span.x.end <= bounds.x + bounds.width.get(),
                "span {:?} extends past right edge={}",
                span,
                bounds.x + bounds.width.get()
            );
            assert!(span.y >= bounds.y);
            assert!(span.y < bounds.y + bounds.height.get());
        }

        let pixel_count: u32 = result.iter().map(|s| s.x.end - s.x.start).sum();
        let expected = 100 * 100;
        let tolerance = (expected as f64 * 0.15) as u32;
        assert!(
            (pixel_count as i64 - expected as i64).unsigned_abs() <= tolerance as u64,
            "pixel count {pixel_count} too far from expected {expected}"
        );
    }
}
