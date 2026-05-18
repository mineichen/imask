use std::{
    fmt::Debug,
    iter::{FusedIterator, Once},
    marker::PhantomData,
    num::{NonZero, NonZeroU32},
    ops::{Add, Mul},
};

use num_traits::Zero;

use crate::{CreateRange, ImageDimension, Rect, SignedNonZeroable};

pub struct RectIterator<R: CreateRange<Item: SignedNonZeroable>> {
    pub kind: RectIteratorKind<R>,
    width: <R::Item as SignedNonZeroable>::NonZero,
    height: <R::Item as SignedNonZeroable>::NonZero,
}

// #[derive(Clone)]
pub enum RectIteratorKind<R: CreateRange<Item: SignedNonZeroable>> {
    FullWidth(Once<R>),
    PartialWidth(PartialWidthRectIterator<R>),
}

impl<R: CreateRange<Item: SignedNonZeroable + TryInto<u32, Error: Debug>>> ImageDimension
    for RectIterator<R>
{
    fn width(&self) -> std::num::NonZero<u32> {
        NonZero::new(self.width.into().try_into().expect("width < u32::MAX"))
            .expect("self.width is NonZero")
    }

    fn bounds(&self) -> crate::Rect<u32> {
        let height = NonZeroU32::new(self.height.into().try_into().expect("height < u32::MAX"))
            .expect("self.width is NonZero");
        Rect {
            x: 0,
            y: 0,
            width: self.width(),
            height,
        }
    }
}

impl<R> RectIterator<R>
where
    R: CreateRange<
        Item: SignedNonZeroable<NonZero: PartialOrd>
                  + Mul<Output = R::Item>
                  + Add<Output = R::Item>
                  + Copy
                  + Zero
                  + Debug
                  + PartialEq,
    >,
{
    pub fn new(
        x: R::Item,
        y: R::Item,
        width: <R::Item as SignedNonZeroable>::NonZero,
        height: <R::Item as SignedNonZeroable>::NonZero,
        global_width: <R::Item as SignedNonZeroable>::NonZero,
    ) -> Self {
        debug_assert!(width <= global_width);
        let kind = if width < global_width {
            RectIteratorKind::PartialWidth(PartialWidthRectIterator::new(
                x,
                y,
                width,
                height,
                global_width,
            ))
        } else {
            debug_assert_eq!(
                x,
                <R::Item as Zero>::zero(),
                "x must be zero for full width ranges"
            );
            let start = y * global_width.into();
            let len = R::Item::create_non_zero(height.into() * global_width.into())
                .expect("Only happens on overflow");
            RectIteratorKind::FullWidth(std::iter::once(R::new_debug_checked(start, len)))
        };
        Self {
            kind,
            width: global_width,
            height: R::Item::create_non_zero(height.into() + y).unwrap(),
        }
    }
}

impl<R> Iterator for RectIterator<R>
where
    R: CreateRange<Item: Add<Output = R::Item> + Copy + SignedNonZeroable + PartialOrd>,
{
    type Item = R;

    fn next(&mut self) -> Option<Self::Item> {
        self.kind.next()
    }
}
impl<R> Iterator for RectIteratorKind<R>
where
    R: CreateRange<Item: Add<Output = R::Item> + Copy + SignedNonZeroable + PartialOrd>,
{
    type Item = R;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::FullWidth(x) => x.next(),
            Self::PartialWidth(x) => x.next(),
        }
    }
}
impl<R: CreateRange<Item: Add<Output = R::Item> + Copy + SignedNonZeroable + PartialOrd>>
    FusedIterator for RectIterator<R>
{
}

#[derive(Clone)]
pub struct PartialWidthRectIterator<R: CreateRange<Item: SignedNonZeroable>> {
    start_index: R::Item,
    end_index: R::Item,
    width: <R::Item as SignedNonZeroable>::NonZero,
    image_width: <R::Item as SignedNonZeroable>::NonZero,
    _range: PhantomData<R>,
}

impl<R> PartialWidthRectIterator<R>
where
    R: CreateRange<
        Item: SignedNonZeroable<NonZero: PartialOrd>
                  + Mul<Output = R::Item>
                  + Add<Output = R::Item>
                  + Copy,
    >,
{
    pub fn new(
        x: R::Item,
        y: R::Item,
        width: <R::Item as SignedNonZeroable>::NonZero,
        height: <R::Item as SignedNonZeroable>::NonZero,
        global_width: <R::Item as SignedNonZeroable>::NonZero,
    ) -> Self {
        debug_assert!(width < global_width);
        let start_index = x + y * global_width.into();
        let end_index = start_index + height.into() * global_width.into();
        Self {
            start_index,
            end_index,
            width,
            image_width: global_width,
            _range: PhantomData,
        }
    }
}

impl<R> Iterator for PartialWidthRectIterator<R>
where
    R: CreateRange<Item: Add<Output = R::Item> + Copy + SignedNonZeroable + PartialOrd>,
{
    type Item = R;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start_index < self.end_index {
            let r = R::new_debug_checked(self.start_index, self.width);
            self.start_index = self.start_index + self.image_width.into();
            Some(r)
        } else {
            None
        }
    }
}
impl<R: CreateRange<Item: Add<Output = R::Item> + Copy + SignedNonZeroable + PartialOrd>>
    FusedIterator for PartialWidthRectIterator<R>
{
}

#[cfg(feature = "range-set-blaze-0_5")]
mod range_set_blaze_interop {
    use std::ops::{RangeInclusive, Sub};

    use num_traits::One;
    use range_set_blaze_0_5::{Integer, SortedDisjoint, SortedStarts};

    use super::*;

    impl<T> SortedStarts<T> for RectIterator<RangeInclusive<T>> where
        T: Add<Output = T> + Sub<Output = T> + Integer + One + SignedNonZeroable + PartialOrd
    {
    }
    impl<T: Add<Output = T> + Sub<Output = T> + Integer + One + SignedNonZeroable + PartialOrd>
        SortedDisjoint<T> for PartialWidthRectIterator<RangeInclusive<T>>
    {
    }
    impl<T> SortedStarts<T> for PartialWidthRectIterator<RangeInclusive<T>> where
        T: Add<Output = T> + Sub<Output = T> + Integer + One + SignedNonZeroable + PartialOrd
    {
    }
    impl<T> SortedDisjoint<T> for RectIterator<RangeInclusive<T>> where
        T: Add<Output = T> + Sub<Output = T> + Integer + One + SignedNonZeroable + PartialOrd
    {
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use super::*;
    const NON_ZERO_5: NonZeroU16 = NonZeroU16::new(5).unwrap();
    const NON_ZERO_10: NonZeroU16 = NonZeroU16::new(10).unwrap();

    #[test]
    fn simple_range() {
        let x = RectIterator::new(2u16, 4, NON_ZERO_5, NON_ZERO_5, NON_ZERO_10);
        assert_eq!(
            vec!(42..47, 52..57, 62..67, 72..77, 82..87),
            x.collect::<Vec<_>>()
        )
    }

    #[test]
    fn full_width_range() {
        let x = RectIterator::new(0u16, 2, NON_ZERO_5, NON_ZERO_5, NON_ZERO_5);
        assert_eq!(vec!(10..35), x.collect::<Vec<_>>());
    }
}
