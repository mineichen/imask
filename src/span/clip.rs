use std::fmt::Debug;
use std::num::NonZeroU32;
use std::ops::{Add, Sub};

use crate::{ImageDimension, NonZeroRange, Rect, SignedNonZeroable, Span};

pub struct ClipSpanIter<TIter, T: SignedNonZeroable> {
    parent: TIter,
    clip: Rect<T>,
    output_bounds: Rect<u32>,
    pending: Option<Span<T>>,
}

impl<TIter, T> ClipSpanIter<TIter, T>
where
    TIter: Iterator<Item = Span<T>> + ImageDimension,
    T: SignedNonZeroable
        + TryFrom<u32, Error: Debug>
        + Ord
        + Add<Output = T>
        + Sub<Output = T>
        + Copy
        + Debug,
{
    pub fn new(mut parent: TIter, roi: Rect<u32>) -> Self {
        let pb = parent.bounds();

        let x_start = pb.x.max(roi.x);
        let y_start = pb.y.max(roi.y);
        let x_end = (pb.x + pb.width.get()).min(roi.x + roi.width.get());
        let y_end = (pb.y + pb.height.get()).min(roi.y + roi.height.get());

        let output_bounds = Rect::new(
            x_start,
            y_start,
            NonZeroU32::new(x_end - x_start).expect("Empty x intersection"),
            NonZeroU32::new(y_end - y_start).expect("Empty y intersection"),
        );

        let clip = Rect::new(
            T::try_from(x_start).expect("x_start overflow"),
            T::try_from(y_start).expect("y_start overflow"),
            T::create_non_zero(T::try_from(x_end - x_start).expect("width overflow"))
                .expect("width must be non-zero"),
            T::create_non_zero(T::try_from(y_end - y_start).expect("height overflow"))
                .expect("height must be non-zero"),
        );

        let pending = parent.find(|span| span.y >= clip.y);

        Self {
            parent,
            clip,
            output_bounds,
            pending,
        }
    }
}

impl<TIter: ImageDimension, T: SignedNonZeroable> ImageDimension for ClipSpanIter<TIter, T> {
    fn bounds(&self) -> Rect<u32> {
        self.output_bounds
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.output_bounds.width
    }
}

impl<TIter: Iterator<Item = Span<T>>, T: SignedNonZeroable + Ord + Debug + Add<Output = T> + Copy>
    Iterator for ClipSpanIter<TIter, T>
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let span = self.pending.take().or_else(|| self.parent.next())?;
            if span.y >= self.clip.len_y().into() {
                return None;
            }
            let start = span.x.start.max(self.clip.x);
            let end = span.x.end.min(self.clip.len_x().into());
            if let Ok(range) = NonZeroRange::try_from(start..end) {
                return Some(Span::new(range, span.y));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::{ImageDimension, ImaskSet, Rect};

    use super::*;

    const NON_ZERO_5: NonZeroU32 = NonZeroU32::new(5).unwrap();
    const NON_ZERO_10: NonZeroU32 = NonZeroU32::new(10).unwrap();
    const NON_ZERO_100: NonZeroU32 = NonZeroU32::new(100).unwrap();

    #[test]
    fn smaller_bounds_do_crop() {
        let src = Rect::new(10u32, 10, NON_ZERO_10, NON_ZERO_10);
        let bounds = Rect::new(12u32, 12, NON_ZERO_5, NON_ZERO_5);
        let expected = bounds.into_spans().collect::<Vec<_>>();
        let clipped = ClipSpanIter::new(src.into_spans(), bounds).collect::<Vec<_>>();
        assert_eq!(expected, clipped);
    }

    #[test]
    fn bigger_bounds_have_no_effect() {
        let src = Rect::new(10u32, 10, NON_ZERO_10, NON_ZERO_10);
        let iter = src.into_spans();
        let expected = iter.clone().collect::<Vec<_>>();
        let bounds = Rect::new(0u32, 0, NON_ZERO_100, NON_ZERO_100);
        let clipped = ClipSpanIter::new(iter, bounds).collect::<Vec<_>>();
        assert_eq!(expected, clipped);
    }

    #[test]
    fn with_no_overlapping_parts() {
        let src = Rect::new(10u32, 10, NON_ZERO_10, NON_ZERO_10);
        let iter = src.into_spans();
        let expected = iter.clone().collect::<Vec<_>>();
        let bounds = Rect::new(0u32, 0, NON_ZERO_100, NON_ZERO_100);
        let clipped = ClipSpanIter::new(
            iter.union(
                Rect {
                    x: 100u32,
                    y: 10,
                    width: NON_ZERO_10,
                    height: NON_ZERO_10,
                }
                .into_spans(),
            ),
            bounds,
        )
        .collect::<Vec<_>>();
        assert_eq!(expected, clipped);
    }

    #[test]
    fn clip_returns_intersection_bounds() {
        let source = Rect::new(0u32, 0, NON_ZERO_100, NON_ZERO_100);
        let roi = Rect::new(
            10u32,
            10,
            NonZeroU32::new(80).unwrap(),
            NonZeroU32::new(110).unwrap(),
        );
        let expected_bounds = Rect::new(
            10u32,
            10,
            NonZeroU32::new(80).unwrap(),
            NonZeroU32::new(90).unwrap(),
        );

        let clipped = ClipSpanIter::new(source.into_spans(), roi);
        assert_eq!(expected_bounds, clipped.bounds());

        let spans: Vec<_> = clipped.collect();
        let expected_spans: Vec<_> = expected_bounds.into_spans().collect();
        assert_eq!(expected_spans, spans);
    }
}
