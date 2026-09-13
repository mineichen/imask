use std::fmt::Debug;
use std::ops::{Add, Sub};

use crate::{ImageDimension, NonZeroRange, Roi, SignedNonZeroable, Span};

pub struct ClipSpanIter<TIter, T: SignedNonZeroable> {
    parent: TIter,
    clip: Roi<T>,
    output_bounds: Roi<u32>,
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
    pub fn new(mut parent: TIter, roi: impl Into<Roi<u32>>) -> Self {
        let roi = roi.into();
        let pb = parent.roi();

        let x_start = pb.x.start.max(roi.x.start);
        let y_start = pb.y.start.max(roi.y.start);
        let x_end = pb.x.end.min(roi.x.end);
        let y_end = pb.y.end.min(roi.y.end);

        let output_bounds = Roi {
            x: NonZeroRange::new(x_start..x_end),
            y: NonZeroRange::new(y_start..y_end),
        };
        // This proofs, that ClipSpanIter::new should be fallible
        // We also have the problem of calling parent.next() after None
        debug_assert_eq!(pb.intersection(&roi), Some(output_bounds));

        let clip = Roi {
            x: NonZeroRange::new(
                T::try_from(x_start).expect("x_start overflow")
                    ..T::try_from(x_end).expect("x_end overflow"),
            ),
            y: NonZeroRange::new(
                T::try_from(y_start).expect("y_start overflow")
                    ..T::try_from(y_end).expect("y_end overflow"),
            ),
        };

        let pending = parent.find(|span| span.y >= clip.y.start);

        Self {
            parent,
            clip,
            output_bounds,
            pending,
        }
    }
}

impl<TIter: ImageDimension, T: SignedNonZeroable> ImageDimension for ClipSpanIter<TIter, T> {
    fn roi(&self) -> Roi<u32> {
        self.output_bounds
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.output_bounds.width()
    }
}

impl<TIter: Iterator<Item = Span<T>>, T: SignedNonZeroable + Ord + Debug + Add<Output = T> + Copy>
    Iterator for ClipSpanIter<TIter, T>
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let span = self.pending.take().or_else(|| self.parent.next())?;
            if span.y >= self.clip.y.end {
                return None;
            }
            let range = span.x.intersection(&self.clip.x);
            if let Some(range) = range {
                return Some(Span {
                    x: range,
                    y: span.y,
                });
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // todo: Can be improved: If parent is inbound, parent bounds can be returned
        let (_, hi) = self.parent.size_hint();
        let pending = self.pending.is_some() as usize;
        (0, hi.map(|h| h.saturating_add(pending)))
    }
}

#[cfg(test)]
mod tests {

    use crate::{ImaskSet, Roi};

    use super::*;

    #[test]
    fn smaller_bounds_do_crop() {
        let src = Roi::new(10u32..20, 10..20);
        let bounds = Roi::new(12u32..17, 12..17);
        let expected = bounds.into_spans().collect::<Vec<_>>();
        let clipped = ClipSpanIter::new(src.into_spans(), bounds).collect::<Vec<_>>();
        assert_eq!(expected, clipped);
    }

    #[test]
    fn bigger_bounds_have_no_effect() {
        let src = Roi::new(10u32..20, 10..20);
        let iter = src.into_spans();
        let expected = iter.clone().collect::<Vec<_>>();
        let bounds = Roi::new(0u32..100, 0..100);
        let clipped = ClipSpanIter::new(iter, bounds).collect::<Vec<_>>();
        assert_eq!(expected, clipped);
    }

    #[test]
    fn with_no_overlapping_parts() {
        let src = Roi::new(10u32..20, 10..20);
        let iter = src.into_spans();
        let expected = iter.clone().collect::<Vec<_>>();
        let bounds = Roi::new(0u32..100, 0..100);
        let clipped = ClipSpanIter::new(
            iter.union(Roi::new(100u32..110, 10..20).into_spans()),
            bounds,
        )
        .collect::<Vec<_>>();
        assert_eq!(expected, clipped);
    }

    #[test]
    fn clip_returns_intersection_bounds() {
        let source = Roi::new(0u32..100, 0..100);
        let roi = Roi::new(10u32..90, 10..120);
        let expected_bounds = Roi::new(10u32..90, 10..100);

        let clipped = ClipSpanIter::new(source.into_spans(), roi);
        assert_eq!(expected_bounds, clipped.roi());

        let spans: Vec<_> = clipped.collect();
        let expected_spans: Vec<_> = expected_bounds.into_spans().collect();
        assert_eq!(expected_spans, spans);
    }
}
