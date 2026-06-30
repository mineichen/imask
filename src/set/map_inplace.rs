use std::{
    cell::RefCell, collections::VecDeque, fmt::Debug, iter::FusedIterator, num::NonZero,
    ops::RangeInclusive, rc::Rc,
};

use crate::{
    CreateRange, ImageDimension, NonZeroRange, RangeToOffsetsIter, SortedRanges,
    SortedRangesSpanIter, Span, SpanToOffsetsIter, UncheckedCast,
};

impl<TIncluded, TExcluded> SortedRanges<TIncluded, TExcluded> {
    /// Transform the ranges in-place using a closure.
    /// The closure receives a SourceIterator and returns an iterator of `RangeInclusive<u64>`.
    /// Returns Some(SortedRanges) if non-empty, None if empty.
    /// ```
    /// use std::ops::RangeInclusive;
    /// use imask::{Rect, SortedRanges, SourceIterator, ImaskSet};
    /// use std::num::NonZero;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let size = const { NonZero::new(1000u32).unwrap() };
    /// let source = [10u32..20, 30..45, 50..60].with_bounds(size, size);
    /// let ranges = SortedRanges::<u16, u16>::try_from_ordered_iter(source)?;
    /// let ranges = ranges.map_inplace(|iter| {
    ///     iter.map(|x| {
    ///         let (start, end) = x.into_inner();
    ///         (start+5)..=(end + 5)
    ///     })
    /// }).expect("Is not empty");
    /// assert_eq!(
    ///     vec!(15u64..25, 35..50, 55..65),
    ///     ranges.iter_roi_owned::<std::ops::Range<u64>>().collect::<Vec<_>>()
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn map_inplace<TIter, TFun>(self, f: TFun) -> Option<Self>
    where
        TIter: Iterator<Item = RangeInclusive<u64>>,
        TFun: FnOnce(SourceIterator<TIncluded, TExcluded>) -> TIter,
        TIncluded: TryFrom<u64, Error: Debug> + Clone,
        TExcluded: TryFrom<u64, Error: Debug> + Clone,
    {
        let original_len = self.included.len();
        // Rc is required, because we cannot restrict TIter by the Lifetime of the FnOnce-argument
        // When working with pointers, it was difficult to forbid the Lambda use to std::mem::swap...
        // If this happens, `map_inplace` panics
        let cell = Rc::new(RefCell::new((self, 0usize)));

        let source = SourceIterator {
            cell: cell.clone(),
            offset: 0,
            original_len,
        };

        let items = f(source);
        let offsets_iter = RangeToOffsetsIter::<_, TIncluded, TExcluded>::new(items);
        let mut cache: VecDeque<(TExcluded, TIncluded)> = VecDeque::new();
        let mut write_pos = 0;

        let write_tuple = |col: &mut SortedRanges<_, _>, (excl, incl), write_pos: &mut usize| {
            if *write_pos < col.included.len() {
                col.excluded[*write_pos] = excl;
                col.included[*write_pos] = incl;
            } else {
                col.excluded.push(excl);
                col.included.push(incl);
            }
            *write_pos += 1;
        };

        for tuple in offsets_iter {
            let mut x = cell.borrow_mut();
            let (read_pos, col) = (x.1, &mut x.0);
            if (write_pos < read_pos || read_pos >= original_len) && cache.is_empty() {
                write_tuple(col, tuple, &mut write_pos);
            } else {
                cache.push_back(tuple);
                while (write_pos < read_pos || read_pos >= original_len)
                    && let Some(tuple) = cache.pop_front()
                {
                    write_tuple(col, tuple, &mut write_pos)
                }
            }
        }
        let not_empty = {
            let mut x = cell.borrow_mut();
            let col = &mut x.0;
            while let Some(tuple) = cache.pop_front() {
                write_tuple(col, tuple, &mut write_pos);
            }

            col.included.truncate(write_pos);
            col.excluded.truncate(write_pos);
            !x.0.included.is_empty()
        };

        not_empty.then(move|| {
            Rc::try_unwrap(cell).expect("You mustn't move the SourceIterator outside the lambda provided to map_inplace").into_inner().0
        })
    }

    /// Transform the ranges in-place using a closure that operates on `Span<u64>` items.
    /// The closure receives a span iterator (backed by the same `SortedRanges`) and returns
    /// an iterator of `Span<u64>`. This preserves row boundaries and works correctly with
    /// full-width multiline masks.
    ///
    /// ```
    /// use std::num::NonZero;
    /// use imask::{
    ///     ImageDimension, ImaskSet, Rect, SortedRanges, SortedRangesSpanIter, Span,
    ///     NonZeroRange,
    /// };
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let width = NonZero::new(100u32).unwrap();
    /// let height = NonZero::new(200u32).unwrap();
    /// // Two rows, each full width
    /// let spans = [
    ///     Span::new(NonZeroRange::try_from(0..100)?, 0u64),
    ///     Span::new(NonZeroRange::try_from(50..100)?, 1u64),
    /// ].with_bounds(width, height);
    /// let ranges = SortedRanges::<u32, u32>::try_from_span_iter(spans)?;
    ///
    /// let result = ranges.map_span_inplace(|source| {
    ///     let extra = vec![
    ///         Span::new(NonZeroRange::try_from(0..50).unwrap(), 1u64),
    ///     ];
    ///     source.union(extra)
    /// }).expect("Non-empty");
    ///
    /// assert_eq!(1, result.len());
    /// let out_spans: Vec<_> = result.spans::<u64>().collect();
    /// assert_eq!(out_spans, vec![
    ///     Span::new(NonZeroRange::try_from(0..100).unwrap(), 0u64),
    ///     Span::new(NonZeroRange::try_from(0..100).unwrap(), 1u64),
    /// ]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn map_span_inplace<TIter, TFun>(self, f: TFun) -> Option<Self>
    where
        TIter: Iterator<Item = Span<u64>>,
        TFun: FnOnce(SortedRangesSpanIter<SourceIterator<TIncluded, TExcluded>>) -> TIter,
        TIncluded: TryFrom<u64, Error: Debug> + Clone + UncheckedCast<u64>,
        TExcluded: TryFrom<u64, Error: Debug> + Clone + UncheckedCast<u64>,
    {
        let original_len = self.included.len();
        let width = self.bounds.width.get();
        let offset_x = self.bounds.x as u64;
        let offset_y = self.bounds.y as u64;
        let cell = Rc::new(RefCell::new((self, 0usize)));

        let source = SourceIterator {
            cell: cell.clone(),
            offset: 0,
            original_len,
        };

        let source_spans = SortedRangesSpanIter::new(source);
        let items = f(source_spans);
        // The closure works with global spans (containing bounds offset).
        // Convert them to local spans for writing back.
        let items = items.map(move |span| {
            Span::new(
                NonZeroRange::new_debug_checked_zeroable(
                    span.x.start - offset_x,
                    span.x.end - offset_x,
                ),
                span.y - offset_y,
            )
        });
        let offsets_iter = SpanToOffsetsIter::<_, TIncluded, TExcluded>::new(items, width);
        let mut cache: VecDeque<(TExcluded, TIncluded)> = VecDeque::new();
        let mut write_pos = 0;

        let write_tuple = |col: &mut SortedRanges<_, _>, (excl, incl), write_pos: &mut usize| {
            if *write_pos < col.included.len() {
                col.excluded[*write_pos] = excl;
                col.included[*write_pos] = incl;
            } else {
                col.excluded.push(excl);
                col.included.push(incl);
            }
            *write_pos += 1;
        };

        for tuple in offsets_iter {
            let mut x = cell.borrow_mut();
            let read_pos = x.1;
            let col = &mut x.0;
            if (write_pos < read_pos || read_pos >= original_len) && cache.is_empty() {
                write_tuple(col, tuple, &mut write_pos);
            } else {
                cache.push_back(tuple);
                while (write_pos < read_pos || read_pos >= original_len)
                    && let Some(tuple) = cache.pop_front()
                {
                    write_tuple(col, tuple, &mut write_pos)
                }
            }
        }
        let not_empty = {
            let mut x = cell.borrow_mut();
            let col = &mut x.0;
            while let Some(tuple) = cache.pop_front() {
                write_tuple(col, tuple, &mut write_pos);
            }

            col.included.truncate(write_pos);
            col.excluded.truncate(write_pos);
            !x.0.included.is_empty()
        };

        not_empty.then(move || {
            Rc::try_unwrap(cell).expect("You mustn't move the SourceIterator outside the lambda provided to map_span_inplace").into_inner().0
        })
    }
}

pub struct SourceIterator<TIncluded, TExcluded> {
    cell: Rc<RefCell<(SortedRanges<TIncluded, TExcluded>, usize)>>,
    offset: u64,
    original_len: usize,
}

impl<TIncluded, TExcluded> FusedIterator for SourceIterator<TIncluded, TExcluded> where
    Self: Iterator
{
}

impl<TIncluded, TExcluded> ImageDimension for SourceIterator<TIncluded, TExcluded> {
    fn width(&self) -> NonZero<u32> {
        self.cell.borrow().0.bounds.width
    }

    fn bounds(&self) -> crate::Rect<u32> {
        self.cell.borrow().0.bounds
    }
}

impl<TIncluded, TExcluded> Iterator for SourceIterator<TIncluded, TExcluded>
where
    TIncluded: UncheckedCast<u64>,
    TExcluded: UncheckedCast<u64>,
{
    type Item = RangeInclusive<u64>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut x = self.cell.borrow_mut();
        let (col, read_pos) = &mut *x;
        if *read_pos >= self.original_len {
            return None;
        }
        let exclude = (*col.excluded.get(*read_pos)?).cast_unchecked();
        self.offset += exclude;

        let include = (*col.included.get(*read_pos)?).cast_unchecked();
        let out_range =
            RangeInclusive::new_debug_checked(self.offset, NonZero::new(include).unwrap());
        self.offset += include;
        *read_pos += 1;

        Some(out_range)
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZero, ops::Range};

    use crate::{ImageDimension, ImaskSet, NonZeroRange, Rect, SortedRanges, Span};

    #[test]
    fn full_width_multiline_mask_union() {
        let width = NonZero::new(100u32).unwrap();
        let height = NonZero::new(200u32).unwrap();
        // Two rows, each full width
        let spans = [
            Span::new(NonZeroRange::try_from(0..100).unwrap(), 0u64),
            Span::new(NonZeroRange::try_from(0..100).unwrap(), 1u64),
        ]
        .with_bounds(width, height);
        let ranges = SortedRanges::<u32, u32>::try_from_span_iter(spans).unwrap();

        let result = ranges
            .map_span_inplace(|source| {
                let extra = vec![Span::new(NonZeroRange::try_from(0..50).unwrap(), 2u64)];
                source.union(extra)
            })
            .expect("Non-empty");

        assert_eq!(1, result.len());
        let out_spans: Vec<_> = result.spans::<u64>().collect();
        assert_eq!(
            out_spans,
            vec![
                Span::new(NonZeroRange::try_from(0..100).unwrap(), 0u64),
                Span::new(NonZeroRange::try_from(0..100).unwrap(), 1u64),
                Span::new(NonZeroRange::try_from(0..50).unwrap(), 2u64),
            ]
        );
    }

    #[test]
    fn subtract_with_bounds_offset() {
        let roi = Rect::new(
            1u32,
            2,
            NonZero::new(100u32).unwrap(),
            NonZero::new(200u32).unwrap(),
        );
        // Two rows, each full width, globally offset by (1, 2)
        let global_spans: Vec<Span<u64>> = vec![
            Span::new(NonZeroRange::try_from(1u64..101).unwrap(), 2u64),
            Span::new(NonZeroRange::try_from(1u64..101).unwrap(), 3u64),
            Span::new(NonZeroRange::try_from(1u64..101).unwrap(), 4u64),
            Span::new(NonZeroRange::try_from(1u64..101).unwrap(), 5u64),
        ];

        let ranges =
            SortedRanges::<u32, u32>::try_from_span_iter(global_spans.clone().with_roi(roi))
                .unwrap();

        assert_eq!(roi, ranges.bounds());

        // Remove the entire second row (global y=3)
        let result = ranges
            .map_span_inplace(|source| {
                // Verify source spans are global (with bounds offset applied)
                let bounds = source.bounds();
                let verified = source.inspect(|s| {
                    assert_eq!(Range::from(s.x), 1..101, "global x.start should be 1");
                    assert!(s.y >= 2, "global y should be >= 2 for bounds.y = 2");
                });
                let remove = [Span::new(NonZeroRange::try_from(1u64..101).unwrap(), 3u64)];
                verified.with_roi(bounds).subtract(remove)
            })
            .expect("Non-empty");

        assert_eq!(roi, result.bounds());
        assert_eq!(2, result.len());
        let out_spans: Vec<Span<u64>> = result.spans::<u64>().collect();
        assert_eq!(
            out_spans,
            vec![
                Span::new(NonZeroRange::try_from(1u64..101).unwrap(), 2u64),
                Span::new(NonZeroRange::try_from(1u64..101).unwrap(), 4u64),
                Span::new(NonZeroRange::try_from(1u64..101).unwrap(), 5u64),
            ]
        );
    }

    #[test]
    fn full_width_multiline_mask_subtract_partial() {
        let width = NonZero::new(100u32).unwrap();
        let height = NonZero::new(200u32).unwrap();
        // Two rows, each full width
        let spans = [
            Span::new(NonZeroRange::try_from(0..100).unwrap(), 0u64),
            Span::new(NonZeroRange::try_from(0..100).unwrap(), 1u64),
        ]
        .with_bounds(width, height);
        let ranges = SortedRanges::<u32, u32>::try_from_span_iter(spans).unwrap();

        // Remove half of the second row
        let result = ranges
            .map_span_inplace(|source| {
                let remove = vec![Span::new(NonZeroRange::try_from(0..50).unwrap(), 1u64)];
                source.subtract(remove)
            })
            .expect("Non-empty");

        assert_eq!(2, result.len());
        let out_spans: Vec<_> = result.spans::<u64>().collect();
        assert_eq!(
            out_spans,
            vec![
                Span::new(NonZeroRange::try_from(0..100).unwrap(), 0u64),
                Span::new(NonZeroRange::try_from(50..100).unwrap(), 1u64),
            ]
        );
    }

    #[test]
    fn map_span_inplace_identity_for_full_width_rows() {
        let width = NonZero::new(100u32).unwrap();
        let height = NonZero::new(200u32).unwrap();
        let spans = [
            Span::new(NonZeroRange::try_from(0..100).unwrap(), 0u64),
            Span::new(NonZeroRange::try_from(0..100).unwrap(), 1u64),
        ]
        .with_bounds(width, height);
        let ranges = SortedRanges::<u32, u32>::try_from_span_iter(spans).unwrap();
        let original = ranges.spans::<u64>().collect::<Vec<_>>();

        // Pass through identity — rows must be preserved
        let result = ranges.map_span_inplace(|source| source).expect("Non-empty");

        let out_spans: Vec<_> = result.spans::<u64>().collect();
        assert_eq!(out_spans, original);
    }

    #[test]
    fn map_span_inplace_preserves_row_boundaries() {
        let width = NonZero::new(100u32).unwrap();
        let height = NonZero::new(3u32).unwrap();
        // Row 0 full, row 1 partial (touching row 0's boundary), row 2 full
        let spans = [
            Span::new(NonZeroRange::try_from(0..100).unwrap(), 0u64),
            Span::new(NonZeroRange::try_from(0..100).unwrap(), 1u64),
            Span::new(NonZeroRange::try_from(0..100).unwrap(), 2u64),
        ]
        .with_bounds(width, height);
        let ranges = SortedRanges::<u32, u32>::try_from_span_iter(spans).unwrap();

        // Subtract middle portion of middle row — rows 0 and 2 must stay separate
        let result = ranges
            .map_span_inplace(|source| {
                let remove = vec![Span::new(NonZeroRange::try_from(25..75).unwrap(), 1u64)];
                source.subtract(remove)
            })
            .expect("Non-empty");

        assert_eq!(2, result.len());
        let out_spans: Vec<_> = result.spans::<u64>().collect();
        assert_eq!(
            out_spans,
            vec![
                Span::new(NonZeroRange::try_from(0..100).unwrap(), 0u64),
                Span::new(NonZeroRange::try_from(0..25).unwrap(), 1u64),
                Span::new(NonZeroRange::try_from(75..100).unwrap(), 1u64),
                Span::new(NonZeroRange::try_from(0..100).unwrap(), 2u64),
            ]
        );
    }
}

#[cfg(feature = "range-set-blaze-0_5")]
mod range_set_blaze_0_5_interop {
    use range_set_blaze_0_5::{SortedDisjoint, SortedStarts};

    use super::*;
    impl<TIncluded, TExcluded> SortedStarts<u64> for SourceIterator<TIncluded, TExcluded>
    where
        TIncluded: UncheckedCast<u64>,
        TExcluded: UncheckedCast<u64>,
    {
    }

    impl<TIncluded, TExcluded> SortedDisjoint<u64> for SourceIterator<TIncluded, TExcluded>
    where
        TIncluded: UncheckedCast<u64>,
        TExcluded: UncheckedCast<u64>,
    {
    }
}
