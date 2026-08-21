use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::binary_heap::PeekMut;
use std::fmt::Debug;

use crate::{CreateRange, ImageDimension, MaybeResult, NonZeroRange, PipelineError, Rect, Span};

pub struct UnionAll<I: Iterator> {
    heap: BinaryHeap<PendingIter<I>>,
    accumulator: Option<I::Item>,
    roi: Rect<u32>,
}

impl<I: Iterator<Item: Ord>> UnionAll<I> {
    /// Merge a set of possibly-fallible iterators.
    ///
    /// Each item of `iters` is a [`MaybeResult`]: an infallible [`Iterator`]
    /// ([`ImageDimension`]), a [`Result`] of one, or any other infallible
    /// value that can be converted into an iterator (see [`MaybeResult`]).
    /// The first error encountered while seeding the merge is propagated.
    pub fn new<S>(iters: impl IntoIterator<Item = S>) -> Result<Self, PipelineError>
    where
        S: MaybeResult<Ok: IntoIterator<IntoIter = I>, Err: Into<PipelineError>>,
        I: ImageDimension,
    {
        let mut iters = iters
            .into_iter()
            .map(|item| {
                item.into_result().map_err(|e| e.into()).and_then(|x| {
                    let mut iter = x.into_iter();
                    let bounds = iter.bounds();
                    let first = iter.next().ok_or(PipelineError::Empty)?;
                    Ok((
                        bounds,
                        PendingIter {
                            pending: Some(first),
                            iter,
                        },
                    ))
                })
            })
            .filter(|x| !matches!(x, Err(PipelineError::Empty)));
        let mut heap = BinaryHeap::with_capacity(iters.size_hint().0);
        let (mut roi, first) = iters.next().ok_or(PipelineError::Empty)??;

        heap.push(first);
        for pending in iters {
            let (roi_i, item) = pending?;
            heap.push(item);
            roi = roi.bounds(&roi_i);
        }

        Ok(Self {
            heap,
            accumulator: None,
            roi,
        })
    }
}

impl<I: Iterator> ImageDimension for UnionAll<I> {
    fn bounds(&self) -> crate::Rect<u32> {
        self.roi
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.roi.width
    }
}

impl<I, T> Iterator for UnionAll<I>
where
    I: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug,
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Span<T>> {
        loop {
            let item = match self.heap.peek_mut() {
                Some(mut entry) => {
                    let item = entry.pending.take().unwrap();
                    entry.pending = entry.iter.next();
                    if entry.pending.is_none() {
                        PeekMut::pop(entry);
                    }
                    item
                }
                None => return self.accumulator.take(),
            };

            match self.accumulator.take() {
                None => {
                    self.accumulator = Some(item);
                }
                Some(acc) => {
                    if item.y == acc.y && item.x.start <= acc.x.end {
                        self.accumulator = Some(Span {
                            x: NonZeroRange::new_debug_checked_zeroable(
                                acc.x.start,
                                acc.x.end.max(item.x.end),
                            ),
                            y: acc.y,
                        });
                    } else {
                        self.accumulator = Some(item);
                        return Some(acc);
                    }
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let accumulator = self.accumulator.is_some() as usize;
        let mut lo = accumulator;
        let mut hi = Some(accumulator);
        for entry in self.heap.iter() {
            let pending = entry.pending.is_some() as usize;
            let (e_lo, e_hi) = entry.iter.size_hint();
            lo = lo.max(e_lo.saturating_add(pending));
            hi = match (hi, e_hi) {
                (Some(x), Some(h)) => Some(x.max(h.saturating_add(pending))),
                _ => None,
            };
        }
        (lo, hi)
    }
}

struct PendingIter<I: Iterator> {
    pending: Option<I::Item>,
    iter: I,
}

impl<I: Iterator<Item: Ord>> PartialEq for PendingIter<I> {
    fn eq(&self, other: &Self) -> bool {
        self.pending == other.pending
    }
}

impl<I: Iterator<Item: Ord>> Eq for PendingIter<I> {}

impl<I: Iterator<Item: Ord>> PartialOrd for PendingIter<I> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<I: Iterator<Item: Ord>> Ord for PendingIter<I> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.pending, &other.pending) {
            (None, None) => Ordering::Equal,
            (_, None) => Ordering::Greater,
            (None, _) => Ordering::Less,
            (Some(a), Some(b)) => b.cmp(a),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::{ImaskSet, SortedRanges};

    const BOUNDS: Rect<u32> = Rect::new(
        0,
        0,
        NonZeroU32::new(100).unwrap(),
        NonZeroU32::new(100).unwrap(),
    );

    #[test]
    fn empty() {
        let result = UnionAll::new(std::iter::empty::<SortedRanges<u32>>());
        assert!(matches!(result, Err(PipelineError::Empty)));
    }

    #[test]
    fn single_iterator() {
        let span = Span::new(0u16..10, 0);

        let iter = std::iter::once(SortedRanges::from(span));
        assert_eq!(vec![span], UnionAll::new(iter).unwrap().collect::<Vec<_>>());
    }

    #[test]
    fn two_non_overlapping() {
        let a: SortedRanges<u16> = Span::new(0..5, 0).into();
        let b: SortedRanges<u16> = Span::new(10..15, 0).into();
        assert_eq!(
            vec![Span::new(0..5, 0u16), Span::new(10..15, 0u16),],
            UnionAll::new([a, b]).unwrap().collect::<Vec<_>>()
        );
    }

    #[test]
    fn two_overlapping() {
        let a: SortedRanges<u16> = Span::new(0..10, 0).into();
        let b: SortedRanges<u16> = Span::new(5..15, 0).into();
        assert_eq!(
            vec![Span::new(0..15, 0u16)],
            UnionAll::new([a, b]).unwrap().collect::<Vec<_>>()
        );
    }

    #[test]
    fn three_overlapping() {
        let a: SortedRanges<u16> = Span::new(0..5, 0).into();
        let b: SortedRanges<u16> = Span::new(3..8, 0).into();
        let c: SortedRanges<u16> = Span::new(6..12, 0).into();
        assert_eq!(
            vec![Span::new(0..12, 0u16)],
            UnionAll::new([a, b, c]).unwrap().collect::<Vec<_>>()
        );
    }

    #[test]
    fn same_spans() {
        let span = Span::new(0..10, 0);
        let a: SortedRanges<u16> = span.into();
        let b: SortedRanges<u16> = span.into();
        assert_eq!(
            vec![span],
            UnionAll::new([a, b]).unwrap().collect::<Vec<_>>()
        );
    }

    #[test]
    fn different_lines() {
        let a = Span::new(0..10u16, 0);
        let b = Span::new(0..10u16, 1);
        assert_eq!(
            vec![a, b],
            UnionAll::new([SortedRanges::from(a), SortedRanges::from(b)])
                .unwrap()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn some_empty_iterators() {
        let a = Span::new(0..10, 0);
        let c = Span::new(5..15, 0);
        assert_eq!(
            vec![Span::new(0..15, 0)],
            UnionAll::new([
                vec!(a).with_roi(a.into()),
                vec![].with_roi(a.into()),
                vec![c].with_roi(c.into())
            ])
            .unwrap()
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn complex_merge() {
        let a = SortedRanges::<u16>::try_from_span_iter(
            [Span::new(0..5, 0u16), Span::new(0..5, 1)].with_roi(BOUNDS),
        );
        let b = SortedRanges::<u16>::try_from_span_iter(
            [Span::new(3..8, 0u16), Span::new(0..5, 2)].with_roi(BOUNDS),
        );
        let c = SortedRanges::<u16>::try_from_span_iter(
            [Span::new(6..10, 0u16), Span::new(3..8, 1)].with_roi(BOUNDS),
        );
        assert_eq!(
            vec![
                Span::new(0..10, 0u16),
                Span::new(0..8, 1u16),
                Span::new(0..5, 2u16),
            ],
            UnionAll::new([a, b, c]).unwrap().collect::<Vec<_>>()
        );
    }

    #[test]
    fn via_imaskset() {
        let a = SortedRanges::from(Span::new(0..10, 0));
        let b = SortedRanges::from(Span::new(5..15, 0));
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..15).unwrap(), 0u16)],
            [
                Result::<_, std::convert::Infallible>::Ok(a),
                Result::<_, std::convert::Infallible>::Ok(b)
            ]
            .union_all()
            .unwrap()
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn result_error_propagates() {
        let item_ok: SortedRanges<u32> = Span::new(0..10, 0).into();
        let item_err = u8::try_from(256).unwrap_err();
        let result = UnionAll::new([Ok(item_ok.into_iter()), Err(item_err)].with_roi(BOUNDS));
        assert!(matches!(result, Err(PipelineError::IncompatibleSize(_))));
    }

    #[test]
    fn sorted_ranges_via_union_all_new() {
        let a = SortedRanges::from(Span::new(0u16..10, 0));
        let b = SortedRanges::from(Span::new(5..15, 0));
        assert_eq!(
            vec![Span::new(0..15, 0)],
            UnionAll::new([a, b]).unwrap().collect::<Vec<_>>()
        );
    }
}
