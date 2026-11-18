use crate::{PipelineError, Roi, SortedRanges, UncheckedCast, WithRoi, span::ClipSpanIter};

impl<T> SortedRanges<T> {
    /// Restricts the pixels to `roi`, the bounds become the intersection of
    /// the current bounds and `roi`. The counterpart of
    /// [`ImaskSet::clip`](crate::ImaskSet::clip), which yields the clipped spans.
    ///
    /// - If `roi` contains the bounds already, `self` is returned as is.
    /// - Otherwise the ranges are clipped in place (see
    ///   [`SortedRanges::map_span_inplace`]). Clipping never splits a range,
    ///   so the buffers are reused without growing. Nothing is allocated.
    ///
    /// # Errors
    /// - [`PipelineError::Empty`], if no pixel lies within `roi`
    /// - [`PipelineError::IncompatibleSize`], if `T` cannot hold the area of
    ///   the new bounds: rows becoming full width merge ranges and ranges
    ///   clipped away merge gaps, both up to the area
    pub fn clip_inplace(self, roi: impl Into<Roi<u32>>) -> Result<Self, PipelineError>
    where
        T: TryFrom<u64> + Clone + UncheckedCast<u64>,
        PipelineError: From<<T as TryFrom<u64>>::Error>,
    {
        let bounds = self
            .bounds
            .intersection(&roi.into())
            .ok_or(PipelineError::Empty)?;
        if bounds == self.bounds {
            return Ok(self);
        }
        let area = u64::from(bounds.width().get()) * u64::from(bounds.height().get());
        T::try_from(area)?;
        // Gaps and lengths within `bounds` are up to its area, so they fit `T`:
        // `map_span_inplace` would panic otherwise
        self.map_span_inplace(|spans| {
            let clipped = ClipSpanIter::<_, u64>::new(spans, bounds).ok();
            WithRoi::new(clipped.into_iter().flatten(), bounds)
        })
        .ok_or(PipelineError::Empty)
    }
}

#[cfg(test)]
mod tests {
    use std::{fmt::Display, io};

    use testresult::TestResult;

    use crate::ImaskSet;

    use super::*;

    /// Pixels (4, 2), (6, 2), (4, 3) and (6, 3): within bounds of 3 pixels
    /// width, the range 2..4 continues in the next row
    fn two_columns() -> Result<SortedRanges<u32>, io::Error> {
        SortedRanges::try_from_ordered_iter([0u32..1, 2..4, 5..6].with_roi(Roi::new(4u32..7, 2..4)))
    }

    /// The columns `0..10` of `rows` rows within bounds of 20 pixels width
    fn left_half<T: TryFrom<u64, Error: Display>>(rows: u32) -> Result<SortedRanges<T>, io::Error> {
        SortedRanges::try_from_ordered_iter(
            (0..rows)
                .map(|y| y * 20..y * 20 + 10)
                .with_roi(Roi::new(0u32..20, 0..rows)),
        )
    }

    #[test]
    fn clip_inplace_within_roi_returns_self() -> TestResult {
        let ranges = two_columns()?;
        let expected = ranges.clone();
        let included = ranges.included.as_ptr();
        let clipped = ranges.clip_inplace(Roi::new(4u32..7, 0..4))?;
        assert_eq!(clipped, expected);
        assert_eq!(clipped.included.as_ptr(), included, "Same buffer");
        Ok(())
    }

    #[test]
    fn clip_inplace_restricts_pixels_and_bounds_in_the_same_buffers() -> TestResult {
        let ranges = two_columns()?;
        let (included, excluded) = (ranges.included.as_ptr(), ranges.excluded.as_ptr());
        let right = ranges.clip_inplace(Roi::new(5u32..10, 0..10))?;
        let expected =
            SortedRanges::try_from_ordered_iter([1u32..2, 3..4].with_roi(Roi::new(5u32..7, 2..4)))?;
        assert_eq!(right, expected);
        assert_eq!(right.included.as_ptr(), included, "Same buffer");
        assert_eq!(right.excluded.as_ptr(), excluded, "Same buffer");

        let lower = two_columns()?.clip_inplace(Roi::new(0u32..10, 3..10))?;
        let expected =
            SortedRanges::try_from_ordered_iter([0u32..1, 2..3].with_roi(Roi::new(4u32..7, 3..4)))?;
        assert_eq!(lower, expected);
        Ok(())
    }

    /// Clipping a range over several rows keeps it contiguous: the part in a
    /// row ends at the right edge of the clip, the one of the next row starts
    /// at the left edge
    #[test]
    fn clip_inplace_keeps_ranges_over_several_rows_contiguous() -> TestResult {
        // (2..4, 0), (0..4, 1), (0..1, 2) within bounds of 4 pixels width
        let ranges = SortedRanges::<u32>::try_from_ordered_iter(
            std::iter::once(2u32..9).with_roi(Roi::new(0u32..4, 0..3)),
        )?;
        let clipped = ranges.clip_inplace(Roi::new(1u32..3, 0..3))?;
        // (2..3, 0), (1..3, 1) within bounds of 2 pixels width
        let expected = SortedRanges::try_from_ordered_iter(
            std::iter::once(1u32..4).with_roi(Roi::new(1u32..3, 0..3)),
        )?;
        assert_eq!(clipped, expected);
        Ok(())
    }

    #[test]
    fn clip_inplace_without_remaining_pixels_is_empty() -> TestResult {
        let disjoint = two_columns()?.clip_inplace(Roi::new(10u32..20, 0..10));
        assert_eq!(disjoint, Err(PipelineError::Empty));
        // Overlaps the bounds, but not the pixels
        let gap = two_columns()?.clip_inplace(Roi::new(5u32..6, 0..10));
        assert_eq!(gap, Err(PipelineError::Empty));
        Ok(())
    }

    #[test]
    fn clip_inplace_merges_rows_becoming_full_width() -> TestResult {
        let ranges = left_half::<u8>(20)?;
        let included = ranges.included.as_ptr();
        let clipped = ranges.clip_inplace(Roi::new(0u32..10, 0..20))?;
        let expected = SortedRanges::<u8>::try_from_ordered_iter(
            std::iter::once(0u32..200).with_roi(Roi::new(0u32..10, 0..20)),
        )?;
        assert_eq!(clipped, expected);
        assert_eq!(clipped.included.as_ptr(), included, "Same buffer");
        Ok(())
    }

    /// Even if the clipped ranges would fit
    #[test]
    fn clip_inplace_fails_if_the_area_does_not_fit() -> TestResult {
        // 15x20 pixels
        let clipped = left_half::<u8>(20)?.clip_inplace(Roi::new(0u32..15, 0..20));
        assert!(
            matches!(clipped, Err(PipelineError::IncompatibleSize(_))),
            "{clipped:?}"
        );
        Ok(())
    }
}
