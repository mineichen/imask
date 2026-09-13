use std::fmt::Debug;
use std::ops::{Add, Mul, Sub};

use crate::{CreateRange, ImageDimension, Roi, SignedNonZeroable, Span};

pub struct SpanIntoRangesIter<TIter: Iterator, TOut: CreateRange<Item: SignedNonZeroable>> {
    parent: TIter,
    bounds: Roi<TOut::Item>,
    static_offset: TOut::Item,
    unreleased: Option<TOut>,
}

impl<TIter: Iterator + ImageDimension, TOut: CreateRange<Item: SignedNonZeroable>> ImageDimension
    for SpanIntoRangesIter<TIter, TOut>
{
    fn roi(&self) -> crate::Roi<u32> {
        self.parent.roi()
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.parent.width()
    }
}

impl<TIter: Iterator + ImageDimension, TOut: CreateRange<Item: SignedNonZeroable>>
    SpanIntoRangesIter<TIter, TOut>
where
    TOut::Item: TryFrom<u32, Error: Debug> + Ord + Debug,
{
    pub(crate) fn new(parent: TIter) -> Self {
        let bounds = parent.roi();
        let static_offset = (bounds.x.start + bounds.y.start * bounds.width().get())
            .try_into()
            .expect("Cant calculate static offset");
        let bounds = bounds.try_cast::<TOut::Item>().unwrap();
        Self {
            parent,
            bounds,
            static_offset,
            unreleased: None,
        }
    }
}

impl<
    TIter: Iterator<Item = Span<T>> + ImageDimension,
    TOut: CreateRange<Item = T>,
    T: Copy
        + Mul<Output = T>
        + Add<Output = T>
        + Sub<Output = T>
        + Eq
        + SignedNonZeroable
        + Debug
        + PartialOrd,
> Iterator for SpanIntoRangesIter<TIter, TOut>
{
    type Item = TOut;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let Some(next) = self.parent.next() else {
                return self.unreleased.take();
            };
            let offset = next.y * self.bounds.width().into();
            let start = offset + next.x.start - self.static_offset;
            let end = offset + next.x.end - self.static_offset;
            if let Some(unrel) = &mut self.unreleased {
                debug_assert!(
                    unrel.end() <= start,
                    "Non-monotonic 1D range: prev_end={:?} > start={:?} (span y={:?}, x=[{:?},{:?}])",
                    unrel.end(),
                    start,
                    next.y,
                    next.x.start,
                    next.x.end,
                );
                if unrel.end() == start {
                    *unrel = TOut::new_debug_checked_zeroable(unrel.start(), end);
                } else {
                    let mut successor = TOut::new_debug_checked_zeroable(start, end);
                    std::mem::swap(unrel, &mut successor);
                    return Some(successor);
                }
            } else {
                self.unreleased = Some(TOut::new_debug_checked_zeroable(start, end))
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lo, hi) = self.parent.size_hint();
        let unreleased = self.unreleased.is_some() as usize;
        let lo = usize::from(lo.saturating_add(unreleased) > 0);
        (lo, hi.map(|h| h.saturating_add(unreleased)))
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::*;
    use crate::{ImaskSet, Roi};

    #[test]
    fn summarize_multiline() {
        let rect = Roi::new(10u32..20, 10..20);

        let via_span = rect.into_spans().into_ranges::<Range<u32>>();
        assert_eq!(rect, via_span.roi());
        let via_span = via_span.collect::<Vec<_>>();
        assert_eq!(vec![0..100], via_span);
    }
}
