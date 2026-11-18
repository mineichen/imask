use std::{
    cmp::Ord,
    fmt::Debug,
    num::NonZeroU32,
    ops::{Add, Sub},
};

use crate::{
    CreateRange, ImageDimension, IncompatibleSizeError, IntoPipelineOutput, MaybeResult,
    NonZeroRange, PipelineEmptyError, PipelineError, Roi, SignedNonZeroable, SortedRangesSpanIter,
    Span, UncheckedCast, WithBounds, WithRoi,
    span::{ClipSpanIter, FoldInlineSpanIter},
};

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
