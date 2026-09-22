use std::{
    cmp::Ord,
    fmt::{Debug, Display},
    io,
    num::{NonZero, NonZeroU32},
    ops::{Add, Div, Mul, Rem, Sub},
};

use crate::visualize_iter::IterVisualizer;
use crate::{
    CreateRange, ImageDimension, IncompatibleSizeError, IntoPipelineOutput, MaybeResult,
    NonZeroRange, PipelineEmptyError, PipelineError, Roi, SignedNonZeroable, SortedRangesSpanIter,
    Span, UncheckedCast, WithBounds, WithRoi,
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
mod inspect_spans;
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
pub use inspect_spans::*;
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
    /// Behaves exactly like [`std::iter::Inspect`], but forwards [`ImageDimension`]
    /// to the wrapped iterator.
    fn inspect_spans<F>(self, f: F) -> InspectSpans<Self::IntoIter, F>
    where
        F: FnMut(&Self::Item),
    {
        InspectSpans::new(self.into_iter(), f)
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
    /// use imask::{Roi, ImaskSet, ImageDimension};
    ///
    /// const SIZE: NonZeroU32 = NonZeroU32::new(10).unwrap();
    /// let spans = Roi::new(10u32..20, 20..30).into_spans();
    /// let mut count = 0;
    /// let mut inspect = spans.clone().fold_inline(0, |a, _r| {
    ///     *a += 1;
    /// });
    /// assert_eq!(spans.roi(), inspect.roi());
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
    ) -> Result<crate::span::Intersect<Self::IntoIter, TOther::IntoIter>, PipelineEmptyError>
    where
        Self::IntoIter: ImageDimension,
        TOther::IntoIter: ImageDimension,
    {
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

    fn clip<T>(
        self,
        roi: impl Into<Roi<u32>>,
    ) -> Result<ClipSpanIter<Self::IntoIter, T>, PipelineError>
    where
        Self::IntoIter: Iterator<Item = Span<T>> + ImageDimension,
        T: SignedNonZeroable
            + TryFrom<u32, Error: Into<PipelineError>>
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
        TOut::Item: TryFrom<u32, Error: Debug> + Ord + Debug,
    {
        crate::span::SpanIntoRangesIter::new(self.into_iter())
    }

    fn sanitize_sorted_disjoint(self) -> SanitizeSortedDisjoint<Self::IntoIter>
    where
        Self::Item: CreateRange<Item: Debug>,
    {
        SanitizeSortedDisjoint::new(self)
    }

    fn with_roi(self, roi: impl Into<Roi<u32>>) -> WithRoi<Self::IntoIter> {
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
        let roi = iter.roi().expand_saturating(radius);
        crate::span::DilateSpanIterAcc::new(iter.with_roi(roi), offset)
    }

    /// Dilates by `offset`, declaring `roi` as region of interest of the input.
    ///
    /// The effective region of interest is the intersection of `roi` with the bounds the
    /// input declares ([`ImageDimension::roi`]); if they don't overlap,
    /// [`PipelineError::Empty`] is returned. Input spans (partially) outside that region
    /// are clipped — spans entirely outside dilate to nothing.
    fn dilate_within<T>(
        self,
        offset: <T as SignedNonZeroable>::NonZero,
        roi: impl Into<Roi<u32>>,
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
            .roi()
            .intersection(&roi.into())
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
    bounds: Roi<u32>,
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
    fn build(self, bounds: Roi<u32>) -> SortedRanges<T> {
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

use crate::span::builder::roi_hint::SortedRangesSpanBuilderInternal;
use crate::span::builder::tight::SortedRangesTightSpanBuilderInternal;

impl<T> From<Span<T::Sqrt>> for SortedRanges<T>
where
    T: crate::number::Sqrtable,
    T::Sqrt: SignedNonZeroable + Copy + Into<u32> + Sub<Output = T::Sqrt>,
{
    fn from(span: Span<T::Sqrt>) -> Self {
        let bounds = Roi {
            x: NonZeroRange::new_unchecked(span.x.start.into()..span.x.end.into()),
            y: NonZeroRange::new_unchecked(span.y.into()..span.y.into() + 1),
        };
        #[allow(clippy::eq_op, reason = "Avoid additional bound on num_traits::Zero")]
        let zero = span.x.start - span.x.start;
        Self {
            included: vec![T::from(span.x.len())],
            excluded: vec![T::from(zero)],
            bounds,
        }
    }
}

impl<T> SortedRanges<T> {
    /// Crate-internal constructor from prebuilt delta-encoded parts.
    /// Counterpart of [`Self::into_raw_parts`]; exclusively for the span
    /// builders in `span::builder`.
    pub(crate) fn new_internal(included: Vec<T>, excluded: Vec<T>, bounds: Roi<u32>) -> Self {
        Self {
            included,
            excluded,
            bounds,
        }
    }

    /// Destructures into the raw delta-encoded parts
    /// `(included, excluded, bounds)`. Counterpart of [`Self::new_internal`];
    /// exclusively for the span builders in `span::builder`.
    pub(crate) fn into_raw_parts(self) -> (Vec<T>, Vec<T>, Roi<u32>) {
        (self.included, self.excluded, self.bounds)
    }

    #[deprecated = "Use from_span instead, which automatically sets the correct bounds"]
    pub fn new<TRange>(r: NonZeroRange<TRange>, bounds: impl Into<Roi<u32>>) -> Self
    where
        TRange: UncheckedCast<T> + Sub<Output = TRange>,
        T: TryFrom<u64>,
    {
        let bounds = bounds.into();
        assert!(bounds.x.start == 0);
        assert!(bounds.y.start == 0);
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
        let bounds = iter.roi();
        Self::try_from_ordered_iter_roi_internal(iter).map(|r| r.build(bounds))
    }

    #[deprecated = "Use `try_from_ordered_iter(input.with_roi(bounds))` instead"]
    pub fn try_from_ordered_iter_roi<TIter>(
        iter: TIter,
        bounds: impl Into<Roi<u32>>,
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
        let bounds = iter.roi();
        debug_assert_eq!(
            iter.width(),
            bounds.width(),
            "width() must equal roi().width()"
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
    /// The first pass builds with `input.roi()` while tracking
    /// `min_x`/`max_x_end`/`min_y`/`max_y` (the same job
    /// [`BoundsInspector`](crate::BoundsInspector) does for flat ranges,
    /// but directly on spans so no extra pass is needed).
    ///
    /// Afterwards:
    /// - if the tracked min-bounds equal `input.roi()`, the first result is returned as-is.
    /// - if only `y_end` is too big (same `x`/`y` range and width), only the roi is adapted.
    /// - if `x` matches but the `y`-offset is off, the absolute start
    ///   (`excluded[0]`) is shifted by `offset * width` and the roi is adapted.
    /// - otherwise (`x`-bounds don't match, hence the row stride changes),
    ///   the spans are re-encoded in-place via [`SortedRanges::map_span_inplace`],
    ///   reusing the existing `included`/`excluded` buffers.
    pub fn try_from_span_iter_minbounds<TIter, TSpan>(iter: TIter) -> Result<Self, PipelineError>
    where
        TIter: IntoIterator<Item = Span<TSpan>, IntoIter: ImageDimension>,
        TSpan: Copy + TryInto<u64>,
        T: TryFrom<u64, Error: Display> + UncheckedCast<u64> + Copy,
        IncompatibleSizeError: From<TSpan::Error>,
        IncompatibleSizeError: From<T::Error>,
    {
        let mut iter = iter.into_iter();
        let declared = iter.roi();
        debug_assert_eq!(
            iter.width(),
            declared.width(),
            "width() must equal roi().width()"
        );
        let (min, max) = iter.size_hint();
        let size_hint = max.unwrap_or(min);
        let mut builder = SortedRangesTightSpanBuilderInternal::<T>::new(declared, size_hint);
        for span in &mut iter {
            builder.add(span)?;
        }
        builder.build()
    }

    #[cfg(feature = "async-io")]
    pub(crate) fn from_parts(included: Vec<T>, excluded: Vec<T>, bounds: Roi<u32>) -> Self {
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
        let width = u64::from(self.bounds.width().get());
        let x_end = u64::from(self.bounds.x.end);
        let y_end = u64::from(self.bounds.y.end);
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
            self.bounds.width(),
            width,
            NonZeroU32::new(self.bounds.y.end).unwrap(),
            self.bounds.x.start.cast_unchecked(),
            self.bounds.y.start.cast_unchecked(),
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
            self.bounds.width(),
            width,
            NonZeroU32::new(self.bounds.y.end).unwrap(),
            self.bounds.x.start.cast_unchecked(),
            self.bounds.y.start.cast_unchecked(),
        )
    }

    /// Returns `true`, if the point (`x`, `y`) is part of any included range.
    ///
    /// Heuristic: The bounds are checked first (O(1)) and the ranges are only
    /// searched (O(n)), if the point lies within [`SortedRanges::bounds`].
    pub fn contains<TP: TryInto<u32>>(&self, x: TP, y: TP) -> bool
    where
        T: Into<u64> + Copy,
    {
        let Ok(x_u32) = x.try_into() else {
            return false;
        };
        let Ok(y_u32) = y.try_into() else {
            return false;
        };
        if !self.bounds.contains(&x_u32, &y_u32) {
            return false;
        }
        let flat = u64::from(y_u32 - self.bounds.y.start) * u64::from(self.bounds.width().get())
            + u64::from(x_u32 - self.bounds.x.start);
        let mut start: u64 = 0;
        for (&gap, &len) in self.excluded.iter().zip(&self.included) {
            start += gap.into();
            let end = start + len.into();
            if flat < end {
                return flat >= start;
            }
            start = end;
        }
        false
    }
}

impl<T> ImageDimension for SortedRanges<T> {
    fn roi(&self) -> Roi<u32> {
        self.bounds
    }
    fn width(&self) -> NonZero<u32> {
        self.bounds.width()
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

    use super::*;
    use crate::{NonZeroRange, Roi};
    use std::num::IntErrorKind;

    const TEST_BOUNDS: Roi<u32> = Roi {
        x: NonZeroRange::<u32>::new_const(0..1000),
        y: NonZeroRange::<u32>::new_const(0..1000),
    };

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
        let r = SortedRanges::<u64>::from(span);
        let mut spans = r.spans::<u32>();
        let first = spans.next().expect("Has one");
        assert_eq!(None, spans.next(), "First {first:?}");
        assert_eq!(span, first);
    }

    #[test]
    fn from_span_infers_span_type_from_destination() {
        // No integer suffixes on the Span: `T::Sqrt` is inferred from the
        // `SortedRanges<T>` turbofish, which the old `Squareable` direction
        // could not do.
        let r = SortedRanges::<u16>::from(Span::new(0..5, 0));
        assert_eq!(Span::new(0..5, 0), r.spans::<u8>().next().unwrap());
        let r = SortedRanges::<u64>::from(Span::new(0..10, 0));
        assert_eq!(Span::new(0..10, 0), r.spans::<u32>().next().unwrap());
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
                let bounds = a_iter.roi();
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
        let map = SortedRanges::<u32>::try_from_ordered_iter([0u64..1, 5..6].with_roi(TEST_BOUNDS));

        let map = map.unwrap();
        let collected: Vec<_> = map.iter_roi::<std::ops::Range<u64>>().collect();
        assert_eq!(vec![0..1, 5..6], collected);
    }

    #[test]
    fn contains_point() -> TestResult {
        let bounds = Roi::new(0u32..100, 0..100);
        // row 0: x in 5..10, row 2: x in 5..10
        let ranges =
            SortedRanges::<u16>::try_from_ordered_iter([5u32..10, 205..210].with_roi(bounds))?;

        // out_of_bounds: x/y beyond the rect
        assert!(!ranges.contains(100u16, 0u16));
        assert!(!ranges.contains(0u16, 100u16));
        // (T::MAX, T::MAX): out of bounds without overflowing the bounds check
        assert!(!ranges.contains(u16::MAX, u16::MAX));
        // InboundNotContainedX: in bounds, but x lies in a gap of an included row
        assert!(!ranges.contains(15u16, 0u16));
        // InboundNotContainedY: in bounds, but the row contains no ranges at all
        assert!(!ranges.contains(7u16, 1u16));
        // Match: TP-generic call and matches on both rows
        assert!(ranges.contains(7u8, 0u8));
        assert!(ranges.contains(5u16, 2u16));
        assert!(ranges.contains(7u16, 2u16));
        // Match boundaries: start inclusive, end exclusive
        assert!(ranges.contains(5u16, 0u16));
        assert!(!ranges.contains(10u16, 0u16));

        assert!(ranges.contains(5usize, 0usize));
        assert!(!ranges.contains(10usize, 0usize));
        Ok(())
    }

    #[test]
    fn contains_single_span_wider_than_u8() {
        // Regression: SortedRanges::<u32>::from(Span::new(0..257, 0)).contains(0, 0)
        // must be true.
        let ranges = SortedRanges::<u32>::from(Span::new(0..257, 0));
        assert!(ranges.contains(0u8, 0));
        // Last pixel of the span is still included (end exclusive).
        assert!(ranges.contains(256u16, 0));
        // End is exclusive / out of bounds.
        assert!(!ranges.contains(257u16, 0));
        // Height is 1, so any other row is out of bounds.
        assert!(!ranges.contains(0u16, 1));
    }

    #[test]
    fn split_when_collection_becomes_bigger() {
        let a =
            SortedRanges::<u8>::try_from_ordered_iter([10u32..15, 30..35].with_roi(TEST_BOUNDS))
                .unwrap();

        let a = a
            .map_inplace(|iter| {
                let bounds = iter.roi();
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
        let rect = Roi::new(2u32..6, 1..4);
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
        let rect = Roi::new(0u32..10, 1..4);
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
            spans.with_bounds(TEST_BOUNDS.width(), TEST_BOUNDS.height()),
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
            spans.with_bounds(TEST_BOUNDS.width(), TEST_BOUNDS.height()),
        );
        assert!(matches!(result, Err(PipelineError::Empty)));
    }

    #[test]
    fn try_from_span_iter_overlapping_panics() {
        let spans = vec![Span::new(0u32..500, 0), Span::new(0u32..500, 0)];
        let result = SortedRanges::<u64>::try_from_span_iter(
            spans.with_bounds(TEST_BOUNDS.width(), TEST_BOUNDS.height()),
        );
        assert_eq!(
            result.unwrap_err(),
            PipelineError::from(IntErrorKind::NegOverflow)
        );
    }

    #[test]
    fn try_from_span_iter_preserves_bounds_offset() -> TestResult {
        let bounds_with_offset = Roi::new(1u32..5, 1..5);
        let spans = vec![Span::new(1u32..2, 1), Span::new(1u32..2, 2)];

        let reconstructed =
            SortedRanges::<u32>::try_from_span_iter(spans.clone().with_roi(bounds_with_offset))?;

        assert_eq!(bounds_with_offset, ImageDimension::roi(&reconstructed));
        assert_eq!(spans, reconstructed.spans().collect::<Vec<_>>());
        Ok(())
    }

    #[test]
    fn span_roundtrip_with_offset_produces_global_spans() {
        let roi = Roi::new(1u32..201, 2..102);

        let global_spans = vec![
            Span::new(1u32..11, 2),
            Span::new(1u32..11, 3),
            Span::new(1u32..11, 4),
        ];

        let sorted =
            SortedRanges::<u32>::try_from_span_iter(global_spans.clone().with_roi(roi)).unwrap();

        assert_eq!(ImageDimension::roi(&sorted), roi);

        let result_spans: Vec<Span<u32>> = sorted.spans().collect();
        assert_eq!(result_spans, global_spans);

        let local_ranges: Vec<Range<u64>> = sorted.iter_roi().collect();
        assert_eq!(
            local_ranges,
            vec![0..10, 200..210, 400..410],
            "iter_roi must produce LOCAL ranges (row 0, 1, 2 of the ROI), \
             not positions computed from global span y values"
        );
    }

    #[test]
    fn iter_roi_is_local_but_spans_are_global_with_offset() {
        let roi = Roi::new(5u32..55, 7..37);
        let sorted =
            SortedRanges::<u32>::try_from_ordered_iter(vec![0u64..10, 60..70].with_roi(roi))
                .unwrap();

        let iter = sorted.iter_roi::<Range<u64>>();
        assert_eq!(
            ImageDimension::roi(&iter),
            roi,
            "iter_roi is a LOCAL iterator — its ImageDimension must report the declared ROI"
        );

        let span_iter = sorted.spans::<u32>();
        assert_eq!(
            ImageDimension::roi(&span_iter),
            roi,
            "spans() produces GLOBAL spans — its ImageDimension must report the ROI offset, {:?}",
            span_iter.clone().collect::<Vec<_>>()
        );
    }

    #[test]
    fn try_from_span_iter_u16_max_width_two_rows() -> TestResult {
        const WIDTH: NonZeroU32 = NonZero::new(u16::MAX as u32).unwrap();
        const HEIGHT: NonZeroU32 = NonZero::new(2u32).unwrap();
        let spans = vec![Span::new(0u16..u16::MAX, 0), Span::new(0u16..u16::MAX, 1)];

        let result = SortedRanges::<u64>::try_from_span_iter(spans.with_bounds(WIDTH, HEIGHT))?;

        let ranges: Vec<Range<u64>> = result.iter_roi().collect();
        assert_eq!(vec![0..131070], ranges);
        Ok(())
    }

    #[test]
    fn from_span_iter_minbounds_height_too_big() -> TestResult {
        // Declared height (10) is much bigger than needed (2); x, y and width match.
        let declared = Roi::new(0u32..10, 0..10);
        let spans = vec![Span::new(0u32..10, 0), Span::new(0u32..10, 1)];

        let result =
            SortedRanges::<u32>::try_from_span_iter_minbounds(spans.clone().with_roi(declared))?;

        let expected_bounds = Roi::new(0u32..10, 0..2);
        assert_eq!(expected_bounds, ImageDimension::roi(&result));
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
        let declared = Roi::new(0u32..10, 0..10);
        let spans = vec![Span::new(0u32..10, 2), Span::new(0u32..10, 3)];

        let result =
            SortedRanges::<u32>::try_from_span_iter_minbounds(spans.clone().with_roi(declared))?;

        let expected_bounds = Roi::new(0u32..10, 2..4);
        assert_eq!(expected_bounds, ImageDimension::roi(&result));
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
        let declared = Roi::new(0u32..10, 0..10);
        let spans = vec![Span::new(2u32..5, 1), Span::new(2u32..5, 2)];

        let result =
            SortedRanges::<u32>::try_from_span_iter_minbounds(spans.clone().with_roi(declared))?;

        let expected_bounds = Roi::new(2u32..5, 1..3);
        assert_eq!(expected_bounds, ImageDimension::roi(&result));
        assert_eq!(spans, result.spans().collect::<Vec<_>>());

        let direct = SortedRanges::<u32>::try_from_span_iter(spans.with_roi(expected_bounds))?;
        assert_eq!(
            direct.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
            result.iter_roi::<Range<u64>>().collect::<Vec<_>>(),
        );
        Ok(())
    }
}
