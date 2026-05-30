use std::fmt::Debug;
use std::ops::{Add, Mul, Sub};

use crate::{CreateRange, ImageDimension, Rect, SignedNonZeroable, Span};

pub struct SpanIntoRangesIter<TIter: Iterator, TOut: CreateRange<Item: SignedNonZeroable>> {
    parent: TIter,
    bounds: Rect<TOut::Item>,
    static_offset: TOut::Item,
    unreleased: Option<TOut>,
}

impl<TIter: Iterator + ImageDimension, TOut: CreateRange<Item: SignedNonZeroable>> ImageDimension
    for SpanIntoRangesIter<TIter, TOut>
{
    fn bounds(&self) -> crate::Rect<u32> {
        self.parent.bounds()
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.parent.width()
    }
}

impl<TIter: Iterator + ImageDimension, TOut: CreateRange<Item: SignedNonZeroable>>
    SpanIntoRangesIter<TIter, TOut>
where
    TOut::Item: TryFrom<u32, Error: Debug>,
{
    pub(crate) fn new(parent: TIter) -> Self {
        let bounds = parent.bounds();
        let static_offset = (bounds.x + bounds.y * bounds.width.get())
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
            let offset = next.y * self.bounds.width.into();
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
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::ops::Range;

    use super::*;
    use crate::{ImaskSet, Rect};

    const NON_ZERO_10: NonZeroU32 = NonZeroU32::new(10).unwrap();

    #[test]
    fn summarize_multiline() {
        let rect = Rect::new(10u32, 10, NON_ZERO_10, NON_ZERO_10);

        let via_span = rect.clone().into_spans().into_ranges::<Range<u32>>();
        assert_eq!(rect, via_span.bounds());
        let via_span = via_span.collect::<Vec<_>>();
        assert_eq!(vec![0..100], via_span);
    }
}
