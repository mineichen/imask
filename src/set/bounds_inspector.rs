use std::{fmt::Debug, iter::FusedIterator, marker::PhantomData, num::NonZero};

use num_traits::{One, Zero};

use crate::{CreateRange, ImageDimension, Rect, UncheckedCast};

#[cfg(feature = "range-set-blaze-0_5")]
use std::ops::RangeInclusive;

pub struct BoundsInspector<T, R> {
    parent: T,
    _range: PhantomData<R>,
    min_column: u32,
    max_column: u32,
    min_row: u32,
    max_row: u32,
}

impl<T, R> Debug for BoundsInspector<T, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundsInspector")
            .field("min_column", &self.min_column)
            .field("max_column", &self.max_column)
            .field("min_row", &self.min_row)
            .field("max_row", &self.max_row)
            .finish()
    }
}

impl<T, R> BoundsInspector<T, R>
where
    T: Iterator,
    R: CreateRange,
{
    pub fn new(parent: T) -> Self {
        BoundsInspector {
            parent,
            _range: PhantomData,
            min_column: u32::MAX,
            max_column: u32::MIN,
            min_row: u32::MAX,
            max_row: u32::MIN,
        }
    }
}

impl<T, R> BoundsInspector<T, R>
where
    T: Iterator + ImageDimension,
    R: CreateRange,
{
    pub fn bounds(&self) -> Option<Rect<u32>> {
        if self.max_row < self.min_row {
            return None;
        }

        let parent_bounds = self.parent.bounds();
        let width = self.max_column - self.min_column + 1;
        let height = self.max_row - self.min_row + 1;

        Some(Rect::new(
            parent_bounds.x + self.min_column,
            parent_bounds.y + self.min_row,
            NonZero::new(width).expect("width should be non-zero"),
            NonZero::new(height).expect("height should be non-zero"),
        ))
    }
}

impl<T, R> Iterator for BoundsInspector<T, R>
where
    T: Iterator<Item = R> + ImageDimension,
    R: CreateRange,
    R::Item: Copy
        + Ord
        + std::ops::Rem<Output = R::Item>
        + std::ops::Div<Output = R::Item>
        + std::ops::Sub<Output = R::Item>
        + Zero
        + One
        + UncheckedCast<u32>,
    u32: UncheckedCast<R::Item>,
{
    type Item = R;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.parent.next()?;

        let start = item.start();
        let end = item.end();
        let width_u32 = self.parent.width().get();
        let width_val: R::Item = width_u32.cast_unchecked();

        let start_row = (start / width_val).cast_unchecked();
        let start_col = (start % width_val).cast_unchecked();

        let last = end - One::one();
        let end_row = (last / width_val).cast_unchecked();
        let end_col = (last % width_val).cast_unchecked();

        self.min_row = self.min_row.min(start_row);
        self.max_row = self.max_row.max(end_row);

        if start_row == end_row {
            self.min_column = self.min_column.min(start_col);
            self.max_column = self.max_column.max(end_col);
        } else {
            self.min_column = 0;
            self.max_column = width_u32 - 1;
        }

        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.parent.size_hint()
    }
}

impl<T, R: CreateRange> FusedIterator for BoundsInspector<T, R>
where
    BoundsInspector<T, R>: Iterator,
    T: FusedIterator,
{
}

impl<T, R> ImageDimension for BoundsInspector<T, R>
where
    T: Iterator + ImageDimension,
    R: CreateRange,
{
    fn width(&self) -> NonZero<u32> {
        self.parent.width()
    }
    fn bounds(&self) -> Rect<u32> {
        self.parent.bounds()
    }
}

#[cfg(feature = "range-set-blaze-0_5")]
mod range_set_blaze_0_5_interop {
    use range_set_blaze_0_5::{Integer, SortedDisjoint, SortedStarts};

    use super::*;

    impl<T, TRangeItem> SortedStarts<TRangeItem> for BoundsInspector<T, RangeInclusive<TRangeItem>>
    where
        TRangeItem: Integer,
        T: SortedStarts<TRangeItem>,
        BoundsInspector<T, RangeInclusive<TRangeItem>>:
            FusedIterator<Item = RangeInclusive<TRangeItem>>,
    {
    }

    impl<T, TRangeItem> SortedDisjoint<TRangeItem> for BoundsInspector<T, RangeInclusive<TRangeItem>>
    where
        TRangeItem: Integer,
        RangeInclusive<TRangeItem>: CreateRange,
        BoundsInspector<T, RangeInclusive<TRangeItem>>: SortedStarts<TRangeItem>,
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
    fn bounds_uses_parent_offset() {
        let roi = Rect::new(100, 100, WIDTH_U32, WIDTH_U32);
        let mut inspector = [13..18usize, 32..33]
            .with_roi(roi)
            .inspect_bounds::<Range<usize>>();
        assert_eq!(2, (&mut inspector).count());
        let expected =
            const { Rect::new(102, 101, NonZero::new(6).unwrap(), NonZero::new(3).unwrap()) };
        assert_eq!(inspector.bounds(), Some(expected));
        assert_eq!(inspector.width(), WIDTH_U32);
    }

    #[test]
    fn single_range_crossing_image_width() {
        let source = std::iter::once(2..27usize).with_bounds(WIDTH_U32, WIDTH_U32);
        let mut inspector = BoundsInspector::<_, Range<usize>>::new(source);
        assert_eq!(1, (&mut inspector).count());
        let b = const { Rect::new(0, 0, NonZero::new(10).unwrap(), NonZero::new(3).unwrap()) };
        assert_eq!(inspector.bounds(), Some(b));
        assert_eq!(inspector.width(), WIDTH_U32);
    }

    #[test]
    fn multiple_ranges_with_different_lengths_and_row_gaps() {
        let mut inspector = [3..6usize, 30..33, 55..65]
            .with_bounds(WIDTH_U32, WIDTH_U32)
            .inspect_bounds();
        // let mut inspector = BoundsInspector::<_, Range<usize>>::new(source);
        let count = (&mut inspector).count();
        assert_eq!(count, 3);
        let b = const { Rect::new(0, 0, NonZero::new(10).unwrap(), NonZero::new(7).unwrap()) };
        assert_eq!(inspector.bounds(), Some(b));
        assert_eq!(inspector.width(), WIDTH_U32);
    }

    #[test]
    fn multiple_ranges_with_offset() {
        let mut inspector = [13..18usize, 32..33]
            .with_bounds(WIDTH_U32, WIDTH_U32)
            .inspect_bounds();
        // let mut inspector = BoundsInspector::<_, Range<usize>>::new(source);
        assert_eq!(2, (&mut inspector).count());
        let b = const { Rect::new(2, 1, NonZero::new(6).unwrap(), NonZero::new(3).unwrap()) };
        assert_eq!(inspector.bounds(), Some(b));
        assert_eq!(inspector.width(), WIDTH_U32);
    }

    #[test]
    fn empty_iterator_returns_none() {
        let source: [Range<usize>; 0] = [];
        let inspector =
            BoundsInspector::<_, Range<usize>>::new(source.with_bounds(WIDTH_U32, WIDTH_U32));
        assert_eq!(inspector.bounds(), None);
        assert_eq!(inspector.width(), WIDTH_U32);
    }

    #[cfg(feature = "range-set-blaze-0_5")]
    use range_set_blaze_0_5::SortedDisjoint;
    #[cfg(feature = "range-set-blaze-0_5")]
    fn _impl_disjoint(
        inspector: BoundsInspector<impl SortedDisjoint<u32> + ImageDimension, RangeInclusive<u32>>,
    ) {
        fn implements(_: impl SortedDisjoint<u32>) {}
        implements(inspector);
    }
}
