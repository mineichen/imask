use std::{iter::FusedIterator, num::NonZero};

use crate::{ImageDimension, Rect};

#[cfg(feature = "async-io")]
pin_project_lite::pin_project! {
    #[derive(Clone, Debug)]
    pub struct InspectSpans<I, F> {
        #[pin] inner: I,
        f: F,
    }
}
#[cfg(not(feature = "async-io"))]
#[derive(Clone, Debug)]
pub struct InspectSpans<I, F> {
    inner: I,
    f: F,
}

impl<I, F> InspectSpans<I, F> {
    pub fn new(inner: I, f: F) -> Self {
        Self { inner, f }
    }

    pub fn into_inner(self) -> I {
        self.inner
    }
}

impl<I: Iterator, F: FnMut(&I::Item)> Iterator for InspectSpans<I, F> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().inspect(|item| (self.f)(item))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

#[cfg(feature = "async-io")]
impl<I: futures_core::Stream, F: FnMut(&I::Item)> futures_core::Stream for InspectSpans<I, F> {
    type Item = I::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.project();
        let opt = std::task::ready!(this.inner.poll_next(cx));
        let opt = opt.inspect(|x| (this.f)(x));
        std::task::Poll::Ready(opt)
    }
}

impl<I: FusedIterator, F: FnMut(&I::Item)> FusedIterator for InspectSpans<I, F> {}

impl<I: ImageDimension, F> ImageDimension for InspectSpans<I, F> {
    fn bounds(&self) -> Rect<u32> {
        self.inner.bounds()
    }

    fn width(&self) -> NonZero<u32> {
        self.inner.width()
    }
}

#[cfg(feature = "range-set-blaze-0_5")]
mod range_set_blaze_0_5_interop {
    use super::*;
    use range_set_blaze_0_5::{Integer, SortedDisjoint, SortedStarts};

    impl<I, F, TRangeItem> SortedStarts<TRangeItem> for InspectSpans<I, F>
    where
        I: SortedStarts<TRangeItem>,
        F: FnMut(&I::Item),
        TRangeItem: Integer,
    {
    }

    impl<I, F, TRangeItem> SortedDisjoint<TRangeItem> for InspectSpans<I, F>
    where
        I: SortedDisjoint<TRangeItem>,
        F: FnMut(&I::Item),
        TRangeItem: Integer,
    {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ImaskSet;

    const WIDTH: NonZero<u32> = NonZero::new(10).unwrap();

    #[test]
    fn forwards_items_and_calls_closure() {
        let mut seen = Vec::new();
        let result: Vec<_> = [0u32..10, 20..30]
            .with_bounds(WIDTH, WIDTH)
            .inspect_spans(|r| seen.push(r.clone()))
            .collect();
        assert_eq!(result, vec![0u32..10, 20..30]);
        assert_eq!(seen, vec![0u32..10, 20..30]);
    }

    #[test]
    fn forwards_image_dimension() {
        let inner = [0u32..10].with_bounds(WIDTH, WIDTH);
        let expected_bounds = inner.bounds();
        let expected_width = inner.width();
        let inspect = inner.inspect_spans(|_| {});
        assert_eq!(inspect.bounds(), expected_bounds);
        assert_eq!(inspect.width(), expected_width);
    }
}
