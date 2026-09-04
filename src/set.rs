use std::{
    cmp::Ord,
    fmt::{Debug, Display},
    io,
    num::{IntErrorKind, NonZero, NonZeroU32},
    ops::{Add, Div, Mul, Rem, Sub},
};

use crate::visualize_iter::IterVisualizer;
use crate::{
    CreateRange, ImageDimension, IncompatibleSizeError, IntoPipelineOutput, MaybeResult,
    NonZeroRange, PipelineError, Rect, SignedNonZeroable, SortedRangesSpanIter, Span,
    UncheckedCast, WithBounds, WithRoi,
    span::{ClipSpanIter, FoldInlineSpanIter},
};

fn invalid<T: Display>(e: T) -> std::io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

mod bounds_inspector;
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
mod span_offsets_iter;
// mod split_rows;

pub use bounds_inspector::*;
#[cfg(feature = "range-set-blaze-0_5")]
pub use dilate::*;
pub use iter::*;
pub use iter_global::*;
pub use map_inplace::*;
pub use offsets_iter::*;
pub use rect::*;
pub use sanitize_sorted_disjoint::*;
pub use span_offsets_iter::*;
// pub use split_rows::*;

pub(crate) type SortedRangesSliceIter<'a, TIncluded, TExcluded, T> = SortedRangesIter<
    std::iter::Copied<std::slice::Iter<'a, TIncluded>>,
    std::iter::Copied<std::slice::Iter<'a, TExcluded>>,
    T,
>;

pub(crate) type SortedRangesOwnedSpanIter<T, TRange> = SortedRangesSpanIter<
    SortedRangesIter<std::vec::IntoIter<T>, std::vec::IntoIter<T>, NonZeroRange<TRange>>,
>;

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
    /// In contrast to std::iter::inspect, `fold_inline` calls the lambda on all inputs spans,
    /// if `Self::finish` is called. The function deliberately uses Fn rather than FnMut,
    /// to force the caller to use the accumulator instead of `&mut other_state`, which can only be obtained via `Self::finish`
    /// and thus is guaranteed to use all input spans, even if a consumer of Self doesn't drive it to completion.
    ///
    /// If you only want to inspect spans which the consumer consumed, you can create a
    /// `InlineAccumulatorSpanIter::new` yourself, which doesn't have the `A: 'static` restriction
    /// and accepts a `FnMut` to bypass accumulator entirely (accumulator could then be `()`)
    /// ```
    /// use std::num::NonZeroU32;
    /// use imask::{Rect, ImaskSet, ImageDimension};
    ///
    /// const SIZE: NonZeroU32 = NonZeroU32::new(10).unwrap();
    /// let spans = Rect::new(10u32, 20, SIZE, SIZE).into_spans();
    /// let mut count = 0;
    /// let mut inspect = spans.clone().fold_inline(0, |a, _r| {
    ///     *a += 1;
    /// });
    /// assert_eq!(spans.bounds(), inspect.bounds());
    /// assert_eq!(9, (&mut inspect).take(9).count());
    /// assert_eq!(10, inspect.finish_all());
    /// ```
    fn fold_inline<F, A>(self, accumulator: A, f: F) -> FoldInlineSpanIter<Self::IntoIter, F, A>
    where
        F: Fn(&mut A, &<Self::IntoIter as Iterator>::Item),
        A: 'static,
    {
        FoldInlineSpanIter::new(self.into_iter(), accumulator, f)
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

    fn intersect<TOther: IntoIterator<Item = Span<T>>, T>(
        self,
        other: TOther,
    ) -> crate::span::Intersect<Self::IntoIter, TOther::IntoIter> {
        crate::span::Intersect::new(self.into_iter(), other.into_iter())
    }

    #[allow(clippy::type_complexity)]
    fn union_all(
        self,
    ) -> Result<
        crate::span::UnionAll<
            <<Self::Item as MaybeResult>::Ok as std::iter::IntoIterator>::IntoIter,
        >,
        <<Self::Item as MaybeResult>::Err as IntoPipelineOutput>::Output,
    >
    where
        Self::Item: MaybeResult<
                Ok: std::iter::IntoIterator<
                    Item: Ord + Copy + std::fmt::Debug,
                    IntoIter: ImageDimension,
                >,
                Err: IntoPipelineOutput,
            >,
    {
        crate::span::UnionAll::new(self)
    }

    fn cluster<T>(self) -> crate::span::ClusterSpanIter<Self::IntoIter, T>
    where
        Self::IntoIter: Iterator<Item = Span<T>> + ImageDimension + std::iter::FusedIterator,
        T: Ord
            + Copy
            + std::fmt::Debug
            + std::ops::Add<Output = T>
            + std::ops::Sub<Output = T>
            + num_traits::One
            + UncheckedCast<u32>,
    {
        crate::span::ClusterSpanIter::new(self.into_iter())
    }

    fn clip<T>(self, roi: Rect<u32>) -> ClipSpanIter<Self::IntoIter, T>
    where
        Self::IntoIter: Iterator<Item = Span<T>> + ImageDimension,
        T: SignedNonZeroable
            + TryFrom<u32, Error: Debug>
            + Ord
            + Add<Output = T>
            + Sub<Output = T>
            + Copy
            + Debug,
    {
        ClipSpanIter::new(self.into_iter(), roi)
    }

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
    #[deprecated(
        since = "0.0.1",
        note = "use dilate_within, which allows specifying the region of interest"
    )]
    fn dilate<T>(
        self,
        offset: <T as SignedNonZeroable>::NonZero,
    ) -> Result<crate::span::DilateSpanIterAcc<WithRoi<Self::IntoIter>, T>, PipelineError>
    where
        T: Ord
            + Copy
            + Debug
            + Add<Output = T>
            + num_traits::SaturatingSub<Output = T>
            + num_traits::One
            + num_traits::Zero
            + SignedNonZeroable
            + UncheckedCast<u32>
            + UncheckedCast<u64>
            + TryFrom<u64, Error: Into<IncompatibleSizeError>>,
        u32: UncheckedCast<T>,
        Self::IntoIter: Iterator<Item = Span<T>> + ImageDimension,
    {
        let iter = self.into_iter();
        let radius: u32 = offset.into().cast_unchecked();
        // Extending the declared input bounds keeps the "spans stay within bounds" contract
        // valid for the dilation without changing the produced spans.
        let roi = iter.bounds().expand(radius);
        crate::span::DilateSpanIterAcc::new(iter.with_roi(roi), offset)
    }

    /// Dilates by `offset`, declaring `roi` as region of interest of the input.
    ///
    /// The effective region of interest is the intersection of `roi` with the bounds the
    /// input declares ([`ImageDimension::bounds`]); if they don't overlap,
    /// [`PipelineError::Empty`] is returned. Input spans (partially) outside that region
    /// are clipped — spans entirely outside dilate to nothing.
    fn dilate_within<T>(
        self,
        offset: <T as SignedNonZeroable>::NonZero,
        roi: Rect<u32>,
    ) -> Result<crate::span::DilateSpanIterAcc<WithRoi<Self::IntoIter>, T>, PipelineError>
    where
        T: Ord
            + Copy
            + Debug
            + Add<Output = T>
            + num_traits::SaturatingSub<Output = T>
            + num_traits::One
            + num_traits::Zero
            + SignedNonZeroable
            + UncheckedCast<u32>
            + UncheckedCast<u64>
            + TryFrom<u64, Error: Into<IncompatibleSizeError>>,
        u32: UncheckedCast<T>,
        Self::IntoIter: Iterator<Item = Span<T>> + ImageDimension,
    {
        let iter = self.into_iter();
        let roi = iter
            .bounds()
            .intersection(&roi)
            .ok_or(PipelineError::Empty)?;
        crate::span::DilateSpanIterAcc::new(iter.with_roi(roi), offset)
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
                          + num_traits::SaturatingSub<Output = <Self::Item as CreateRange>::Item>
                          + num_traits::CheckedSub<Output = <Self::Item as CreateRange>::Item>
                          + Copy
                          + range_set_blaze_0_5::Integer
                          + num_traits::Zero
                          + num_traits::One,
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
/// The values are expected to always be > 0 (except the first exclude might be 0)
/// Included represents the number of pixels to include, excluded encodes the gap between two included ranges
///
///
#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive))]
pub struct SortedRanges<T> {
    included: Vec<T>,
    excluded: Vec<T>,
    bounds: Rect<u32>,
}
impl<T: UncheckedCast<u64>> Debug for SortedRanges<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SortedRanges")
            .field("bounds", &self.bounds)
            .field(
                "spans",
                &format_args!(
                    "{}",
                    IterVisualizer::<_, _, 10>::new_with_size(self.spans::<u64>(), self.len())
                ),
            )
            .finish()
    }
}
struct Builder<T> {
    cur_pos: u64,
    included: Vec<T>,
    excluded: Vec<T>,
}

impl<T> Builder<T>
where
    T: TryFrom<u64, Error: Display>,
{
    fn new<TRange>(first_range: TRange, size_hint: usize) -> Result<Self, io::Error>
    where
        TRange: CreateRange<Item: TryInto<u64, Error: Display>>,
    {
        let (start_u64, end_u64) = (
            first_range.start().try_into().map_err(invalid)?,
            first_range.end().try_into().map_err(invalid)?,
        );
        let first_len = create_checked(start_u64, end_u64)?;
        let initial_offset = T::try_from(start_u64).map_err(invalid)?;
        let mut included = Vec::<T>::with_capacity(size_hint);
        let mut excluded = Vec::<T>::with_capacity(size_hint);
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
            range.start().try_into().map_err(invalid)?,
            range.end().try_into().map_err(invalid)?,
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
    fn build(self, bounds: Rect<u32>) -> SortedRanges<T> {
        SortedRanges {
            included: self.included,
            excluded: self.excluded,
            bounds,
        }
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
    T::try_from(end - start).map_err(invalid)
}

/// Builds a [`SortedRanges`] from spans, merging touching spans.
///
/// (`new` takes no span) and `add` allows `start > merge_end || excluded.is_empty()`:
/// the very first span initializes the merge window, equal starts/ends merge touching
/// spans, and only `start < merge_end` (overlap) is an error.
struct SortedRangesSpanBuilderInternal<T> {
    width_u64: u64,
    offset_x_u64: u64,
    offset_y_u64: u64,
    merge_start: u64,
    merge_end: u64,
    bounds: Rect<u32>,
    excluded: Vec<T>,
    included: Vec<T>,
}

impl<T> SortedRangesSpanBuilderInternal<T>
where
    T: TryFrom<u64, Error: Into<IncompatibleSizeError>>,
    IncompatibleSizeError: From<T::Error>,
{
    fn new(bounds: Rect<u32>, size_hint: usize) -> Self {
        Self {
            width_u64: bounds.width.get() as u64,
            offset_x_u64: bounds.x as u64,
            offset_y_u64: bounds.y as u64,
            merge_start: 0,
            merge_end: 0,
            bounds,
            excluded: Vec::with_capacity(size_hint),
            included: Vec::with_capacity(size_hint),
        }
    }

    fn add<TSpan>(&mut self, span: Span<TSpan>) -> Result<(), IncompatibleSizeError>
    where
        TSpan: Copy + TryInto<u64>,
        IncompatibleSizeError: From<TSpan::Error>,
    {
        let global_y: u64 = span.y.try_into()?;
        // Spans are expected to always stay within the declared bounds
        // (ImageDimension). A span below the ROI offset violates that invariant
        // and is therefore a programmer error.
        let local_y = global_y
            .checked_sub(self.offset_y_u64)
            .ok_or(IntErrorKind::NegOverflow)?;
        let global_x_start: u64 = span.x.start.try_into()?;
        let global_x_end: u64 = span.x.end.try_into()?;
        let local_x_start = global_x_start
            .checked_sub(self.offset_x_u64)
            .ok_or(IntErrorKind::NegOverflow)?;
        let local_x_end = global_x_end
            .checked_sub(self.offset_x_u64)
            .ok_or(IntErrorKind::NegOverflow)?;

        let span_offset = local_y * self.width_u64;
        let start = span_offset + local_x_start;
        let end = span_offset + local_x_end;

        if self.excluded.is_empty() || start > self.merge_end {
            if !self.excluded.is_empty() {
                let included = T::try_from(self.merge_end - self.merge_start);
                self.included.push(included?);
            }
            let excluded = T::try_from(start - self.merge_end);
            self.excluded.push(excluded?);
            self.merge_start = start;
            self.merge_end = end;
        } else if start == self.merge_end {
            self.merge_end = end;
        } else {
            // start < merge_end (and excluded not empty): overlap violates the
            // sorted & disjoint contract span iterators promise → programmer error.
            return Err(IntErrorKind::NegOverflow.into());
        }
        Ok(())
    }

    fn build(self) -> Result<SortedRanges<T>, PipelineError> {
        if self.excluded.is_empty() {
            return Err(PipelineError::Empty);
        }
        let Self {
            mut included,
            excluded,
            merge_start,
            merge_end,
            bounds,
            ..
        } = self;
        let include = T::try_from(merge_end - merge_start);
        included.push(include.map_err(IncompatibleSizeError::from)?);
        Ok(SortedRanges {
            included,
            excluded,
            bounds,
        })
    }
}

/// Builds a [`SortedRanges`] from spans where [`SortedRangesSpanBuilder::add`] is infallible.
///
/// This makes it suitable for use with [`ImaskSet::fold_inline`]: the first error is captured
/// internally and only surfaced by [`SortedRangesSpanBuilder::build`].
///
/// ```
/// use std::num::NonZeroU32;
/// use imask::{ImaskSet, Rect, SortedRanges, SortedRangesSpanBuilder, Span};
///
/// const SIZE: NonZeroU32 = NonZeroU32::new(10).unwrap();
/// let rect = Rect::new(10, 10, SIZE, SIZE);
/// let mut builder = SortedRangesSpanBuilder::<u32>::new(rect);
/// let mut iter = rect.into_spans().fold_inline(builder, |b, s| b.add(*s));
/// iter.next();
/// let ranges = iter.finish_all().build().unwrap();
/// assert_eq!(
///     SortedRanges::try_from_span_iter(rect.into_spans()).unwrap(),
///     ranges
/// );
/// ```
pub struct SortedRangesSpanBuilder<T> {
    builder: SortedRangesSpanBuilderInternal<T>,
    error: Option<IncompatibleSizeError>,
}

impl<T> SortedRangesSpanBuilder<T>
where
    T: TryFrom<u64>,
    IncompatibleSizeError: From<T::Error>,
{
    pub fn new(bounds: Rect<u32>) -> Self {
        Self {
            builder: SortedRangesSpanBuilderInternal::new(bounds, 0),
            error: None,
        }
    }

    pub fn add<TSpan: Copy + TryInto<u64>>(&mut self, span: Span<TSpan>)
    where
        IncompatibleSizeError: From<TSpan::Error>,
    {
        if self.error.is_none() {
            self.error = self.builder.add(span).err();
        }
    }

    pub fn build(self) -> Result<SortedRanges<T>, PipelineError> {
        if let Some(error) = self.error {
            return Err(error.into());
        }
        self.builder.build()
    }
}

impl<T: SignedNonZeroable + UncheckedCast<u32> + Sub<Output = T>> From<Span<T>> for SortedRanges<T>
where
    Rect<T>: From<Span<T>>,
{
    fn from(span: Span<T>) -> Self {
        let bounds = Rect::<T>::from(span).cast_unchecked::<u32>();
        #[allow(clippy::eq_op, reason = "Avoid additional bound on num_traits::Zero")]
        let zero = span.x.start - span.x.start;
        Self {
            included: vec![span.x.len()],
            excluded: vec![zero],
            bounds,
        }
    }
}

impl<T> SortedRanges<T> {
    #[deprecated = "Use from_span instead, which automatically sets the correct bounds"]
    pub fn new<TRange>(r: NonZeroRange<TRange>, bounds: Rect<u32>) -> Self
    where
        TRange: UncheckedCast<T> + Sub<Output = TRange>,
        T: TryFrom<u64>,
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
        T: TryFrom<u64, Error: Display>,
    {
        let iter = iter.into_iter();
        let bounds = iter.bounds();
        Self::try_from_ordered_iter_roi_internal(iter).map(|r| r.build(bounds))
    }

    #[deprecated = "Use `try_from_ordered_iter(input.with_roi(bounds))` instead"]
    pub fn try_from_ordered_iter_roi<TIter>(
        iter: TIter,
        bounds: Rect<u32>,
    ) -> Result<Self, io::Error>
    where
        TIter: IntoIterator<Item: CreateRange<Item: TryInto<u64, Error: Display>>>,
        T: TryFrom<u64, Error: Display>,
    {
        Self::try_from_ordered_iter(iter.with_roi(bounds))
    }
    pub fn try_from_span_iter<TIter, TSpan>(iter: TIter) -> Result<Self, PipelineError>
    where
        TIter: IntoIterator<Item = Span<TSpan>, IntoIter: ImageDimension>,
        TSpan: Copy + TryInto<u64>,
        T: TryFrom<u64, Error: Display>,
        IncompatibleSizeError: From<TSpan::Error>,
        IncompatibleSizeError: From<T::Error>,
    {
        let iter = iter.into_iter();
        let bounds = iter.bounds();
        debug_assert_eq!(
            iter.width(),
            bounds.width,
            "width() must equal bounds().width"
        );
        let size_hint = iter.size_hint().0;
        let mut builder = SortedRangesSpanBuilderInternal::<T>::new(bounds, size_hint);
        for span in iter {
            builder.add(span)?;
        }
        builder.build()
    }

    /// Collects spans while tracking the minimal bounds, then shrinks [`SortedRanges::bounds`]
    /// to those minimal bounds.
    ///
    /// The first pass builds with `input.bounds()` while tracking
    /// `min_x`/`max_x_end`/`min_y`/`max_y` (the same job
    /// [`BoundsInspector`](crate::BoundsInspector) does for flat ranges,
    /// but directly on spans so no extra pass is needed).
    ///
    /// Afterwards:
    /// - if the tracked min-bounds equal `input.bounds()`, the first result is returned as-is.
    /// - if only `y + height` is too big (same `x`, `y` and `width`), only [`Rect`] is adapted.
    /// - if `x`/`width` match but the `y`-offset is off, the absolute start
    ///   (`excluded[0]`) is shifted by `offset * width` and [`Rect`] is adapted.
    /// - otherwise (`x`-bounds don't match, hence the row stride changes),
    ///   the first result is re-encoded via
    ///   `first.into_spans().with_roi(min_bounds)` + [`SortedRanges::try_from_span_iter`].
    pub fn try_from_span_iter_minbounds<TIter, TSpan>(iter: TIter) -> Result<Self, PipelineError>
    where
        TIter: IntoIterator<Item = Span<TSpan>, IntoIter: ImageDimension>,
        TSpan: Copy + TryInto<u64>,
        T: TryFrom<u64, Error: Display> + UncheckedCast<u64> + UncheckedCast<u32> + Copy,
        IncompatibleSizeError: From<TSpan::Error>,
        IncompatibleSizeError: From<T::Error>,
    {
        let mut iter = iter.into_iter();
        let declared = iter.bounds();
        debug_assert_eq!(
            iter.width(),
            declared.width,
            "width() must equal bounds().width"
        );
        let size_hint = iter.size_hint().0;
        let mut builder = SortedRangesSpanBuilderInternal::<T>::new(declared, size_hint);

        let mut min_x = u64::MAX;
        let mut max_x_end = u64::MIN;
        let mut min_y = u64::MAX;
        let mut max_y = u64::MIN;

        for span in &mut iter {
            let x_start: u64 = span
                .x
                .start
                .try_into()
                .map_err(IncompatibleSizeError::from)?;
            let x_end: u64 = span.x.end.try_into().map_err(IncompatibleSizeError::from)?;
            let y: u64 = span.y.try_into().map_err(IncompatibleSizeError::from)?;
            min_x = min_x.min(x_start);
            max_x_end = max_x_end.max(x_end);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            builder.add(span)?;
        }
        let mut first = builder.build()?;

        let min_x_32 = u32::try_from(min_x).map_err(IncompatibleSizeError::from)?;
        let max_x_end_32 = u32::try_from(max_x_end).map_err(IncompatibleSizeError::from)?;
        let min_y_32 = u32::try_from(min_y).map_err(IncompatibleSizeError::from)?;
        let max_y_32 = u32::try_from(max_y).map_err(IncompatibleSizeError::from)?;
        let tight_width =
            NonZeroU32::new(max_x_end_32 - min_x_32).expect("non-empty spans imply non-zero width");
        let tight_height = NonZeroU32::new(max_y_32 - min_y_32 + 1)
            .expect("non-empty spans imply non-zero height");
        let tight = Rect::new(min_x_32, min_y_32, tight_width, tight_height);

        if tight == declared {
            return Ok(first);
        }

        if tight.x == declared.x && tight.width == declared.width {
            if tight.y == declared.y {
                // Only trailing empty rows: flat layout unchanged, shrink height.
                first.bounds = tight;
                return Ok(first);
            }
            // Same x/width, y-offset off: flat positions shift uniformly by
            // delta = (tight.y - declared.y) * width. Only the absolute start
            // (excluded[0]) stores an absolute position, the rest are deltas,
            // so a single adjustment suffices
            // (conceptually `buffer.for_each_mut(|v| *v += offset * width)`
            // on absolute positions).
            let declared_y_u64 = u64::from(declared.y);
            let tight_y_u64 = u64::from(tight.y);
            let width_u64 = declared.width.get() as u64;
            assert!(tight_y_u64 >= declared_y_u64);
            let delta = (tight_y_u64 - declared_y_u64) * width_u64;
            let first_u64: u64 = first.excluded[0].cast_unchecked();
            let adjusted = first_u64
                .checked_sub(delta)
                .ok_or(IntErrorKind::NegOverflow)?;
            first.excluded[0] = T::try_from(adjusted).map_err(IncompatibleSizeError::from)?;
            first.bounds = tight;
            return Ok(first);
        }

        // x-bounds don't match (or y moved outside): row stride changed,
        // full re-encode via spans is required.
        let overridden_roi = first.spans::<u32>().with_roi(tight);
        Self::try_from_span_iter(overridden_roi)
    }

    #[cfg(feature = "async-io")]
    pub(crate) fn from_parts(included: Vec<T>, excluded: Vec<T>, bounds: Rect<u32>) -> Self {
        Self {
            bounds,
            excluded,
            included,
        }
    }
    fn try_from_ordered_iter_roi_internal<TIter>(iter: TIter) -> Result<Builder<T>, io::Error>
    where
        TIter: IntoIterator<Item: CreateRange<Item: TryInto<u64, Error: Display>>>,
        T: TryFrom<u64, Error: Display>,
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

    pub fn iter_roi<TRange: CreateRange>(
        &self,
    ) -> SortedRangesIter<
        std::iter::Copied<std::slice::Iter<'_, T>>,
        std::iter::Copied<std::slice::Iter<'_, T>>,
        TRange,
    >
    where
        T: UncheckedCast<TRange::Item>,
        TRange::Item: Default + Copy + SignedNonZeroable + Add<Output = TRange::Item>,
    {
        SortedRangesIter::new(
            self.included.iter().copied(),
            self.excluded.iter().copied(),
            TRange::Item::default(),
            self.bounds,
        )
    }
    pub fn iter_roi_owned<TRange: CreateRange>(
        self,
    ) -> SortedRangesIter<std::vec::IntoIter<T>, std::vec::IntoIter<T>, TRange>
    where
        T: UncheckedCast<TRange::Item>,
        TRange::Item: Default + Copy + SignedNonZeroable + Add<Output = TRange::Item>,
    {
        SortedRangesIter::new(
            self.included.into_iter(),
            self.excluded.into_iter(),
            TRange::Item::default(),
            self.bounds,
        )
    }
    pub fn spans<TRange>(
        &self,
    ) -> SortedRangesSpanIter<SortedRangesSliceIter<'_, T, T, NonZeroRange<TRange>>>
    where
        NonZeroRange<TRange>: CreateRange<Item = TRange>,
        T: UncheckedCast<TRange>,
        TRange: Default + Copy + SignedNonZeroable + Add<Output = TRange>,
    {
        SortedRangesSpanIter::new(self.iter_roi::<NonZeroRange<TRange>>())
    }

    pub fn spans_owned<TRange>(self) -> SortedRangesOwnedSpanIter<T, TRange>
    where
        NonZeroRange<TRange>: CreateRange<Item = TRange>,
        T: UncheckedCast<TRange>,
        TRange: Default + Copy + SignedNonZeroable + Add<Output = TRange>,
    {
        SortedRangesSpanIter::new(self.iter_roi_owned::<NonZeroRange<TRange>>())
    }

    /// Like [`SortedRanges::spans_owned`], but verifies upfront that all
    /// reconstructed coordinates are representable in `TRange`.
    ///
    /// [`SortedRanges::spans_owned`] and [`spans`](SortedRanges::spans)
    /// will eventually drop support for a generic parameter and just return
    /// Span<T>. This method instead validates, via the [`ImageDimension`]
    /// bounds, that every value produced while iterating fits `TRange`.
    ///
    /// This allows e.g. producing `Span<u16>` from a `SortedRanges<u32>`, as
    /// long as its bounds are small enough.
    ///
    /// # Errors
    /// Returns an [`IncompatibleSizeError`] if
    /// `bounds.x + bounds.width` > TRange::MAX or
    /// `bounds.y + bounds.height` > TRange::MAX
    pub fn try_into_spans<TRange>(
        self,
    ) -> Result<SortedRangesOwnedSpanIter<T, TRange>, IncompatibleSizeError>
    where
        NonZeroRange<TRange>: CreateRange<Item = TRange>,
        T: UncheckedCast<TRange> + UncheckedCast<u64>,
        TRange: Default + Copy + SignedNonZeroable + Add<Output = TRange> + TryFrom<u64>,
        IncompatibleSizeError: From<TRange::Error>,
    {
        let width = u64::from(self.bounds.width.get());
        let x_end = u64::from(self.bounds.x) + width;
        let y_end = u64::from(self.bounds.y) + u64::from(self.bounds.height.get());
        // Final value of the flattened position accumulator; the row-cut
        // position can exceed it by up to one row width.
        let flat_end = self
            .included
            .iter()
            .chain(&self.excluded)
            .map(|&len| UncheckedCast::<u64>::cast_unchecked(len))
            .sum::<u64>()
            + width;
        for value in [flat_end, x_end, y_end] {
            TRange::try_from(value)?;
        }
        Ok(self.spans_owned::<TRange>())
    }

    pub fn iter_global_with<TRange: CreateRange>(
        &self,
        width: NonZeroU32,
    ) -> SortedRangesIterGlobal<
        std::iter::Copied<std::slice::Iter<'_, T>>,
        std::iter::Copied<std::slice::Iter<'_, T>>,
        TRange,
    >
    where
        T: UncheckedCast<TRange::Item>,
        TRange::Item: Default
            + Copy
            + SignedNonZeroable
            + Add<Output = TRange::Item>
            + Sub<Output = TRange::Item>
            + Mul<Output = TRange::Item>
            + Div<Output = TRange::Item>
            + Rem<Output = TRange::Item>
            + Ord,
        u32: UncheckedCast<TRange::Item>,
    {
        SortedRangesIterGlobal::new(
            self.included.iter().copied(),
            self.excluded.iter().copied(),
            self.bounds.width,
            width,
            NonZeroU32::new(self.bounds.height.get() + self.bounds.y).unwrap(),
            self.bounds.x.cast_unchecked(),
            self.bounds.y.cast_unchecked(),
        )
    }
    pub fn iter_global_owned_with<TRange: CreateRange>(
        self,
        width: NonZeroU32,
    ) -> SortedRangesIterGlobal<std::vec::IntoIter<T>, std::vec::IntoIter<T>, TRange>
    where
        T: UncheckedCast<TRange::Item>,
        TRange::Item: Default
            + Copy
            + SignedNonZeroable
            + Add<Output = TRange::Item>
            + Sub<Output = TRange::Item>
            + Mul<Output = TRange::Item>
            + Div<Output = TRange::Item>
            + Rem<Output = TRange::Item>
            + Ord,
        u32: UncheckedCast<TRange::Item>,
    {
        SortedRangesIterGlobal::new(
            self.included.into_iter(),
            self.excluded.into_iter(),
            self.bounds.width,
            width,
            NonZeroU32::new(self.bounds.height.get() + self.bounds.y).unwrap(),
            self.bounds.x.cast_unchecked(),
            self.bounds.y.cast_unchecked(),
        )
    }
}

impl<T> ImageDimension for SortedRanges<T> {
    fn bounds(&self) -> Rect<u32> {
        self.bounds
    }
    fn width(&self) -> NonZero<u32> {
        self.bounds.width
    }
}

/// Iterate over the [`Span`]s of a [`SortedRanges`] by value, equivalent to
/// [`SortedRanges::spans_owned::<T>`](SortedRanges::spans_owned).
///
/// This makes [`SortedRanges`] usable everywhere an
/// `IntoIterator<Item = Span<T>>` is accepted, e.g. as an item of
/// [`ImaskSet::union_all`](crate::ImaskSet::union_all) or of the outer
/// iterator of [`UnionAll::new`](crate::span::UnionAll::new).
impl<T> std::iter::IntoIterator for SortedRanges<T>
where
    T: Ord
        + Copy
        + Debug
        + Default
        + Add<Output = T>
        + Sub<Output = T>
        + Mul<Output = T>
        + Div<Output = T>
        + Rem<Output = T>
        + SignedNonZeroable
        + UncheckedCast<T>,
    u32: UncheckedCast<T>,
{
    type Item = Span<T>;
    type IntoIter = SortedRangesOwnedSpanIter<T, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.spans_owned::<T>()
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
        let input = SortedRanges::<u32>::try_from_ordered_iter(
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

    #[test]
    fn into_iter_matches_spans_owned() -> TestResult {
        let input = SortedRanges::<u32>::try_from_ordered_iter(
            [0..1000u32, 1001..2000].with_roi(TEST_BOUNDS),
        )?;
        assert_eq!(
            input.clone().spans_owned::<u32>().collect::<Vec<_>>(),
            input.into_iter().collect::<Vec<_>>()
        );
        Ok(())
    }

    #[test]
    fn ranges_from_span_roundtrip() {
        let x = NonZeroRange::from_span(15u32, NonZero::new(10).unwrap());
        let span = Span { y: 10u32, x };
        let r = SortedRanges::from(span);
        let mut spans = r.spans::<u32>();
        let first = spans.next().expect("Has one");
        assert_eq!(None, spans.next(), "First {first:?}");
        assert_eq!(span, first);
    }

    #[cfg(feature = "range-set-blaze-0_5")]
    #[test]
    fn combine_inline() {
        let a =
            SortedRanges::<u8>::try_from_ordered_iter([10u32..20, 30..40].with_roi(TEST_BOUNDS))
                .unwrap();
        let b =
            SortedRanges::<u8>::try_from_ordered_iter([20u32..30, 41..45].with_roi(TEST_BOUNDS))
                .unwrap();

        let b_iter = b.iter_roi::<RangeInclusive<u64>>();
        let a = a
            .map_inplace(|a_iter| {
                let bounds = a_iter.bounds();
                range_set_blaze_0_5::SortedDisjoint::union(b_iter, a_iter).with_roi(bounds)
            })
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
            SortedRanges::<u32>::try_from_ordered_iter([0u64..1, 5u64..6].with_roi(TEST_BOUNDS));

        let map = map.unwrap();
        let collected: Vec<_> = map.iter_roi::<std::ops::Range<u64>>().collect();
        assert_eq!(vec![0u64..1, 5u64..6], collected);
    }

    #[test]
    fn split_when_collection_becomes_bigger() {
        let a =
            SortedRanges::<u8>::try_from_ordered_iter([10u32..15, 30..35].with_roi(TEST_BOUNDS))
                .unwrap();

        let a = a
            .map_inplace(|iter| {
                let bounds = iter.bounds();
                iter.flat_map(|x| {
                    let with_offset = (*x.start() + 10)..=(*x.end() + 10);
                    [x, with_offset]
                })
                .with_roi(bounds)
            })
            .unwrap();

        assert_eq!(
            vec![10u64..15, 20..25, 30..35, 40..45],
            a.iter_roi_owned().collect::<Vec<_>>()
        );
    }

    #[test]
    fn split_returns_none_when_empty() {
        let a = SortedRanges::<u8>::try_from_ordered_iter(
            std::iter::once(10u32..15).with_roi(TEST_BOUNDS),
        )
        .unwrap();

        let result =
            a.map_inplace(|_| std::iter::empty().with_bounds(NonZeroU32::MIN, NonZeroU32::MIN));

        assert!(result.is_none());
    }

    #[test]
    fn range_with_initial_offset() {
        let encoded =
            SortedRanges::<u8>::try_from_ordered_iter([10u32..20, 255..257].with_roi(TEST_BOUNDS))
                .unwrap();
        assert_eq!(
            vec![10u64..=19, 255u64..=256],
            encoded.iter_roi_owned().collect::<Vec<_>>()
        );
    }

    #[test]
    fn owned_iterator() {
        let encoded =
            SortedRanges::<u8>::try_from_ordered_iter([10u32..20, 255..257].with_roi(TEST_BOUNDS))
                .unwrap();
        let collected: Vec<_> = encoded.iter_roi_owned().collect();
        assert_eq!(2, collected.len());
        assert_eq!(10u64..=19, collected[0]);
        assert_eq!(255u64..=256, collected[1]);
    }
    #[test]
    fn assert_big_gap_causes_error() {
        let error =
            SortedRanges::<u8>::try_from_ordered_iter([10u32..20, 276..280].with_roi(TEST_BOUNDS))
                .unwrap_err();
        assert!(error.to_string().contains("out of range"), "{error}");
    }

    #[test]
    fn assert_big_ranges_cause_error() {
        let error = SortedRanges::<u8>::try_from_ordered_iter(
            core::iter::once(10u32..280).with_roi(TEST_BOUNDS),
        )
        .unwrap_err();
        assert!(error.to_string().contains("out of range"), "{error}");
    }
    #[test]
    fn zero_ranges_cause_error() {
        let error = SortedRanges::<u8>::try_from_ordered_iter(
            core::iter::once(10u32..10).with_roi(TEST_BOUNDS),
        )
        .unwrap_err();
        assert!(error.to_string().contains("must be >"), "{error}");
    }

    #[test]
    fn overlapping_cause_error() {
        let error =
            SortedRanges::<u8>::try_from_ordered_iter([10u32..12, 11..12].with_roi(TEST_BOUNDS))
                .unwrap_err();
        assert!(error.to_string().contains("must be >"), "{error}");
    }

    #[test]
    fn iterate_with_different_output_types() {
        let encoded =
            SortedRanges::<u8>::try_from_ordered_iter([10u32..15, 30..35].with_roi(TEST_BOUNDS))
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
        let ranges = SortedRanges::<u16>::try_from_ordered_iter(
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
        let ranges = SortedRanges::<u16>::try_from_ordered_iter(
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
        let ranges = SortedRanges::<u16>::try_from_ordered_iter(
            [0u32..1, 3..4, 8..11, 13..14, 19..21].with_bounds(SIZE, SIZE),
        )
        .unwrap();

        let with_smaller: Vec<_> = ranges
            .iter_global_with::<Range<u32>>(NonZero::new(10u32).unwrap())
            .collect();
        assert_eq!(with_smaller, vec![0u32..1, 3..4, 8..11]);
    }

    #[test]
    fn try_from_span_iter_roundtrip() -> TestResult {
        let original = SortedRanges::<u32>::try_from_ordered_iter(
            [0u32..1000, 1001..2000].with_roi(TEST_BOUNDS),
        )?;
        let spans: Vec<_> = original.spans::<u32>().collect();

        let reconstructed = SortedRanges::<u32>::try_from_span_iter(
            spans.with_bounds(TEST_BOUNDS.width, TEST_BOUNDS.height),
        )?;

        assert_eq!(
            original.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
            reconstructed.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
        );
        Ok(())
    }

    #[test]
    fn try_from_span_iter_empty_returns_empty_error() {
        let spans: Vec<Span<u32>> = vec![];
        let result = SortedRanges::<u32>::try_from_span_iter(
            spans.with_bounds(TEST_BOUNDS.width, TEST_BOUNDS.height),
        );
        assert!(matches!(result, Err(PipelineError::Empty)));
    }

    #[test]
    fn try_from_span_iter_overlapping_panics() {
        let spans = vec![Span::new(0u32..500, 0u32), Span::new(0u32..500, 0u32)];
        let result = SortedRanges::<u64>::try_from_span_iter(
            spans.with_bounds(TEST_BOUNDS.width, TEST_BOUNDS.height),
        );
        assert_eq!(
            result.unwrap_err(),
            PipelineError::from(IntErrorKind::NegOverflow)
        );
    }

    #[test]
    fn try_from_span_iter_preserves_bounds_offset() -> TestResult {
        let bounds_with_offset = Rect::new(
            1,
            1,
            NonZero::new(4u32).unwrap(),
            NonZero::new(4u32).unwrap(),
        );
        let spans = vec![Span::new(1u32..2, 1u32), Span::new(1u32..2, 2u32)];

        let reconstructed =
            SortedRanges::<u32>::try_from_span_iter(spans.clone().with_roi(bounds_with_offset))?;

        assert_eq!(bounds_with_offset, ImageDimension::bounds(&reconstructed));
        assert_eq!(spans, reconstructed.spans().collect::<Vec<_>>());
        Ok(())
    }

    #[test]
    fn span_roundtrip_with_offset_produces_global_spans() {
        let roi = Rect::new(
            1u32,
            2,
            NonZero::new(200).unwrap(),
            NonZero::new(100).unwrap(),
        );

        let global_spans = vec![
            Span::new(1u32..11, 2u32),
            Span::new(1u32..11, 3u32),
            Span::new(1u32..11, 4u32),
        ];

        let sorted =
            SortedRanges::<u32>::try_from_span_iter(global_spans.clone().with_roi(roi)).unwrap();

        assert_eq!(ImageDimension::bounds(&sorted), roi);

        let result_spans: Vec<Span<u32>> = sorted.spans().collect();
        assert_eq!(result_spans, global_spans);

        let local_ranges: Vec<Range<u64>> = sorted.iter_roi().collect();
        assert_eq!(
            local_ranges,
            vec![0u64..10, 200..210, 400..410],
            "iter_roi must produce LOCAL ranges (row 0, 1, 2 of the ROI), \
             not positions computed from global span y values"
        );
    }

    #[test]
    fn iter_roi_is_local_but_spans_are_global_with_offset() {
        let roi = Rect::new(
            5u32,
            7,
            NonZero::new(50).unwrap(),
            NonZero::new(30).unwrap(),
        );
        let sorted =
            SortedRanges::<u32>::try_from_ordered_iter(vec![0u64..10, 60..70].with_roi(roi))
                .unwrap();

        let iter = sorted.iter_roi::<Range<u64>>();
        assert_eq!(
            ImageDimension::bounds(&iter),
            Rect::new(5, 7, roi.width, roi.height),
            "iter_roi is a LOCAL iterator — its ImageDimension must report offset 0"
        );

        let span_iter = sorted.spans::<u32>();
        assert_eq!(
            ImageDimension::bounds(&span_iter),
            roi,
            "spans() produces GLOBAL spans — its ImageDimension must report the ROI offset, {:?}",
            span_iter.clone().collect::<Vec<_>>()
        );
    }

    #[test]
    fn try_from_span_iter_u16_max_width_two_rows() -> TestResult {
        const WIDTH: NonZeroU32 = NonZero::new(u16::MAX as u32).unwrap();
        const HEIGHT: NonZeroU32 = NonZero::new(2u32).unwrap();
        let spans = vec![
            Span::new(0u16..u16::MAX, 0u16),
            Span::new(0u16..u16::MAX, 1u16),
        ];

        let result = SortedRanges::<u64>::try_from_span_iter(spans.with_bounds(WIDTH, HEIGHT))?;

        let ranges: Vec<Range<u64>> = result.iter_roi().collect();
        assert_eq!(vec![0u64..131070], ranges);
        Ok(())
    }

    #[test]
    fn from_span_iter_minbounds_height_too_big() -> TestResult {
        // Declared height (10) is much bigger than needed (2); x, y and width match.
        let declared = Rect::new(
            0u32,
            0,
            NonZero::new(10u32).unwrap(),
            NonZero::new(10u32).unwrap(),
        );
        let spans = vec![Span::new(0u32..10, 0u32), Span::new(0u32..10, 1u32)];

        let result =
            SortedRanges::<u32>::try_from_span_iter_minbounds(spans.clone().with_roi(declared))?;

        let expected_bounds = Rect::new(
            0u32,
            0,
            NonZero::new(10u32).unwrap(),
            NonZero::new(2u32).unwrap(),
        );
        assert_eq!(expected_bounds, ImageDimension::bounds(&result));
        assert_eq!(spans, result.spans().collect::<Vec<_>>());

        // Same flat layout as a direct collect with tight bounds.
        let direct = SortedRanges::<u32>::try_from_span_iter(spans.with_roi(expected_bounds))?;
        assert_eq!(
            direct.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
            result.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
        );
        Ok(())
    }

    #[test]
    fn from_span_iter_minbounds_y_offset() -> TestResult {
        // Declared y (0) is smaller than actual min y (2); x/width match so only
        // the absolute start (offset * width) has to be shifted.
        let declared = Rect::new(
            0u32,
            0,
            NonZero::new(10u32).unwrap(),
            NonZero::new(10u32).unwrap(),
        );
        let spans = vec![Span::new(0u32..10, 2u32), Span::new(0u32..10, 3u32)];

        let result =
            SortedRanges::<u32>::try_from_span_iter_minbounds(spans.clone().with_roi(declared))?;

        let expected_bounds = Rect::new(
            0u32,
            2,
            NonZero::new(10u32).unwrap(),
            NonZero::new(2u32).unwrap(),
        );
        assert_eq!(expected_bounds, ImageDimension::bounds(&result));
        assert_eq!(spans, result.spans().collect::<Vec<_>>());

        let direct = SortedRanges::<u32>::try_from_span_iter(spans.with_roi(expected_bounds))?;
        assert_eq!(
            direct.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
            result.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
        );
        Ok(())
    }

    #[test]
    fn from_span_iter_minbounds_x_mismatch() -> TestResult {
        // Declared x/width (0/10) don't match actual (2/3): row stride changes,
        // so a full re-encode via into_spans().with_roi() is required.
        let declared = Rect::new(
            0u32,
            0,
            NonZero::new(10u32).unwrap(),
            NonZero::new(10u32).unwrap(),
        );
        let spans = vec![Span::new(2u32..5, 1u32), Span::new(2u32..5, 2u32)];

        let result =
            SortedRanges::<u32>::try_from_span_iter_minbounds(spans.clone().with_roi(declared))?;

        let expected_bounds = Rect::new(
            2u32,
            1,
            NonZero::new(3u32).unwrap(),
            NonZero::new(2u32).unwrap(),
        );
        assert_eq!(expected_bounds, ImageDimension::bounds(&result));
        assert_eq!(spans, result.spans().collect::<Vec<_>>());

        let direct = SortedRanges::<u32>::try_from_span_iter(spans.with_roi(expected_bounds))?;
        assert_eq!(
            direct.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
            result.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
        );
        Ok(())
    }
}
