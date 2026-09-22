use std::{
    fmt::Debug,
    iter::FusedIterator,
    num::NonZeroU32,
    ops::{Add, Sub},
};

use num_traits::One;

use crate::{ImageDimension, NonZeroRange, Roi, SignedNonZeroable, Span, UncheckedCast};

#[derive(Clone)]
pub struct RectSpanIter<T> {
    span: Span<T>,
    y_start: T,
    y_end: T,
}

impl<T: SignedNonZeroable + Ord + Debug + Copy + Add<Output = T> + PartialEq> RectSpanIter<T> {
    pub fn new(rect: impl Into<Roi<T>>) -> Self {
        let rect = rect.into();
        let span = Span {
            x: rect.x,
            y: rect.y.start,
        };
        Self {
            span,
            y_start: rect.y.start,
            y_end: rect.y.end,
        }
    }
}

impl<T: UncheckedCast<u32>> ImageDimension for RectSpanIter<T> {
    #[inline]
    fn roi(&self) -> Roi<u32> {
        let x_start = self.span.x.start.cast_unchecked();
        let x_end = self.span.x.end.cast_unchecked();
        // Declared start, not the cursor: `roi()` stays stable while iterating
        // (and stays valid once exhausted, when `span.y == y_end`).
        let y_start = self.y_start.cast_unchecked();
        let y_end = self.y_end.cast_unchecked();
        debug_assert!(x_start < x_end);
        debug_assert!(y_start < y_end);
        Roi {
            x: NonZeroRange::new_unchecked(x_start..x_end),
            y: NonZeroRange::new_unchecked(y_start..y_end),
        }
    }

    #[inline]
    fn width(&self) -> std::num::NonZero<u32> {
        NonZeroU32::new(self.span.x.end.cast_unchecked() - self.span.x.start.cast_unchecked())
            .expect("X mustn't be zero length")
    }
}

impl<T: Ord + One + Copy + Add<Output = T> + Sub<Output = T> + TryInto<usize>> Iterator
    for RectSpanIter<T>
{
    type Item = Span<T>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.span.y < self.y_end {
            let r = Some(self.span);
            self.span.y = self.span.y + T::one();
            r
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match (self.y_end - self.span.y).try_into() {
            Ok(size) => (size, Some(size)),
            Err(_) => (0, None),
        }
    }
}

impl<T: Ord + One + Copy + Add<Output = T> + Sub<Output = T> + TryInto<usize>> FusedIterator
    for RectSpanIter<T>
{
}

#[cfg(test)]
mod tests {
    use crate::Span;

    use super::*;
    #[test]
    fn rect_iter() {
        let rect = Roi::new(10u32..20, 10..20);
        let iter = RectSpanIter::new(rect);
        let expected: Vec<Span<u32>> = (0..10).map(|y| Span::new(10..20, y + 10)).collect();
        assert_eq!(expected, iter.collect::<Vec<_>>());
    }
}
