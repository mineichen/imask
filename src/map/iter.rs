use std::{iter::FusedIterator, marker::PhantomData};

use crate::{CreateRange, SignedNonZeroable, UncheckedCast};

#[derive(Clone)]
pub struct SortedRangesMapIter<TIncludedIter, TExcludedIter, TMetaIter, TRange: CreateRange> {
    include: TIncludedIter,
    excluded: TExcludedIter,
    meta: TMetaIter,
    offset: TRange::Item,
    _out: PhantomData<TRange>,
}

impl<TIncludedIter, TExcludedIter, TMetaIter, TRange: CreateRange>
    SortedRangesMapIter<TIncludedIter, TExcludedIter, TMetaIter, TRange>
{
    pub(crate) fn new(
        include: TIncludedIter,
        excluded: TExcludedIter,
        meta: TMetaIter,
        offset: TRange::Item,
    ) -> Self {
        Self {
            include,
            excluded,
            meta,
            offset,
            _out: PhantomData,
        }
    }
}

impl<
    TIncluded: Iterator<Item: UncheckedCast<TRange::Item>>,
    TExcluded: Iterator<Item: UncheckedCast<TRange::Item>>,
    TMeta: Iterator,
    TRange: CreateRange,
> Iterator for SortedRangesMapIter<TIncluded, TExcluded, TMeta, TRange>
where
    TRange::Item: SignedNonZeroable + Copy + std::ops::Add<Output = TRange::Item>,
{
    type Item = TRange::ListItem<TMeta::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        let exclude = self.excluded.next()?.cast_unchecked();
        self.offset = self.offset + exclude;

        let Some(include) = self.include.next() else {
            unreachable!("There must be more include");
        };
        let include = include.cast_unchecked();
        let Some(meta) = self.meta.next() else {
            unreachable!("There must be more metadata");
        };

        let out_range = TRange::new_debug_checked(self.offset, include.create_non_zero().unwrap());
        self.offset = self.offset + include;

        Some((out_range, meta).into())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.excluded.size_hint()
    }
}

impl<TIncluded, TExcluded, TMeta, TRange: CreateRange> FusedIterator
    for SortedRangesMapIter<TIncluded, TExcluded, TMeta, TRange>
where
    Self: Iterator,
    TIncluded: FusedIterator,
{
}

#[cfg(feature = "range-set-blaze-0_5")]
mod range_set_blaze_0_5_interop {
    use crate::SignedNonZeroable;

    use super::*;
    use range_set_blaze_0_5::{Integer, SortedDisjointMap, SortedStartsMap, ValueRef};
    use std::ops::RangeInclusive;

    impl<TIncluded, TExcluded, TMeta, TRangeItem> SortedStartsMap<TRangeItem, TMeta::Item>
        for SortedRangesMapIter<TIncluded, TExcluded, TMeta, RangeInclusive<TRangeItem>>
    where
        TIncluded: FusedIterator<Item: UncheckedCast<TRangeItem>>,
        TExcluded: Iterator<Item: UncheckedCast<TRangeItem>>,
        TMeta: Iterator<Item: ValueRef>,
        TRangeItem: Copy
            + Integer
            + num_traits::One
            + SignedNonZeroable
            + std::ops::Sub<Output = TRangeItem>
            + std::ops::Add<Output = TRangeItem>,
    {
    }

    impl<TIncluded, TExcluded, TMeta, TRangeItem> SortedDisjointMap<TRangeItem, TMeta::Item>
        for SortedRangesMapIter<TIncluded, TExcluded, TMeta, std::ops::RangeInclusive<TRangeItem>>
    where
        TIncluded: FusedIterator<Item: UncheckedCast<TRangeItem>>,
        TExcluded: Iterator<Item: UncheckedCast<TRangeItem>>,
        TMeta: Iterator<Item: ValueRef>,
        TRangeItem: Copy
            + Integer
            + num_traits::One
            + SignedNonZeroable
            + std::ops::Sub<Output = TRangeItem>
            + std::ops::Add<Output = TRangeItem>,
    {
    }
}
