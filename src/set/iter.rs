use std::{iter::FusedIterator, num::NonZero};

use crate::{CreateRange, ImageDimension, Rect, SignedNonZeroable, UncheckedCast};

#[derive(Clone)]
pub struct SortedRangesIter<TIncludedIter, TExcludedIter, TOut: CreateRange> {
    include: TIncludedIter,
    excluded: TExcludedIter,
    accumulator: TOut::Item,
    roi: Rect<u32>,
}

impl<TIncludedIter, TExcludedIter, TRange: CreateRange>
    SortedRangesIter<TIncludedIter, TExcludedIter, TRange>
{
    pub(crate) fn new(
        include: TIncludedIter,
        excluded: TExcludedIter,
        accumulator: TRange::Item,
        roi: Rect<u32>,
    ) -> Self {
        Self {
            include,
            excluded,
            accumulator,
            roi,
        }
    }
}

impl<TIncludedIter, TExcludedIter, TOut: CreateRange> ImageDimension
    for SortedRangesIter<TIncludedIter, TExcludedIter, TOut>
{
    fn width(&self) -> NonZero<u32> {
        self.roi.width
    }

    fn bounds(&self) -> crate::Rect<u32> {
        self.roi
    }
}

impl<TIncluded, TExcluded, TOut> Iterator for SortedRangesIter<TIncluded, TExcluded, TOut>
where
    TIncluded: Iterator<Item: UncheckedCast<TOut::Item>>,
    TExcluded: Iterator<Item: UncheckedCast<TOut::Item>>,
    TOut: CreateRange<Item: Copy + SignedNonZeroable + std::ops::Add<Output = TOut::Item>>,
{
    type Item = TOut;

    fn next(&mut self) -> Option<Self::Item> {
        let exclude = self.excluded.next()?.cast_unchecked();
        self.accumulator = self.accumulator + exclude;

        let include = self.include.next()?.cast_unchecked();
        let out_range =
            TOut::new_debug_checked(self.accumulator, include.create_non_zero().unwrap());
        self.accumulator = self.accumulator + include;

        Some(out_range)
    }
}

impl<TIncluded, TExcluded, TOut: CreateRange> FusedIterator
    for SortedRangesIter<TIncluded, TExcluded, TOut>
where
    Self: Iterator,
{
}

#[cfg(feature = "range-set-blaze-0_5")]
mod range_set_blaze_0_5_interop {
    use range_set_blaze_0_5::{Integer, SortedDisjoint, SortedStarts};
    use std::ops::RangeInclusive;

    use super::*;

    impl<TIncluded, TExcluded, T> SortedStarts<T>
        for SortedRangesIter<TIncluded, TExcluded, RangeInclusive<T>>
    where
        TIncluded: FusedIterator<Item: UncheckedCast<T>>,
        TExcluded: Iterator<Item: UncheckedCast<T>>,
        T: Copy
            + SignedNonZeroable
            + std::ops::Add<Output = T>
            + std::ops::Sub<Output = T>
            + num_traits::One
            + Integer,
    {
    }
    impl<TIncluded, TExcluded, T> SortedDisjoint<T>
        for SortedRangesIter<TIncluded, TExcluded, RangeInclusive<T>>
    where
        Self: SortedStarts<T>,
        T: Integer
            + SignedNonZeroable
            + std::ops::Add<Output = T>
            + std::ops::Sub<Output = T>
            + num_traits::One,
    {
    }
}
