use std::{fmt::Debug, ops::Add};

use crate::{MetaRange, NonZeroRange, RangeUnchecked, SignedNonZeroable};

pub trait CreateRange: Sized {
    type Item;
    type ListItem<TMeta>: From<(Self, TMeta)>;

    fn new_debug_checked(
        start: Self::Item,
        len: <Self::Item as SignedNonZeroable>::NonZero,
    ) -> Self
    where
        Self::Item: SignedNonZeroable;
    fn new_debug_checked_zeroable(start: Self::Item, end: Self::Item) -> Self;

    fn start(&self) -> Self::Item;
    fn end(&self) -> Self::Item;
}

impl<
    T: SignedNonZeroable
        + Copy
        + Debug
        + PartialOrd
        + num_traits::One
        + std::ops::Sub<Output = T>
        + std::ops::Add<Output = T>,
> CreateRange for std::ops::RangeInclusive<T>
{
    type Item = T;
    type ListItem<TMeta> = (Self, TMeta);

    #[inline]
    fn new_debug_checked_zeroable(start: Self::Item, end: Self::Item) -> Self {
        debug_assert!(start < end);
        start..=end - T::one()
    }
    #[inline]
    fn new_debug_checked(
        start: Self::Item,
        len: <Self::Item as SignedNonZeroable>::NonZero,
    ) -> Self {
        let end = start.add_nonzero(len) - T::one();
        start..=end
    }

    fn start(&self) -> Self::Item {
        *std::ops::RangeInclusive::start(self)
    }
    fn end(&self) -> Self::Item {
        *std::ops::RangeInclusive::end(self) + T::one()
    }
}

impl<T: SignedNonZeroable + PartialOrd + Copy + Add<Output = T>> CreateRange
    for std::ops::Range<T>
{
    type Item = T;
    type ListItem<TMeta> = (Self, TMeta);

    #[inline]
    fn new_debug_checked_zeroable(start: Self::Item, end: Self::Item) -> Self {
        debug_assert!(start < end);
        start..end
    }
    #[inline]
    fn new_debug_checked(
        start: Self::Item,
        len: <Self::Item as SignedNonZeroable>::NonZero,
    ) -> Self {
        let end = start.add_nonzero(len);
        start..end
    }

    fn start(&self) -> Self::Item {
        self.start
    }
    fn end(&self) -> Self::Item {
        self.end
    }
}

impl<T: Copy + Debug + Ord> CreateRange for NonZeroRange<T> {
    type Item = T;
    type ListItem<TMeta> = MetaRange<Self, TMeta>;

    #[inline]
    fn new_debug_checked_zeroable(start: Self::Item, end: Self::Item) -> Self {
        debug_assert!(start < end);
        NonZeroRange::new_unchecked(RangeUnchecked { start, end })
    }

    #[inline]
    fn new_debug_checked(start: Self::Item, len: <Self::Item as SignedNonZeroable>::NonZero) -> Self
    where
        Self::Item: SignedNonZeroable,
    {
        NonZeroRange::from_span(start, len)
    }
    fn start(&self) -> Self::Item {
        self.start
    }
    fn end(&self) -> Self::Item {
        self.end
    }
}
