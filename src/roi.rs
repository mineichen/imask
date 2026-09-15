use std::{
    fmt::Debug,
    ops::{Add, Sub},
};

use num_traits::{One, Zero};

#[allow(deprecated)]
use crate::Rect;
use crate::{
    CreateRange, NonZeroRange, RangeUnchecked, RectIterator, SignedNonZeroable, UncheckedCast,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive))]
pub struct Roi<T: SignedNonZeroable> {
    pub x: NonZeroRange<T>,
    pub y: NonZeroRange<T>,
}

impl<T: SignedNonZeroable + Debug> Debug for Roi<T>
where
    T::NonZero: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Roi")
            .field("x", &self.x)
            .field("y", &self.y)
            .finish()
    }
}

impl<T: SignedNonZeroable> Roi<T> {
    /// Panics: When `TryInto<NonZeroRange<T>>::try_into` fails. If a `NonZeroRange<T>` is provided, this method doesn't do any validation
    // CreateRange-Bound allows better type inference because it's a Assoc-Type, while `TryInto` is a generic trait param
    pub fn new<TNew: CreateRange<Item = T> + TryInto<NonZeroRange<T>, Error: Debug>>(
        x: TNew,
        y: TNew,
    ) -> Self {
        let x: NonZeroRange<T> = x.try_into().expect("X is invalid");
        let y: NonZeroRange<T> = y.try_into().expect("Y is invalid");
        Self { x, y }
    }

    pub fn from_dimensions(width: T::NonZero, height: T::NonZero) -> Self
    where
        T: SignedNonZeroable + Zero + Copy,
    {
        let x = NonZeroRange::from_span(T::zero(), width);
        let y = NonZeroRange::from_span(T::zero(), height);
        Self { x, y }
    }

    /// Creates a roi without release-mode validation. Only checks in debug builds.
    ///
    /// # Safety
    /// `x.start < x.end` and `y.start < y.end` are required. Use only with
    /// already-validated data (e.g. a `NonZero` length, min/max of valid rois,
    /// or ranges derived from an existing `Roi`/`Span`). A `NonZeroRange` can
    /// be passed through (re-checked in debug only).
    pub fn new_unchecked(x: impl Into<RangeUnchecked<T>>, y: impl Into<RangeUnchecked<T>>) -> Self
    where
        T: Ord + Debug,
    {
        Self {
            x: NonZeroRange::new_unchecked(x),
            y: NonZeroRange::new_unchecked(y),
        }
    }

    pub fn width(&self) -> T::NonZero
    where
        T: Copy + Sub<Output = T>,
    {
        self.x.len_non_zero()
    }

    pub fn height(&self) -> T::NonZero
    where
        T: Copy + Sub<Output = T>,
    {
        self.y.len_non_zero()
    }

    #[deprecated = "Only for migration away from Rect; use x.end / range_x instead"]
    pub fn len_x(&self) -> T::NonZero
    where
        T: Copy,
    {
        T::create_non_zero(self.x.end).expect("Roi end is always non-zero")
    }

    #[deprecated = "Only for migration away from Rect; use y.end / range_y instead"]
    pub fn len_y(&self) -> T::NonZero
    where
        T: Copy,
    {
        T::create_non_zero(self.y.end).expect("Roi end is always non-zero")
    }

    #[deprecated = "Only for migration away from Rect"]
    pub fn cast_unchecked<TNew>(self) -> Roi<TNew>
    where
        T: UncheckedCast<TNew> + Copy,
        TNew: SignedNonZeroable + Ord + Debug + Copy + Add<Output = TNew> + PartialOrd,
    {
        Roi {
            x: NonZeroRange::new_unchecked(
                self.x.start.cast_unchecked()..self.x.end.cast_unchecked(),
            ),
            y: NonZeroRange::new_unchecked(
                self.y.start.cast_unchecked()..self.y.end.cast_unchecked(),
            ),
        }
    }

    pub fn union(&self, other: &Self) -> Self
    where
        T: Copy + Ord + Debug,
    {
        let x = self.x.union(&other.x);
        let y = self.y.union(&other.y);
        Self { x, y }
    }

    /// Largest roi contained in `self` and `other`, or `None` if they don't overlap
    /// (touching edges don't overlap).
    pub fn intersection(&self, other: &Self) -> Option<Self>
    where
        T: Copy + Ord + Debug,
    {
        Some(Self {
            x: self.x.intersection(&other.x)?,
            y: self.y.intersection(&other.y)?,
        })
    }

    pub fn contains(&self, x: &T, y: &T) -> bool
    where
        T: Copy + Ord,
    {
        self.x.contains(x) && self.y.contains(y)
    }

    pub fn try_cast<TNew: Debug + Ord + SignedNonZeroable + TryFrom<T>>(
        self,
    ) -> Result<Roi<TNew>, TNew::Error>
    where
        T: Ord + Debug,
    {
        Ok(Roi {
            x: self.x.try_cast::<TNew>()?,
            y: self.y.try_cast::<TNew>()?,
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
            + PartialOrd
            + Sub<Output = T>,
        T::NonZero: PartialOrd,
    {
        RectIterator::new(
            self.x.start,
            self.y.start,
            self.width(),
            self.height(),
            global_width,
        )
    }

    pub fn into_spans(self) -> crate::span::RectSpanIter<T>
    where
        T: Debug + Ord + Add<Output = T> + Copy + Sub<Output = T>,
    {
        #[allow(deprecated)]
        let rect = Rect::from(self);
        crate::span::RectSpanIter::new(rect)
    }
}

impl Roi<u32> {
    /// Expands the roi by `radius` on all sides.
    ///
    /// Left/top are clamped at 0 (no underflow), right/bottom saturate at `u32::MAX`,
    /// so the result always contains `self`.
    pub fn expand_saturating(self, radius: u32) -> Self {
        let x_start = self.x.start.saturating_sub(radius);
        let y_start = self.y.start.saturating_sub(radius);
        let x_end = self.x.end.saturating_add(radius);
        let y_end = self.y.end.saturating_add(radius);
        Self {
            x: NonZeroRange::new_unchecked(x_start..x_end),
            y: NonZeroRange::new_unchecked(y_start..y_end),
        }
    }
}

#[allow(deprecated)]
impl<T> From<Rect<T>> for Roi<T>
where
    T: SignedNonZeroable + Copy + Debug,
{
    fn from(rect: Rect<T>) -> Self {
        Self {
            x: NonZeroRange::from_span(rect.x, rect.width),
            y: NonZeroRange::from_span(rect.y, rect.height),
        }
    }
}

#[allow(deprecated)]
impl<T> From<Roi<T>> for Rect<T>
where
    T: SignedNonZeroable + Copy + Sub<Output = T> + Debug,
{
    fn from(roi: Roi<T>) -> Self {
        Self {
            x: roi.x.start,
            y: roi.y.start,
            width: roi.width(),
            height: roi.height(),
        }
    }
}

impl<T> From<crate::Span<T>> for Roi<T>
where
    T: SignedNonZeroable + Copy + Sub<Output = T> + Debug + One,
{
    fn from(value: crate::Span<T>) -> Self {
        Self {
            x: value.x,
            y: NonZeroRange::from_span(
                value.y,
                T::one().create_non_zero().expect("One is not zero"),
            ),
        }
    }
}

// Keep parity with Rect::new tests
#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    const NON_ZERO_10: NonZeroU32 = NonZeroU32::new(10).unwrap();

    #[test]
    #[should_panic(expected = "X is invalid")]
    fn new_panics_on_empty_x() {
        let _ = Roi::<u32>::new(5..5, 0..10);
    }

    #[test]
    #[should_panic(expected = "Y is invalid")]
    fn new_panics_on_empty_y() {
        let _ = Roi::<u32>::new(0..10, 5..5);
    }

    #[test]
    fn new_accepts_non_empty() {
        assert_eq!(
            Roi::<u32>::new(0..10, 0..10),
            Roi {
                x: NonZeroRange::new(0..10),
                y: NonZeroRange::new(0..10),
            }
        );
    }

    #[test]
    #[allow(deprecated)]
    fn from_rect_roundtrip() {
        let rect = Rect::new(0u32, 0, NON_ZERO_10, NON_ZERO_10);
        let roi = Roi::from(rect);
        assert_eq!(Rect::from(roi), rect);
    }

    #[test]
    fn intersection_overlapping() {
        let a = Roi::new(0u32..10, 0..10);
        let b = Roi::new(5..15, 5..15);
        let expected = Roi::new(5..10, 5..10);
        assert_eq!(Some(expected), a.intersection(&b));
        assert_eq!(Some(expected), b.intersection(&a));
    }

    #[test]
    fn intersection_disjoint() {
        let a = Roi::new(0u32..10, 0..10);
        let b = Roi::new(20..30, 20..30);
        assert_eq!(None, a.intersection(&b));
    }

    #[test]
    fn intersection_touching_edge_is_none() {
        let a = Roi::new(0u32..10, 0..10);
        let b = Roi::new(10..20, 0..10);
        assert_eq!(None, a.intersection(&b));
    }

    #[test]
    fn union_combines() {
        let a = Roi::new(0u32..10, 0..10);
        let b = Roi::new(5..15, 5..15);
        let expected = Roi::new(0..15, 0..15);
        assert_eq!(expected, a.union(&b));
    }

    #[test]
    fn max_rect_does_not_overflow_on_union() {
        // Rect with x=MAX would overflow on len_x; Roi stores ends directly.
        let a = Roi::new(u32::MAX - 10..u32::MAX, 0..10);
        let b = Roi::new(0..10, 0..10);
        let u = a.union(&b);
        assert_eq!(u.x.start, 0);
        assert_eq!(u.x.end, u32::MAX);
    }
}
