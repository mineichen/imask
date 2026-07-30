use std::{iter::FusedIterator, num::NonZero};

use crate::{ImageDimension, Rect};

pub struct InspectSpanIter<I, F> {
    parent: I,
    f: F,
}

impl<I: Clone, F: Clone> Clone for InspectSpanIter<I, F> {
    fn clone(&self) -> Self {
        Self {
            parent: self.parent.clone(),
            f: self.f.clone(),
        }
    }
}

impl<I, F> InspectSpanIter<I, F> {
    pub fn new(parent: I, f: F) -> Self {
        Self { parent, f }
    }
}

impl<I, F> Iterator for InspectSpanIter<I, F>
where
    I: Iterator,
    F: FnMut(&I::Item),
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.parent.next()?;
        (self.f)(&item);
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.parent.size_hint()
    }
}

impl<I: FusedIterator, F: FnMut(&I::Item)> FusedIterator for InspectSpanIter<I, F> {}

impl<I: ImageDimension, F> ImageDimension for InspectSpanIter<I, F> {
    fn bounds(&self) -> Rect<u32> {
        self.parent.bounds()
    }

    fn width(&self) -> NonZero<u32> {
        self.parent.width()
    }
}
