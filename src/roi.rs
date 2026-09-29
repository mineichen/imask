use std::{
    fmt::Debug,
    ops::{Add, Mul, Sub},
};

use num_traits::{One, Zero};

#[allow(deprecated)]
use crate::Rect;
use crate::{
    CreateRange, NonZeroRange, RangeUnchecked, RectIterator, SignedNonZeroable, SortedRanges,
    UncheckedCast, number::Sqrtable,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive))]
pub struct Roi<T: SignedNonZeroable> {
    pub x: NonZeroRange<T>,
    pub y: NonZeroRange<T>,
}

macro_rules! impl_from {
    ($src:ty, $dst:ty) => {
        impl From<Roi<$src>> for Roi<$dst> {
            #[inline]
            fn from(value: Roi<$src>) -> Self {
                Self {
                    x: value.x.into(),
                    y: value.y.into(),
                }
            }
        }
    };
}
impl_from!(u8, u16);
impl_from!(u8, u32);
impl_from!(u8, u64);
impl_from!(u16, u32);
impl_from!(u16, u64);
impl_from!(u32, u64);

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
    #[inline]
    pub fn new<TNew: CreateRange<Item = T> + TryInto<NonZeroRange<T>, Error: Debug>>(
        x: TNew,
        y: TNew,
    ) -> Self {
        let x: NonZeroRange<T> = x.try_into().expect("X is invalid");
        let y: NonZeroRange<T> = y.try_into().expect("Y is invalid");
        Self { x, y }
    }

    #[inline]
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
    #[inline]
    pub fn new_unchecked(x: impl Into<RangeUnchecked<T>>, y: impl Into<RangeUnchecked<T>>) -> Self
    where
        T: Ord + Debug,
    {
        Self {
            x: NonZeroRange::new_unchecked(x),
            y: NonZeroRange::new_unchecked(y),
        }
    }

    #[inline]
    pub fn width(&self) -> T::NonZero
    where
        T: Copy + Sub<Output = T>,
    {
        self.x.len_non_zero()
    }

    #[inline]
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

    #[inline]
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
    #[inline]
    pub fn intersection(&self, other: &Self) -> Option<Self>
    where
        T: Copy + Ord + Debug,
    {
        Some(Self {
            x: self.x.intersection(&other.x)?,
            y: self.y.intersection(&other.y)?,
        })
    }

    #[inline]
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

/// Infallible, because the pixel count of a `Roi<T::Sqrt>` always fits into `T`
/// and a `Roi` is never empty.
impl<T> From<Roi<T::Sqrt>> for SortedRanges<T>
where
    T: Sqrtable + Mul<Output = T>,
    T::Sqrt: SignedNonZeroable + Copy + Into<u32> + Sub<Output = T::Sqrt>,
{
    fn from(roi: Roi<T::Sqrt>) -> Self {
        let bounds = Roi {
            x: NonZeroRange::new_unchecked(roi.x.start.into()..roi.x.end.into()),
            y: NonZeroRange::new_unchecked(roi.y.start.into()..roi.y.end.into()),
        };
        #[allow(clippy::eq_op, reason = "Avoid additional bound on num_traits::Zero")]
        let zero = roi.x.start - roi.x.start;
        // Bounds equal the roi, so all rows are contiguous and form a single range
        Self::new_internal(
            vec![T::from(roi.x.len()) * T::from(roi.y.len())],
            vec![T::from(zero)],
            bounds,
        )
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

    use crate::{ImageDimension, Span};

    use super::*;

    const NON_ZERO_10: NonZeroU32 = NonZeroU32::new(10).unwrap();

    #[test]
    #[should_panic(expected = "X is invalid")]
    fn new_panics_on_empty_x() {
        let _ = Roi::<u32>::new(5..5, 0..10);
    }

    #[test]
    fn into_sorted_ranges() {
        let ranges: SortedRanges<u64> = Roi::new(10u32..12, 10..12).into();
        assert_eq!(Roi::new(10u32..12, 10..12), ranges.roi());
        assert_eq!(
            vec![Span::new(10u32..12, 10), Span::new(10..12, 11)],
            ranges.spans().collect::<Vec<_>>()
        );
    }

    #[test]
    fn into_sorted_ranges_matches_span_iter() {
        let roi = Roi::new(3u16..7, 5..9);
        let ranges: SortedRanges<u32> = roi.into();
        let expected =
            SortedRanges::<u32>::try_from_span_iter(roi.into_spans()).expect("Roi is not empty");
        assert_eq!(expected, ranges);
    }

    #[test]
    fn into_sorted_ranges_max_does_not_overflow() {
        let ranges: SortedRanges<u32> = Roi::new(0u16..u16::MAX, 0..u16::MAX).into();
        let max = u32::from(u16::MAX) * u32::from(u16::MAX);
        assert_eq!(
            vec![0..max],
            ranges
                .iter_roi::<std::ops::Range<u32>>()
                .collect::<Vec<_>>()
        );

        let ranges: SortedRanges<u16> = Roi::new(0..u8::MAX, 0..u8::MAX).into();
        let max = u16::from(u8::MAX) * u16::from(u8::MAX);
        assert_eq!(
            vec![0..max],
            ranges
                .iter_roi::<std::ops::Range<u16>>()
                .collect::<Vec<_>>()
        );
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
