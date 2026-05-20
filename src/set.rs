use std::{
    cmp::Ord,
    fmt::{Debug, Display},
    io,
    num::{NonZero, NonZeroU32, NonZeroU64},
    ops::{Add, Div, Mul, Rem, Sub},
};

use crate::{
    CreateRange, ImageDimension, NonZeroRange, Rect, SignedNonZeroable, SortedRangesSpanIter, Span,
    UncheckedCast, WithBounds, WithRoi,
};
#[cfg(feature = "range-set-blaze-0_5")]
use num_traits::{CheckedSub, One, SaturatingSub, Zero};

fn invalid_data<T: Display>(e: T) -> std::io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

mod bounds_inspector;
// mod chunk_by_row;
mod affine_transform;
mod clip_2d;
#[cfg(feature = "range-set-blaze-0_5")]
mod dilate;
#[cfg(feature = "async-io")]
mod future;
mod iter;
mod iter_global;
mod map_inplace;
mod offsets_iter;
mod rect;
mod sanitize_sorted_disjoint;
// mod split_rows;

pub use affine_transform::*;
pub use bounds_inspector::*;
// pub use chunk_by_row::*;
pub use clip_2d::*;
#[cfg(feature = "range-set-blaze-0_5")]
pub use dilate::*;
pub use iter::*;
pub use iter_global::*;
pub use map_inplace::*;
pub use offsets_iter::*;
pub use rect::*;
pub use sanitize_sorted_disjoint::*;
// pub use split_rows::*;

pub trait ImaskSet: IntoIterator + Sized {
    // /// # Panics
    // /// If the previous RowIterator is kept when getting the next RowIterator
    // fn chunk_by_row_lending<R: CreateRange<Item: SignedNonZeroable>>(
    //     self,
    // ) -> ChunkByRowRanges<Self::IntoIter, R> {
    //     ChunkByRowRanges::new(self.into_iter())
    // }

    fn inspect_bounds<R: CreateRange>(self) -> BoundsInspector<Self::IntoIter, R> {
        BoundsInspector::new(self.into_iter())
    }
    fn union<TOther: IntoIterator<Item = Span<T>>, T>(
        self,
        other: TOther,
    ) -> crate::span::Union<Self::IntoIter, TOther::IntoIter> {
        crate::span::Union::new(self.into_iter(), other.into_iter())
    }

    fn subtract<TOther: IntoIterator<Item = Span<T>>, T>(
        self,
        other: TOther,
    ) -> crate::span::Subtract<Self::IntoIter, TOther::IntoIter> {
        crate::span::Subtract::new(self.into_iter(), other.into_iter())
    }

    fn union_all<T>(self) -> Option<crate::span::UnionAll<Self::Item>>
    where
        Self: ImageDimension,
        T: Ord + Copy + std::fmt::Debug,
        Self::Item: Iterator<Item = Span<T>>,
    {
        crate::span::UnionAll::new(self)
    }

    fn try_clip_2d(
        self,
        roi: Rect<u32>,
    ) -> Result<Clip2dIter<Self::IntoIter, Self::Item>, RoiWidthExceedsOriginal>
    where
        Self::IntoIter: ImageDimension,
    {
        Clip2dIter::try_new(self.into_iter(), roi)
    }

    // fn split_rows(self) -> SplitRowsIter<Self::IntoIter, Self::Item>
    // where
    //     Self::IntoIter: ImageDimension,
    // {
    //     SplitRowsIter::new(self.into_iter())
    // }

    fn into_ranges<TOut: CreateRange<Item: SignedNonZeroable>>(
        self,
    ) -> crate::span::SpanIntoRangesIter<Self::IntoIter, TOut>
    where
        Self::IntoIter: ImageDimension,
        TOut::Item: TryFrom<u32, Error: Debug>,
    {
        crate::span::SpanIntoRangesIter::new(self.into_iter())
    }

    fn sanitize_sorted_disjoint(self) -> SanitizeSortedDisjoint<Self::IntoIter>
    where
        Self::Item: CreateRange<Item: Debug>,
    {
        SanitizeSortedDisjoint::new(self)
    }

    fn with_roi(self, roi: Rect<u32>) -> WithRoi<Self::IntoIter> {
        WithRoi::new(self.into_iter(), roi)
    }
    fn with_bounds(self, width: NonZeroU32, height: NonZeroU32) -> WithBounds<Self::IntoIter> {
        WithBounds::new(self.into_iter(), width, height)
    }
    fn dilate<T>(
        self,
        offset: <T as SignedNonZeroable>::NonZero,
    ) -> Option<crate::span::DilateSpanIter<Self::IntoIter, T>>
    where
        T: Ord
            + Copy
            + Debug
            + Add<Output = T>
            + SaturatingSub<Output = T>
            + crate::CheckedAddSigned
            + One
            + Zero
            + SignedNonZeroable
            + UncheckedCast<u32>,
        Self::IntoIter: Iterator<Item = Span<T>> + Clone + ImageDimension,
    {
        crate::span::DilateSpanIter::new(self.into_iter(), offset)
    }

    #[cfg(feature = "range-set-blaze-0_5")]
    fn dilate_range<'a>(
        self,
        offset: <<Self::Item as CreateRange>::Item as SignedNonZeroable>::NonZero,
    ) -> DilateIter<'a, Self::Item>
    where
        Self::Item: 'static
            + CreateRange<
                Item: SignedNonZeroable
                          + Debug
                          + Add<Output = <Self::Item as CreateRange>::Item>
                          + SaturatingSub<Output = <Self::Item as CreateRange>::Item>
                          + CheckedSub<Output = <Self::Item as CreateRange>::Item>
                          + Copy
                          + range_set_blaze_0_5::Integer
                          + Zero
                          + One,
            >,
        Self::IntoIter: 'a + std::iter::FusedIterator<Item = Self::Item> + Clone + ImageDimension,
        SanitizeSortedDisjoint<DilateXIter<Self::IntoIter>>: Iterator<Item = Self::Item>,
        u32: UncheckedCast<<Self::Item as CreateRange>::Item>,
    {
        DilateIter::new(self.into_iter(), offset)
    }
}

impl<I: IntoIterator> ImaskSet for I {}

/// Represents areas on images. It's designed to efficiently support various image sizes.
/// Both, TIncluded and TExcluded are expected to always be > 0. Use non-zero signed types
/// Included represents the number of pixels to include, excluded encodes the gap between two included ranges
///
/// Included.len() = excluded.len() + 1
///
/// Meta is expected to be indexable for each included range
#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive))]
pub struct SortedRanges<TIncluded, TExcluded> {
    included: Vec<TIncluded>,
    excluded: Vec<TExcluded>,
    bounds: Rect<u32>,
}
impl<TIncluded, TExcluded> Debug for SortedRanges<TIncluded, TExcluded> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NonEmptyOrderedRanges")
            .field("range_count", &self.included.len())
            .field("bounds", &self.bounds)
            .finish()
    }
}
struct Builder<TIncluded, TExcluded> {
    cur_pos: u64,
    included: Vec<TIncluded>,
    excluded: Vec<TExcluded>,
}

impl<TIncluded, TExcluded> Builder<TIncluded, TExcluded>
where
    TIncluded: TryFrom<u64, Error: Display>,
    TExcluded: TryFrom<u64, Error: Display>,
{
    fn new<TRange>(first_range: TRange, size_hint: usize) -> Result<Self, io::Error>
    where
        TRange: CreateRange<Item: TryInto<u64, Error: Display>>,
    {
        let (start_u64, end_u64) = (
            first_range.start().try_into().map_err(invalid_data)?,
            first_range.end().try_into().map_err(invalid_data)?,
        );
        let first_len = create_checked(start_u64, end_u64)?;
        let initial_offset = TExcluded::try_from(start_u64).map_err(invalid_data)?;
        let mut included = Vec::<TIncluded>::with_capacity(size_hint);
        let mut excluded = Vec::<TExcluded>::with_capacity(size_hint);
        included.push(first_len);
        excluded.push(initial_offset);
        Ok(Self {
            included,
            excluded,
            cur_pos: end_u64,
        })
    }

    fn add<TRange>(&mut self, range: TRange) -> Result<(), io::Error>
    where
        TRange: CreateRange<Item: TryInto<u64, Error: Display>>,
    {
        let (start_u64, end_u64) = (
            range.start().try_into().map_err(invalid_data)?,
            range.end().try_into().map_err(invalid_data)?,
        );
        self.excluded.push(create_checked(self.cur_pos, start_u64)?);
        self.included.push(create_checked(start_u64, end_u64)?);

        // let gap = start_u64.checked_sub(self.cur_pos).ok_or_else(|| {
        //     io::Error::new(
        //         io::ErrorKind::InvalidData,
        //         format!(
        //             "start ({start_u64}) must be >= previous end ({})",
        //             self.cur_pos
        //         ),
        //     )
        // })?;
        // let len: u64 = end_u64.checked_sub(start_u64).ok_or_else(|| {
        //     io::Error::new(
        //         io::ErrorKind::InvalidData,
        //         format!("end ({end_u64}) must be > start ({start_u64})"),
        //     )
        // })?;
        // if gap == 0 {
        //     *self.included.last_mut().expect("at least one range") =
        //         TIncluded::try_from(end_u64 - self.cur_included_start).map_err(invalid_data)?;
        // } else {
        //     self.excluded
        //         .push(TExcluded::try_from(gap).map_err(invalid_data)?);
        //     self.included
        //         .push(TIncluded::try_from(len).map_err(invalid_data)?);
        //     self.cur_included_start = start_u64;
        // }
        self.cur_pos = end_u64;
        Ok(())
    }
    fn build(self, bounds: Rect<u32>) -> SortedRanges<TIncluded, TExcluded> {
        SortedRanges {
            included: self.included,
            excluded: self.excluded,
            bounds,
        }
    }

    fn build_global(self, width: NonZeroU32) -> io::Result<SortedRanges<TIncluded, TExcluded>> {
        let height = u32::try_from(self.cur_pos / NonZeroU64::from(width) + 1)
            .ok()
            .and_then(NonZero::new)
            .ok_or_else(|| io::Error::other("Height is > u32"))?;
        Ok(SortedRanges {
            included: self.included,
            excluded: self.excluded,
            bounds: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
        })
    }
}
fn create_checked<T>(start: u64, end: u64) -> Result<T, io::Error>
where
    T: TryFrom<u64, Error: Display>,
{
    if end <= start {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("end ({end}) must be > start ({start})"),
        ));
    }
    T::try_from(end - start).map_err(invalid_data)
}

impl<TIncluded, TExcluded> SortedRanges<TIncluded, TExcluded> {
    pub fn new<TRange>(r: NonZeroRange<TRange>, bounds: Rect<u32>) -> Self
    where
        TRange: UncheckedCast<TIncluded> + UncheckedCast<TExcluded> + Sub<Output = TRange>,
        TIncluded: TryFrom<u64>,
    {
        assert!(bounds.x == 0);
        assert!(bounds.y == 0);
        Self {
            included: vec![r.len().cast_unchecked()],
            excluded: vec![r.start.cast_unchecked()],
            bounds,
        }
    }

    /// Collects
    pub fn try_from_ordered_iter<TIter>(iter: TIter) -> Result<Self, io::Error>
    where
        TIter: IntoIterator<
                Item: CreateRange<Item: TryInto<u64, Error: Display>>,
                IntoIter: ImageDimension,
            >,
        TIncluded: TryFrom<u64, Error: Display>,
        TExcluded: TryFrom<u64, Error: Display>,
    {
        let iter = iter.into_iter();
        let width = iter.width();
        Self::try_from_ordered_iter_roi_internal(iter).and_then(|x| x.build_global(width))
    }

    pub fn try_from_ordered_iter_roi<TIter>(
        iter: TIter,
        bounds: Rect<u32>,
    ) -> Result<Self, io::Error>
    where
        TIter: IntoIterator<Item: CreateRange<Item: TryInto<u64, Error: Display>>>,
        TIncluded: TryFrom<u64, Error: Display>,
        TExcluded: TryFrom<u64, Error: Display>,
    {
        Self::try_from_ordered_iter_roi_internal(iter).map(|r| r.build(bounds))
    }
    fn try_from_ordered_iter_roi_internal<TIter>(
        iter: TIter,
    ) -> Result<Builder<TIncluded, TExcluded>, io::Error>
    where
        TIter: IntoIterator<Item: CreateRange<Item: TryInto<u64, Error: Display>>>,
        TIncluded: TryFrom<u64, Error: Display>,
        TExcluded: TryFrom<u64, Error: Display>,
    {
        let mut iter = iter.into_iter();
        let Some(first_range) = iter.next() else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Requires at least one item",
            ));
        };
        let mut builder = Builder::new(first_range, iter.size_hint().0 + 1)?;

        for x in iter {
            builder.add(x)?;
        }

        Ok(builder)
    }

    /// Returns the number of ranges
    #[allow(clippy::len_without_is_empty, reason = "Cannot be empty")]
    pub fn len(&self) -> usize {
        self.included.len()
    }

    // Returns the number of ranges
    pub fn len_nonzero(&self) -> NonZero<usize> {
        NonZero::new(self.included.len())
            .expect("Constructors make sure, there is always at least one Range")
    }

    pub fn iter_roi<T: CreateRange>(
        &self,
    ) -> SortedRangesIter<
        std::iter::Copied<std::slice::Iter<'_, TIncluded>>,
        std::iter::Copied<std::slice::Iter<'_, TExcluded>>,
        T,
    >
    where
        TIncluded: UncheckedCast<T::Item>,
        TExcluded: UncheckedCast<T::Item>,
        T::Item: Default + Copy + SignedNonZeroable + Add<Output = T::Item>,
    {
        SortedRangesIter::new(
            self.included.iter().copied(),
            self.excluded.iter().copied(),
            T::Item::default(),
            self.bounds.width,
            self.bounds.height,
        )
    }
    pub fn iter_roi_owned<T: CreateRange>(
        self,
    ) -> SortedRangesIter<std::vec::IntoIter<TIncluded>, std::vec::IntoIter<TExcluded>, T>
    where
        TIncluded: UncheckedCast<T::Item>,
        TExcluded: UncheckedCast<T::Item>,
        T::Item: Default + Copy + SignedNonZeroable + Add<Output = T::Item>,
    {
        SortedRangesIter::new(
            self.included.into_iter(),
            self.excluded.into_iter(),
            T::Item::default(),
            self.bounds.width,
            self.bounds.height,
        )
    }
    pub fn spans<T>(
        &self,
    ) -> SortedRangesSpanIter<
        SortedRangesIter<
            std::iter::Copied<std::slice::Iter<'_, TIncluded>>,
            std::iter::Copied<std::slice::Iter<'_, TExcluded>>,
            NonZeroRange<T>,
        >,
    >
    where
        NonZeroRange<T>: CreateRange<Item = T>,
        TIncluded: UncheckedCast<T>,
        TExcluded: UncheckedCast<T>,
        T: Default + Copy + SignedNonZeroable + Add<Output = T>,
    {
        SortedRangesSpanIter::new(self.iter_roi::<NonZeroRange<T>>())
    }

    pub fn spans_owned<T>(
        self,
    ) -> SortedRangesSpanIter<
        SortedRangesIter<
            std::vec::IntoIter<TIncluded>,
            std::vec::IntoIter<TExcluded>,
            NonZeroRange<T>,
        >,
    >
    where
        NonZeroRange<T>: CreateRange<Item = T>,
        TIncluded: UncheckedCast<T>,
        TExcluded: UncheckedCast<T>,
        T: Default + Copy + SignedNonZeroable + Add<Output = T>,
    {
        SortedRangesSpanIter::new(self.iter_roi_owned::<NonZeroRange<T>>())
    }

    pub fn iter_global_with<T: CreateRange>(
        &self,
        width: NonZeroU32,
    ) -> SortedRangesIterGlobal<
        std::iter::Copied<std::slice::Iter<'_, TIncluded>>,
        std::iter::Copied<std::slice::Iter<'_, TExcluded>>,
        T,
    >
    where
        TIncluded: UncheckedCast<T::Item>,
        TExcluded: UncheckedCast<T::Item>,
        T::Item: Default
            + Copy
            + SignedNonZeroable
            + Add<Output = T::Item>
            + Sub<Output = T::Item>
            + Mul<Output = T::Item>
            + Div<Output = T::Item>
            + Rem<Output = T::Item>
            + Ord,
        u32: UncheckedCast<T::Item>,
    {
        SortedRangesIterGlobal::new(
            self.included.iter().copied(),
            self.excluded.iter().copied(),
            self.bounds.width,
            width,
            NonZeroU32::new(self.bounds.height.get() + self.bounds.y).unwrap(),
        )
    }
    pub fn iter_global_owned_with<T: CreateRange>(
        self,
        width: NonZeroU32,
    ) -> SortedRangesIterGlobal<std::vec::IntoIter<TIncluded>, std::vec::IntoIter<TExcluded>, T>
    where
        TIncluded: UncheckedCast<T::Item>,
        TExcluded: UncheckedCast<T::Item>,
        T::Item: Default
            + Copy
            + SignedNonZeroable
            + Add<Output = T::Item>
            + Sub<Output = T::Item>
            + Mul<Output = T::Item>
            + Div<Output = T::Item>
            + Rem<Output = T::Item>
            + Ord,
        u32: UncheckedCast<T::Item>,
    {
        SortedRangesIterGlobal::new(
            self.included.into_iter(),
            self.excluded.into_iter(),
            self.bounds.width,
            width,
            NonZeroU32::new(self.bounds.height.get() + self.bounds.y).unwrap(),
        )
    }
}

impl<TIncluded, TExcluded> ImageDimension for SortedRanges<TIncluded, TExcluded> {
    fn bounds(&self) -> Rect<u32> {
        self.bounds
    }
    fn width(&self) -> NonZero<u32> {
        self.bounds.width
    }
}

#[cfg(test)]
mod tests {
    use std::ops::{Range, RangeInclusive};

    use testresult::TestResult;

    use crate::{NonZeroRange, Rect};

    use super::*;

    const TEST_BOUNDS: Rect<u32> = Rect::new(
        0,
        0,
        NonZero::new(1000u32).unwrap(),
        NonZero::new(1000u32).unwrap(),
    );

    #[test]
    fn get_spans() -> TestResult {
        let input = SortedRanges::<u32, u32>::try_from_ordered_iter(
            [0..1000u32, 1001..2000].with_roi(TEST_BOUNDS),
        )?;
        let spans = input.spans_owned::<u32>().collect::<Vec<_>>();
        assert_eq!(
            vec!(
                Span {
                    y: 0,
                    x: (0..1000).try_into()?
                },
                Span {
                    y: 1,
                    x: (1..1000).try_into()?
                },
            ),
            spans
        );
        Ok(())
    }

    #[cfg(feature = "range-set-blaze-0_5")]
    #[test]
    fn combine_inline() {
        let a = SortedRanges::<u8, u8>::try_from_ordered_iter_roi([10u32..20, 30..40], TEST_BOUNDS)
            .unwrap();
        let b = SortedRanges::<u8, u8>::try_from_ordered_iter_roi([20u32..30, 41..45], TEST_BOUNDS)
            .unwrap();

        let b_iter = b.iter_roi::<RangeInclusive<u64>>();
        let a = a
            .map_inplace(|a_iter| range_set_blaze_0_5::SortedDisjoint::union(b_iter, a_iter))
            .unwrap();

        assert_eq!(
            vec![10u64..40, 41..45],
            a.iter_roi_owned().collect::<Vec<_>>()
        );
        assert_eq!(
            vec![20u64..30, 41..45],
            b.iter_roi_owned().collect::<Vec<_>>()
        );
    }

    #[test]
    fn ranges_starting_at_zero() {
        let map =
            SortedRanges::<u32, u32>::try_from_ordered_iter_roi([0u64..1, 5u64..6], TEST_BOUNDS);

        let map = map.unwrap();
        let collected: Vec<_> = map.iter_roi::<std::ops::Range<u64>>().collect();
        assert_eq!(vec![0u64..1, 5u64..6], collected);
    }

    #[test]
    fn split_when_collection_becomes_bigger() {
        let a = SortedRanges::<u8, u8>::try_from_ordered_iter_roi([10u32..15, 30..35], TEST_BOUNDS)
            .unwrap();

        let a = a
            .map_inplace(|iter| {
                iter.flat_map(|x| {
                    let with_offset = (*x.start() + 10)..=(*x.end() + 10);
                    [x, with_offset]
                })
            })
            .unwrap();

        assert_eq!(
            vec![10u64..15, 20..25, 30..35, 40..45],
            a.iter_roi_owned().collect::<Vec<_>>()
        );
    }

    #[test]
    fn split_returns_none_when_empty() {
        let a =
            SortedRanges::<u8, u8>::try_from_ordered_iter_roi([10u32..15], TEST_BOUNDS).unwrap();

        let result = a.map_inplace(|_| std::iter::empty());

        assert!(result.is_none());
    }

    #[test]
    fn range_with_initial_offset() {
        let encoded =
            SortedRanges::<u8, u8>::try_from_ordered_iter_roi([10u32..20, 255..257], TEST_BOUNDS)
                .unwrap();
        assert_eq!(
            vec![10u64..=19, 255u64..=256],
            encoded.iter_roi_owned().collect::<Vec<_>>()
        );
    }

    #[test]
    fn owned_iterator() {
        let encoded =
            SortedRanges::<u8, u8>::try_from_ordered_iter_roi([10u32..20, 255..257], TEST_BOUNDS)
                .unwrap();
        let collected: Vec<_> = encoded.iter_roi_owned().collect();
        assert_eq!(2, collected.len());
        assert_eq!(10u64..=19, collected[0]);
        assert_eq!(255u64..=256, collected[1]);
    }
    #[test]
    fn assert_big_gap_causes_error() {
        let error =
            SortedRanges::<u16, u8>::try_from_ordered_iter_roi([10u32..20, 276..280], TEST_BOUNDS)
                .unwrap_err();
        assert!(error.to_string().contains("out of range"), "{error}");
    }

    #[test]
    fn assert_big_ranges_cause_error() {
        let error = SortedRanges::<u8, u16>::try_from_ordered_iter_roi([10u32..280], TEST_BOUNDS)
            .unwrap_err();
        assert!(error.to_string().contains("out of range"), "{error}");
    }
    #[test]
    fn zero_ranges_cause_error() {
        let error = SortedRanges::<u8, u8>::try_from_ordered_iter_roi([10u32..10], TEST_BOUNDS)
            .unwrap_err();
        assert!(error.to_string().contains("must be >"), "{error}");
    }

    #[test]
    fn overlapping_cause_error() {
        let error =
            SortedRanges::<u8, u8>::try_from_ordered_iter_roi([10u32..12, 11..12], TEST_BOUNDS)
                .unwrap_err();
        assert!(error.to_string().contains("must be >"), "{error}");
    }

    #[test]
    fn iterate_with_different_output_types() {
        let encoded =
            SortedRanges::<u8, u8>::try_from_ordered_iter_roi([10u32..15, 30..35], TEST_BOUNDS)
                .unwrap();

        let as_range: Vec<_> = encoded.iter_roi::<Range<u64>>().collect();
        assert_eq!(vec![10u64..15, 30..35], as_range);

        let as_range_inclusive: Vec<_> = encoded.iter_roi::<RangeInclusive<u64>>().collect();
        assert_eq!(vec![10u64..=14, 30..=34], as_range_inclusive);

        let as_nonzero_range: Vec<_> = encoded.iter_roi::<NonZeroRange<u64>>().collect();
        assert_eq!(
            vec![NonZeroRange::new(10u64..15), NonZeroRange::new(30..35)],
            as_nonzero_range
        );
    }

    #[test]
    fn iter_global_with_different_widths() {
        let rect = Rect::new(2u32, 1, NonZero::new(4).unwrap(), NonZero::new(3).unwrap());
        let global_width = NonZero::new(10u32).unwrap();
        let ranges = SortedRanges::<u16, u16>::try_from_ordered_iter(
            rect.into_rect_iter::<std::ops::Range<u32>>(global_width),
        )
        .unwrap();

        let width_smaller = NonZero::new(3u32).unwrap();
        let width_equal = NonZero::new(10u32).unwrap();
        let width_bigger = NonZero::new(20u32).unwrap();

        let with_smaller: Vec<_> = ranges
            .iter_global_with::<Range<u64>>(width_smaller)
            .collect();
        assert_eq!(with_smaller, vec![5..6, 8..9, 11..12]);

        let with_equal: Vec<_> = ranges.iter_global_with::<Range<u64>>(width_equal).collect();
        assert_eq!(with_equal, vec![12u64..16, 22..26, 32..36]);

        let with_bigger: Vec<_> = ranges
            .iter_global_with::<Range<u64>>(width_bigger)
            .collect();
        assert_eq!(with_bigger, vec![22..26, 42..46, 62..66]);
    }
    #[test]
    fn iter_global_with_different_widths_full_rect_width() {
        let rect = Rect::new(0u32, 1, NonZero::new(10).unwrap(), NonZero::new(3).unwrap());
        let global_width = NonZero::new(10u32).unwrap();
        let ranges = SortedRanges::<u16, u16>::try_from_ordered_iter(
            rect.into_rect_iter::<std::ops::Range<u32>>(global_width),
        )
        .unwrap();
        assert_eq!(1, ranges.included.len());

        let width_smaller = NonZero::new(3u32).unwrap();
        let width_equal = NonZero::new(10u32).unwrap();
        let width_bigger = NonZero::new(20u32).unwrap();

        let with_smaller: Vec<_> = ranges
            .iter_global_with::<Range<u64>>(width_smaller)
            .collect();
        assert_eq!(with_smaller, vec![3..12]);

        let with_equal: Vec<_> = ranges.iter_global_with::<Range<u64>>(width_equal).collect();
        assert_eq!(with_equal, vec![10u64..40]);

        let with_bigger: Vec<_> = ranges
            .iter_global_with::<Range<u64>>(width_bigger)
            .collect();
        assert_eq!(with_bigger, vec![20..30, 40..50, 60..70]);
    }

    #[test]
    fn iter_global_with_multiple_in_same_line() {
        const SIZE: NonZero<u32> = NonZero::new(20).unwrap();
        let ranges = SortedRanges::<u16, u16>::try_from_ordered_iter(
            [0u32..1, 3..4, 8..11, 13..14, 19..21].with_bounds(SIZE, SIZE),
        )
        .unwrap();

        let with_smaller: Vec<_> = ranges
            .iter_global_with::<Range<u32>>(NonZero::new(10u32).unwrap())
            .collect();
        assert_eq!(with_smaller, vec![0u32..1, 3..4, 8..11]);
    }

    #[test]
    fn clip_full_rect_produces_single_range() {
        const GLOBAL_WIDTH: NonZeroU32 = NonZero::new(10u32).unwrap();
        const RECT_SIZE: NonZeroU32 = NonZero::new(5).unwrap();
        let rect = Rect::new(2u32, 2, RECT_SIZE, RECT_SIZE);

        let ranges = rect
            .into_rect_iter::<Range<u32>>(GLOBAL_WIDTH)
            .try_clip_2d(rect)
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(1, ranges.len());
        let ranges = SortedRanges::<u16, u16>::try_from_ordered_iter_roi(
            ranges.with_bounds(RECT_SIZE, RECT_SIZE),
            rect,
        )
        .unwrap();

        assert_eq!(1, ranges.len());
        let roi: Vec<_> = ranges.iter_roi::<Range<u64>>().collect();
        assert_eq!(vec![0u64..25], roi);
    }
}

pub trait IntoRoiIterator {
    type IntoIter: Iterator<Item = Self::Item>;
    type Item: CreateRange;

    fn into_roi_iterator() -> Self::IntoIter;
}
