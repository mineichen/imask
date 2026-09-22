//! Builder that shrinks the [`SortedRanges`] bounds to the added content:
//! the initial bounds are only a starting point, `build` tightens them to
//! the minimal bounds of the spans that were actually added.

use std::fmt::Display;
use std::num::IntErrorKind;

use crate::{
    ImaskSet, IncompatibleSizeError, NonZeroRange, PipelineError, Roi, Span, UncheckedCast,
};

use crate::set::SortedRanges;

use super::roi_hint::SortedRangesSpanBuilderInternal;

/// Like [`SortedRangesSpanBuilderInternal`], but additionally tracks the
/// minimal bounds of the added spans and shrinks [`SortedRanges::bounds`] to
/// them on [`build`](Self::build) (see
/// [`SortedRanges::try_from_span_iter_minbounds`], which is implemented on
/// top of this builder).
pub(crate) struct SortedRangesTightSpanBuilderInternal<T> {
    inner: SortedRangesSpanBuilderInternal<T>,
    min_x: u64,
    max_x_end: u64,
    min_y: u64,
    max_y: u64,
}

impl<T> SortedRangesTightSpanBuilderInternal<T>
where
    T: TryFrom<u64, Error: Display> + UncheckedCast<u64> + Copy,
    IncompatibleSizeError: From<T::Error>,
{
    pub(crate) fn new(bounds: Roi<u32>, size_hint: usize) -> Self {
        Self {
            inner: SortedRangesSpanBuilderInternal::new(bounds, size_hint),
            min_x: u64::MAX,
            max_x_end: u64::MIN,
            min_y: u64::MAX,
            max_y: u64::MIN,
        }
    }

    pub(crate) fn add<TSpan>(&mut self, span: Span<TSpan>) -> Result<(), IncompatibleSizeError>
    where
        TSpan: Copy + TryInto<u64>,
        IncompatibleSizeError: From<TSpan::Error>,
    {
        // Track the minimal bounds (the same job `BoundsInspector` does for
        // flat ranges, but directly on spans so no extra pass is needed).
        let x_start: u64 = span.x.start.try_into()?;
        let x_end: u64 = span.x.end.try_into()?;
        let y: u64 = span.y.try_into()?;
        self.min_x = self.min_x.min(x_start);
        self.max_x_end = self.max_x_end.max(x_end);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
        self.inner.add(span)
    }

    pub(crate) fn build(self) -> Result<SortedRanges<T>, PipelineError> {
        let declared = self.inner.declared_bounds();
        let (included, mut excluded, bounds) = self.inner.build()?.into_raw_parts();

        // Fully qualified: `IncompatibleSizeError::from` alone is ambiguous
        // here between the concrete `From<TryFromIntError>` impl and the
        // generic `From<T::Error>` bound.
        let cvt = <IncompatibleSizeError as From<std::num::TryFromIntError>>::from;
        let min_x_32 = u32::try_from(self.min_x).map_err(cvt)?;
        let max_x_end_32 = u32::try_from(self.max_x_end).map_err(cvt)?;
        let min_y_32 = u32::try_from(self.min_y).map_err(cvt)?;
        let max_y_32 = u32::try_from(self.max_y).map_err(cvt)?;
        let tight = Roi {
            x: NonZeroRange::new_unchecked(min_x_32..max_x_end_32),
            y: NonZeroRange::new_unchecked(min_y_32..max_y_32 + 1),
        };

        if tight == declared {
            return Ok(SortedRanges::new_internal(included, excluded, tight));
        }

        if tight.x == declared.x {
            if tight.y == declared.y {
                // Only trailing empty rows: flat layout unchanged, shrink height.
                return Ok(SortedRanges::new_internal(included, excluded, tight));
            }
            // Same x/width, y-offset off: flat positions shift uniformly by
            // delta = (tight.y - declared.y) * width. Only the absolute start
            // (excluded[0]) stores an absolute position, the rest are deltas,
            // so a single adjustment suffices
            // (conceptually `buffer.for_each_mut(|v| *v += offset * width)`
            // on absolute positions).
            let declared_y_u64 = u64::from(declared.y.start);
            let tight_y_u64 = u64::from(tight.y.start);
            let width_u64 = u64::from(declared.width().get());
            assert!(tight_y_u64 >= declared_y_u64);
            let delta = (tight_y_u64 - declared_y_u64) * width_u64;
            let start_u64 = excluded[0].cast_unchecked();
            let adjusted = start_u64
                .checked_sub(delta)
                .ok_or(IntErrorKind::NegOverflow)?;
            excluded[0] = T::try_from(adjusted).map_err(IncompatibleSizeError::from)?;
            return Ok(SortedRanges::new_internal(included, excluded, tight));
        }

        // x-bounds don't match (or y moved outside): row stride changed,
        // re-encode the spans in-place, reusing the existing buffers.
        SortedRanges::new_internal(included, excluded, bounds)
            .map_span_inplace(|source| source.with_roi(tight))
            .ok_or(PipelineError::Empty)
    }
}

/// Builds a [`SortedRanges`](crate::set::SortedRanges) with tight bounds from spans where
/// [`SortedRangesTightSpanBuilder::add`] is infallible.
///
/// This makes it suitable for use with
/// [`ImaskSet::fold_inline`](crate::ImaskSet::fold_inline): the first error is captured
/// internally and only surfaced by [`SortedRangesTightSpanBuilder::build`]. Unlike
/// [`SortedRangesSpanBuilder`](super::roi_hint::SortedRangesSpanBuilder), the bounds are
/// shrunk to the added content (see
/// [`SortedRanges::try_from_span_iter_minbounds`](crate::set::SortedRanges::try_from_span_iter_minbounds)).
///
/// ```
/// use imask::{ImageDimension, ImaskSet, Roi, SortedRanges, SortedRangesTightSpanBuilder};
///
/// let roi = Roi::new(10u32..20, 10..20);
/// let clipped = roi.into_spans().clip(Roi::new(12u32..15, 12..14)).unwrap();
/// let mut builder = SortedRangesTightSpanBuilder::<u32>::new(clipped.roi(), &clipped);
/// let mut iter = clipped.fold_inline(builder, |b, s| b.add(*s));
/// let ranges = iter.finish_all().build().unwrap();
/// assert_eq!(ranges.roi(), Roi::new(12u32..15, 12..14));
/// assert_eq!(
///     SortedRanges::try_from_span_iter_minbounds(
///         roi.into_spans().clip(Roi::new(12u32..15, 12..14)).unwrap()
///     )
///     .unwrap(),
///     ranges
/// );
/// ```
pub struct SortedRangesTightSpanBuilder<T> {
    builder: SortedRangesTightSpanBuilderInternal<T>,
    error: Option<IncompatibleSizeError>,
}

impl<T> SortedRangesTightSpanBuilder<T>
where
    T: TryFrom<u64, Error: Display> + UncheckedCast<u64> + Copy,
    IncompatibleSizeError: From<T::Error>,
{
    pub fn new<S>(bounds: impl Into<Roi<u32>>, spans: &dyn Iterator<Item = Span<S>>) -> Self {
        let (min, max) = spans.size_hint();
        let size_hint = max.unwrap_or(min);
        Self {
            builder: SortedRangesTightSpanBuilderInternal::new(bounds.into(), size_hint),
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
