use std::{fmt::Debug, iter::FusedIterator, marker::PhantomData, num::NonZero};

use crate::{CreateRange, ImageDimension, SignedNonZeroable, UncheckedCast};

pub struct SplitRowsIter<T, R> {
    parent: T,
    pending: Option<R>,
    _range: PhantomData<R>,
}

impl<T: ImageDimension, R> SplitRowsIter<T, R> {
    pub fn new(parent: T) -> Self {
        assert_eq!(parent.bounds().x, 0);
        assert_eq!(parent.bounds().y, 0);
        Self {
            parent,
            pending: None,
            _range: PhantomData,
        }
    }
}

impl<T: ImageDimension, R> Debug for SplitRowsIter<T, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitRowsIter")
            .field("width", &self.parent.width())
            .finish()
    }
}

impl<T, R> Iterator for SplitRowsIter<T, R>
where
    T: Iterator<Item = R> + ImageDimension,
    R: CreateRange<
        Item: Copy
                  + Ord
                  + std::ops::Sub<Output = R::Item>
                  + std::ops::Add<Output = R::Item>
                  + std::ops::Mul<Output = R::Item>
                  + std::ops::Div<Output = R::Item>,
    >,
    u32: UncheckedCast<R::Item>,
{
    type Item = R;

    fn next(&mut self) -> Option<Self::Item> {
        let width: R::Item = self.width().get().cast_unchecked();

        let range = self.pending.take().or_else(|| self.parent.next())?;

        let start = range.start();
        let end = range.end();

        let row_start = start / width * width;
        let next_row_start = row_start + width;

        if end <= next_row_start {
            Some(range)
        } else {
            let remaining_len = end - next_row_start;

            self.pending = Some(R::new_debug_checked_zeroable(next_row_start, remaining_len));
            Some(R::new_debug_checked_zeroable(start, next_row_start - start))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lo, _) = self.parent.size_hint();
        let pending = self.pending.is_some() as usize;
        (lo.saturating_add(pending), None)
    }
}

impl<T, R> FusedIterator for SplitRowsIter<T, R>
where
    T: FusedIterator<Item = R>,
    SplitRowsIter<T, R>: Iterator,
{
}

impl<T, R> ImageDimension for SplitRowsIter<T, R>
where
    T: Iterator<Item = R> + ImageDimension,
    R: CreateRange,
{
    fn width(&self) -> NonZero<u32> {
        self.parent.width()
    }

    fn bounds(&self) -> crate::Rect<u32> {
        self.parent.bounds()
    }
}
#[cfg(feature = "range-set-blaze-0_5")]
mod range_set_blaze_0_5_interop {
    use range_set_blaze_0_5::{Integer, SortedStarts};
    use std::ops::RangeInclusive;

    use super::*;

    impl<T, TRangeItem> SortedStarts<TRangeItem> for SplitRowsIter<T, RangeInclusive<TRangeItem>>
    where
        TRangeItem: Integer,
        T: SortedStarts<TRangeItem>,
        SplitRowsIter<T, RangeInclusive<TRangeItem>>:
            FusedIterator<Item = RangeInclusive<TRangeItem>>,
    {
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZero, ops::Range};

    use super::*;
    use crate::{ImageDimension, ImaskSet};

    const WIDTH_U32: NonZero<u32> = NonZero::new(10u32).unwrap();

    #[test]
    fn range_within_single_row() {
        let source = [0..5usize].with_bounds(WIDTH_U32, WIDTH_U32);
        let result: Vec<_> = SplitRowsIter::new(source).collect();
        assert_eq!(result, vec![0..5]);
    }

    #[test]
    fn range_crossing_one_row_boundary() {
        let source = [5..15usize].with_bounds(WIDTH_U32, WIDTH_U32);
        let split = SplitRowsIter::new(source);
        assert_eq!(split.width(), WIDTH_U32);
        let result: Vec<_> = split.collect();
        assert_eq!(result, vec![5..10, 10..15]);
    }

    #[test]
    fn range_spanning_three_rows() {
        let source = [0..25usize].with_bounds(WIDTH_U32, WIDTH_U32);
        let result: Vec<_> = SplitRowsIter::new(source).collect();
        assert_eq!(result, vec![0..10, 10..20, 20..25]);
    }

    #[test]
    fn range_exactly_one_row() {
        let source = [10..20usize].with_bounds(WIDTH_U32, WIDTH_U32);
        let result: Vec<_> = SplitRowsIter::new(source).collect();
        assert_eq!(result, vec![10..20]);
    }

    #[test]
    fn multiple_ranges_some_crossing() {
        let source = [0..3usize, 6..12, 15..20].with_bounds(WIDTH_U32, WIDTH_U32);
        let result: Vec<_> = SplitRowsIter::new(source).collect();
        assert_eq!(result, vec![0..3, 6..10, 10..12, 15..20]);
    }

    #[test]
    fn empty_iterator() {
        let source = std::iter::empty().with_bounds(WIDTH_U32, WIDTH_U32);
        let result: Vec<Range<usize>> = SplitRowsIter::new(source).collect();
        assert!(result.is_empty());
    }

    #[test]
    fn single_pixel() {
        let source = [5..6usize].with_bounds(WIDTH_U32, WIDTH_U32);
        let result: Vec<_> = source.split_rows().collect();
        assert_eq!(result, vec![5..6]);
    }

    #[test]
    fn single_pixel_at_row_boundary() {
        let source = [10..11usize].with_bounds(WIDTH_U32, WIDTH_U32);
        let result: Vec<_> = source.split_rows().collect();
        assert_eq!(result, vec![10..11]);
    }

    #[test]
    fn range_starting_at_boundary_crossing_two_rows() {
        let source = [10..25usize].with_bounds(WIDTH_U32, WIDTH_U32);
        let result: Vec<_> = source.split_rows().collect();
        assert_eq!(result, vec![10..20, 20..25]);
    }

    #[test]
    fn range_spanning_many_rows() {
        let source = [3..97usize].with_bounds(WIDTH_U32, WIDTH_U32);
        let result: Vec<_> = source.split_rows().collect();
        assert_eq!(
            result,
            vec![
                3..10,
                10..20,
                20..30,
                30..40,
                40..50,
                50..60,
                60..70,
                70..80,
                80..90,
                90..97
            ]
        );
    }
}
