use std::{iter::FusedIterator, num::NonZero};

use crate::{ImageDimension, Rect};

#[derive(Clone)]
pub struct FoldInlineSpanIter<I: Iterator, F: FnMut(&mut A, &I::Item), A> {
    parent: I,
    accumulator: A,
    f: F,
}

impl<I, F, A> FoldInlineSpanIter<I, F, A>
where
    I: Iterator,
    F: FnMut(&mut A, &I::Item),
{
    pub fn new(parent: I, accumulator: A, f: F) -> Self {
        Self {
            parent,
            accumulator,
            f,
        }
    }

    /// Returns accumulator, which is always applied to all items in the iter
    /// If it was not fully consumed yet, the remaining items are also applied
    pub fn finish_all(mut self) -> A
    where
        I: FusedIterator,
    {
        for _ in &mut self {}
        self.accumulator
    }

    /// Returns accumulator applied to all images which were
    /// consumed already, without making sure that self.parent is exhausted
    pub fn finish_partial(self) -> A {
        self.accumulator
    }
}

impl<I, F, A> Iterator for FoldInlineSpanIter<I, F, A>
where
    I: Iterator,
    F: FnMut(&mut A, &I::Item),
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.parent
            .next()
            .inspect(|item| (self.f)(&mut self.accumulator, item))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.parent.size_hint()
    }
}

impl<I, F, A> FusedIterator for FoldInlineSpanIter<I, F, A>
where
    I: FusedIterator,
    F: FnMut(&mut A, &I::Item),
{
}

impl<I, F, A> ImageDimension for FoldInlineSpanIter<I, F, A>
where
    I: ImageDimension + Iterator,
    F: FnMut(&mut A, &I::Item),
{
    fn bounds(&self) -> Rect<u32> {
        self.parent.bounds()
    }

    fn width(&self) -> NonZero<u32> {
        self.parent.width()
    }
}
