use std::{iter::FusedIterator, num::NonZero};

use crate::{ImageDimension, Roi};

#[cfg(feature = "async-io")]
pin_project_lite::pin_project! {
    #[derive(Clone, Debug)]
    pub struct WithRoi<I> {
        #[pin] inner: I,
        roi: Roi<u32>,
    }
}
#[cfg(not(feature = "async-io"))]
#[derive(Clone, Debug)]
pub struct WithRoi<I> {
    inner: I,
    roi: Roi<u32>,
}

impl<I> WithRoi<I> {
    pub fn new(inner: I, roi: impl Into<Roi<u32>>) -> Self {
        Self {
            inner,
            roi: roi.into(),
        }
    }

    pub fn into_inner(self) -> I {
        self.inner
    }
}

impl<I: Iterator> Iterator for WithRoi<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

#[cfg(feature = "async-io")]
impl<I: futures_core::Stream> futures_core::Stream for WithRoi<I> {
    type Item = I::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.project();
        this.inner.poll_next(cx)
    }
}

impl<I: FusedIterator> FusedIterator for WithRoi<I> {}

impl<I> ImageDimension for WithRoi<I> {
    fn roi(&self) -> Roi<u32> {
        self.roi
    }
    fn width(&self) -> NonZero<u32> {
        self.roi.width()
    }
}

#[cfg(feature = "range-set-blaze-0_5")]
mod with_bounds_range_set_blaze_0_5 {
    use super::*;
    use range_set_blaze_0_5::{Integer, SortedDisjoint, SortedStarts};

    impl<T, TRangeItem> SortedStarts<TRangeItem> for WithRoi<T>
    where
        T: SortedStarts<TRangeItem>,
        TRangeItem: Integer,
    {
    }
    impl<T, TRangeItem> SortedDisjoint<TRangeItem> for WithRoi<T>
    where
        T: SortedDisjoint<TRangeItem>,
        TRangeItem: Integer,
    {
    }
}
