use std::{
    fmt::Debug,
    num::NonZeroU32,
    ops::{Add, Sub},
};

use crate::{CreateRange, NonZeroRange, RectIterator, SignedNonZeroable, UncheckedCast};

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

    pub fn cast_unchecked<TNew: SignedNonZeroable>(self) -> Rect<TNew>
    where
        T: UncheckedCast<TNew>,
    {
        let x = self.x.cast_unchecked();
        let y = self.y.cast_unchecked();

        let width = self
            .width
            .into()
            .cast_unchecked()
            .create_non_zero()
            .expect("Still NonZero after cast");
        let height = self
            .height
            .into()
            .cast_unchecked()
            .create_non_zero()
            .expect("Still NonZero after cast");
        Rect {
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

    /// Largest rect contained in `self` and `other`, or `None` if they don't overlap
    /// (touching edges don't overlap).
    pub fn intersection(&self, other: &Self) -> Option<Self>
    where
        T: Copy + Ord + Add<Output = T> + Sub<Output = T>,
    {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let x_end = (self.x + self.width.into()).min(other.x + other.width.into());
        let y_end = (self.y + self.height.into()).min(other.y + other.height.into());
        if x_end <= x || y_end <= y {
            return None;
        }
        Some(Self {
            x,
            y,
            width: T::create_non_zero(x_end - x).expect("Checked above"),
            height: T::create_non_zero(y_end - y).expect("Checked above"),
        })
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

impl Rect<u32> {
    /// Expands the rect by `radius` on all sides.
    ///
    /// Left/top are clamped at 0 (no underflow), right/bottom saturate at `u32::MAX`,
    /// so the result always contains `self`.
    pub fn expand(self, radius: u32) -> Self {
        let x = self.x.saturating_sub(radius);
        let y = self.y.saturating_sub(radius);
        let x_end = self.x.saturating_add(self.width.get()).saturating_add(radius);
        let y_end = self.y.saturating_add(self.height.get()).saturating_add(radius);
        Self::new(
            x,
            y,
            NonZeroU32::new(x_end - x).expect("x_end > x because width is non-zero"),
            NonZeroU32::new(y_end - y).expect("y_end > y because height is non-zero"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NON_ZERO_10: NonZeroU32 = NonZeroU32::new(10).unwrap();

    #[test]
    fn intersection_overlapping() {
        let a = Rect::new(0u32, 0, NON_ZERO_10, NON_ZERO_10);
        let b = Rect::new(5u32, 5, NON_ZERO_10, NON_ZERO_10);
        let expected =
            Rect::new(5u32, 5, NonZeroU32::new(5).unwrap(), NonZeroU32::new(5).unwrap());
        assert_eq!(Some(expected), a.intersection(&b));
        assert_eq!(Some(expected), b.intersection(&a));
    }

    #[test]
    fn intersection_contained() {
        let a = Rect::new(0u32, 0, NON_ZERO_10, NON_ZERO_10);
        let b = Rect::new(
            2u32,
            3,
            NonZeroU32::new(4).unwrap(),
            NonZeroU32::new(2).unwrap(),
        );
        assert_eq!(Some(b), a.intersection(&b));
        assert_eq!(Some(b), b.intersection(&a));
    }

    #[test]
    fn intersection_disjoint() {
        let a = Rect::new(0u32, 0, NON_ZERO_10, NON_ZERO_10);
        let b = Rect::new(20u32, 20, NON_ZERO_10, NON_ZERO_10);
        assert_eq!(None, a.intersection(&b));
        assert_eq!(None, b.intersection(&a));
    }

    #[test]
    fn intersection_touching_edge_is_none() {
        let a = Rect::new(0u32, 0, NON_ZERO_10, NON_ZERO_10);
        let b = Rect::new(10u32, 0, NON_ZERO_10, NON_ZERO_10);
        assert_eq!(None, a.intersection(&b));
    }
}
