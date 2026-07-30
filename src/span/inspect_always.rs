use std::{iter::FusedIterator, num::NonZero};

use crate::{ImageDimension, Rect};

#[derive(Clone)]
pub struct InspectAlwaysSpanIter<I: Iterator, F: FnMut(&I::Item)> {
    parent: I,
    f: F,
    exhausted: bool,
}

impl<I: Iterator, F: FnMut(&I::Item)> InspectAlwaysSpanIter<I, F> {
    pub fn new(parent: I, f: F) -> Self {
        Self {
            parent,
            f,
            exhausted: false,
        }
    }
}

impl<I, F> Iterator for InspectAlwaysSpanIter<I, F>
where
    I: Iterator,
    F: FnMut(&I::Item),
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        match self.parent.next() {
            Some(item) => {
                (self.f)(&item);
                Some(item)
            }
            None => {
                self.exhausted = true;
                None
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.parent.size_hint()
    }
}

impl<I: FusedIterator, F: FnMut(&I::Item)> FusedIterator for InspectAlwaysSpanIter<I, F> {}

impl<I: ImageDimension + Iterator, F: FnMut(&I::Item)> ImageDimension
    for InspectAlwaysSpanIter<I, F>
{
    fn bounds(&self) -> Rect<u32> {
        self.parent.bounds()
    }

    fn width(&self) -> NonZero<u32> {
        self.parent.width()
    }
}

impl<I: Iterator, F: FnMut(&I::Item)> Drop for InspectAlwaysSpanIter<I, F> {
    fn drop(&mut self) {
        if !self.exhausted && !std::thread::panicking() {
            (&mut self.parent).for_each(|x| (self.f)(&x));
        }
    }
}
