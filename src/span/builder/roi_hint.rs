//! Builder whose bounds act as a *hint*: the resulting
//! [`SortedRanges`](crate::set::SortedRanges) keeps the bounds the builder
//! was created with, whether or not the added spans actually reach them.

use std::num::IntErrorKind;

use crate::{IncompatibleSizeError, PipelineError, Roi, Span};

use crate::set::SortedRanges;

/// Builds a [`SortedRanges`] from spans, merging touching spans.
///
/// (`new` takes no span) and `add` allows `start > merge_end || excluded.is_empty()`:
/// the very first span initializes the merge window, equal starts/ends merge touching
/// spans, and only `start < merge_end` (overlap) is an error.
pub(crate) struct SortedRangesSpanBuilderInternal<T> {
    width_u64: u64,
    offset_x_u64: u64,
    offset_y_u64: u64,
    merge_start: u64,
    merge_end: u64,
    bounds: Roi<u32>,
    excluded: Vec<T>,
    included: Vec<T>,
}

impl<T> SortedRangesSpanBuilderInternal<T>
where
    T: TryFrom<u64, Error: Into<IncompatibleSizeError>>,
    IncompatibleSizeError: From<T::Error>,
{
    pub(crate) fn new(bounds: Roi<u32>, size_hint: usize) -> Self {
        Self {
            width_u64: bounds.width().get() as u64,
            offset_x_u64: bounds.x.start as u64,
            offset_y_u64: bounds.y.start as u64,
            merge_start: 0,
            merge_end: 0,
            bounds,
            excluded: Vec::with_capacity(size_hint),
            included: Vec::with_capacity(size_hint),
        }
    }

    #[inline]
    pub(crate) fn declared_bounds(&self) -> Roi<u32> {
        self.bounds
    }

    pub(crate) fn add<TSpan>(&mut self, span: Span<TSpan>) -> Result<(), IncompatibleSizeError>
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

    pub(crate) fn build(self) -> Result<SortedRanges<T>, PipelineError> {
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
        Ok(SortedRanges::new_internal(included, excluded, bounds))
    }
}

/// Builds a [`SortedRanges`] from spans where [`SortedRangesSpanBuilder::add`] is infallible.
///
/// This makes it suitable for use with
/// [`ImaskSet::fold_inline`](crate::ImaskSet::fold_inline): the first error is captured
/// internally and only surfaced by [`SortedRangesSpanBuilder::build`].
///
/// ```
/// use std::num::NonZeroU32;
/// use imask::{ImaskSet, Roi, SortedRanges, SortedRangesSpanBuilder, Span};
///
/// const SIZE: NonZeroU32 = NonZeroU32::new(10).unwrap();
/// let roi = Roi::new(10u32..20, 10..20);
/// let spans = roi.into_spans();
/// let mut builder = SortedRangesSpanBuilder::<u32>::new(roi, &spans);
/// let mut iter = spans.fold_inline(builder, |b, s| b.add(*s));
/// iter.next();
/// let ranges = iter.finish_all().build().unwrap();
/// assert_eq!(
///     SortedRanges::try_from_span_iter(roi.into_spans()).unwrap(),
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
    pub fn new<S>(bounds: impl Into<Roi<u32>>, spans: &dyn Iterator<Item = Span<S>>) -> Self {
        let (min, max) = spans.size_hint();
        let size_hint = max.unwrap_or(min);
        Self {
            builder: SortedRangesSpanBuilderInternal::new(bounds.into(), size_hint),
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
            Err(error.into())
        } else {
            self.builder.build()
        }
    }
}
