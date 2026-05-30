use std::{
    fmt::Debug,
    ops::{Add, Sub},
};

use crate::{CreateRange, NonZeroRange, RectIterator, SignedNonZeroable};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive))]
pub struct Rect<T: SignedNonZeroable> {
    pub x: T,
    pub y: T,
    pub width: T::NonZero,
    pub height: T::NonZero,
}

impl<T: SignedNonZeroable + Debug> Debug for Rect<T>
where
    T::NonZero: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rect")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

impl<T: SignedNonZeroable> Rect<T> {
    pub const fn new(x: T, y: T, width: T::NonZero, height: T::NonZero) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub fn len_y(&self) -> T::NonZero
    where
        T: Add<Output = T> + Copy,
    {
        T::create_non_zero(self.y + self.height.into()).expect("Only fails, if addition overflows")
    }
    /// Offset.x + width
    pub fn len_x(&self) -> T::NonZero
    where
        T: Add<Output = T> + Copy,
    {
        T::create_non_zero(self.x + self.width.into()).expect("Only fails, if addition overflows")
    }

    pub fn bounds(&self, other: &Self) -> Self
    where
        T: Copy + Ord + Add<Output = T> + Sub<Output = T>,
    {
        let min_x = self.x.min(other.x);
        let max_x = (self.x + self.width.into()).max(other.x + other.width.into());
        let min_y = self.y.min(other.y);
        let max_y = (self.y + self.height.into()).max(other.y + other.height.into());
        Self {
            x: min_x,
            y: min_y,
            width: T::create_non_zero(max_x - min_x).expect("X must be bigger"),
            height: T::create_non_zero(max_y - min_y).expect("Y must be bigger"),
        }
    }

    pub fn range_x(&self) -> NonZeroRange<T>
    where
        NonZeroRange<T>: CreateRange<Item = T>,
        T: Copy,
    {
        NonZeroRange::new_debug_checked(self.x, self.width)
    }

    pub fn try_cast<TNew: SignedNonZeroable + TryFrom<T>>(self) -> Result<Rect<TNew>, TNew::Error>
where {
        Ok(Rect {
            x: self.x.try_into()?,
            y: self.y.try_into()?,
            width: TNew::create_non_zero(self.width.into().try_into()?)
                .expect("Width doesn't overflow"),
            height: TNew::create_non_zero(self.height.into().try_into()?)
                .expect("Height doesn't overflow"),
        })
    }

    pub fn into_rect_iter<R: CreateRange<Item = T>>(
        self,
        global_width: T::NonZero,
    ) -> RectIterator<R>
    where
        T: num_traits::Zero
            + Copy
            + Debug
            + PartialEq
            + std::ops::Mul<Output = T>
            + std::ops::Add<Output = T>
            + PartialOrd,
        T::NonZero: PartialOrd,
    {
        RectIterator::new(self.x, self.y, self.width, self.height, global_width)
    }

    pub fn into_spans(self) -> crate::span::RectSpanIter<T>
    where
        T: Debug + Ord + Add<Output = T> + Copy,
    {
        crate::span::RectSpanIter::new(self)
    }
}
