use std::fmt::Debug;
use std::iter::FusedIterator;
use std::ops::{Add, Sub};

use crate::{
    ImageDimension, NonZeroRange, PipelineEmptyError, PipelineError, Roi, SignedNonZeroable, Span,
};

pub struct ClipSpanIter<TIter, T: SignedNonZeroable> {
    parent: TIter,
    clip: Roi<T>,
    output_bounds: Roi<u32>,
    pending: Option<Span<T>>,
}

impl<TIter, T> ClipSpanIter<TIter, T>
where
    TIter: Iterator<Item = Span<T>> + ImageDimension,
    T: SignedNonZeroable
        + TryFrom<u32, Error: Into<PipelineError>>
        + Ord
        + Add<Output = T>
        + Sub<Output = T>
        + Copy
        + Debug,
{
    pub fn new(mut parent: TIter, roi: impl Into<Roi<u32>>) -> Result<Self, PipelineError> {
        let roi = roi.into();
        let output_bounds = parent.roi().intersection(&roi).ok_or(PipelineEmptyError)?;

        let x_start = T::try_from(output_bounds.x.start).map_err(Into::into)?;
        let x_end = T::try_from(output_bounds.x.end).map_err(Into::into)?;
        let y_start = T::try_from(output_bounds.y.start).map_err(Into::into)?;
        let y_end = T::try_from(output_bounds.y.end).map_err(Into::into)?;
        let clip = Roi {
            x: NonZeroRange::new_unchecked(x_start..x_end),
            y: NonZeroRange::new_unchecked(y_start..y_end),
        };

        // First in-range span; `None` means empty output, which must fail here
        // since a constructed iterator encodes exhaustiveness as `pending == None`.
        let pending = parent
            .find(|span| span.y >= clip.y.start)
            .ok_or(PipelineEmptyError)?;

        Ok(Self {
            parent,
            clip,
            output_bounds,
            pending: Some(pending),
        })
    }
}

impl<TIter: ImageDimension, T: SignedNonZeroable> ImageDimension for ClipSpanIter<TIter, T> {
    fn roi(&self) -> Roi<u32> {
        self.output_bounds
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.output_bounds.width()
    }
}

impl<TIter: Iterator<Item = Span<T>>, T: SignedNonZeroable + Ord + Debug + Add<Output = T> + Copy>
    Iterator for ClipSpanIter<TIter, T>
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let span = self.pending.take()?;
            if span.y >= self.clip.y.end {
                return None;
            }
            self.pending = self.parent.next();
            let range = span.x.intersection(&self.clip.x);
            if let Some(range) = range {
                return Some(Span {
                    x: range,
                    y: span.y,
                });
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // todo: Can be improved: If parent is inbound, parent bounds can be returned
        let (_, hi) = self.parent.size_hint();
        let pending = self.pending.is_some() as usize;
        (0, hi.map(|h| h.saturating_add(pending)))
    }
}

impl<TIter: Iterator<Item = Span<T>>, T: SignedNonZeroable + Ord + Debug + Add<Output = T> + Copy>
    FusedIterator for ClipSpanIter<TIter, T>
{
}

#[cfg(test)]
mod tests {

    use testresult::TestResult;

    use crate::{ImaskSet, PipelineError, Roi};

    use super::*;

    #[test]
    fn smaller_bounds_do_crop() -> TestResult {
        let src = Roi::new(10u32..20, 10..20);
        let bounds = Roi::new(12u32..17, 12..17);
        let expected = bounds.into_spans().collect::<Vec<_>>();
        let clipped = ClipSpanIter::new(src.into_spans(), bounds)?.collect::<Vec<_>>();
        assert_eq!(expected, clipped);
        Ok(())
    }

    #[test]
    fn bigger_bounds_have_no_effect() -> TestResult {
        let src = Roi::new(10u32..20, 10..20);
        let iter = src.into_spans();
        let expected = iter.clone().collect::<Vec<_>>();
        let bounds = Roi::new(0u32..100, 0..100);
        let clipped = ClipSpanIter::new(iter, bounds)?.collect::<Vec<_>>();
        assert_eq!(expected, clipped);
        Ok(())
    }

    #[test]
    fn with_no_overlapping_parts() -> TestResult {
        let src = Roi::new(10u32..20, 10..20);
        let iter = src.into_spans();
        let expected = iter.clone().collect::<Vec<_>>();
        let bounds = Roi::new(0u32..100, 0..100);
        let clipped = ClipSpanIter::new(
            iter.union(Roi::new(100u32..110, 10..20).into_spans()),
            bounds,
        )?
        .collect::<Vec<_>>();
        assert_eq!(expected, clipped);
        Ok(())
    }

    #[test]
    fn clip_returns_intersection_bounds() -> TestResult {
        let source = Roi::new(0u32..100, 0..100);
        let roi = Roi::new(10u32..90, 10..120);
        let expected_bounds = Roi::new(10u32..90, 10..100);

        let clipped = ClipSpanIter::new(source.into_spans(), roi)?;
        assert_eq!(expected_bounds, clipped.roi());

        let spans: Vec<_> = clipped.collect();
        let expected_spans: Vec<_> = expected_bounds.into_spans().collect();
        assert_eq!(expected_spans, spans);
        Ok(())
    }

    #[test]
    fn disjoint_bounds_returns_empty_error() {
        let src = Roi::new(10u32..20, 10..20);
        let bounds = Roi::new(30u32..40, 30..40);
        assert_eq!(
            ClipSpanIter::new(src.into_spans(), bounds).err(),
            Some(PipelineError::Empty)
        );
    }

    #[test]
    fn unrepresentable_bounds_returns_incompatible_size() {
        let parent = vec![Span::new(0u8..10, 0u8)].with_roi(Roi::new(0u32..300, 0..10));
        let err = ClipSpanIter::new(parent, Roi::new(0u32..300, 0..10)).err();
        assert!(matches!(err, Some(PipelineError::IncompatibleSize(_))));
    }
}
