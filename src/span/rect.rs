use std::{fmt::Debug, num::NonZeroU32, ops::Add};

use num_traits::One;

use crate::{ImageDimension, Rect, SignedNonZeroable, Span, UncheckedCast};

#[derive(Clone)]
pub struct RectSpanIter<T> {
    span: Span<T>,
    y_end: T,
}

impl<T: SignedNonZeroable + Ord + Debug + Copy + Add<Output = T> + PartialEq> RectSpanIter<T> {
    pub fn new(rect: Rect<T>) -> Self {
        let span = Span::new(rect.x..rect.len_x().into(), rect.y);
        Self {
            span,
            y_end: rect.len_y().into(),
        }
    }
}

impl<T: UncheckedCast<u32>> ImageDimension for RectSpanIter<T> {
    fn bounds(&self) -> Rect<u32> {
        let x_start = self.span.x.start.cast_unchecked();
        let x_end = self.span.x.end.cast_unchecked();
        let y_start = self.span.y.cast_unchecked();
        let y_end = self.y_end.cast_unchecked();
        Rect {
            x: x_start,
            y: y_start,
            width: NonZeroU32::new(x_end - x_start).expect("X mustn't be zero length"),
            height: NonZeroU32::new(y_end - y_start).expect("Y mustn't be zero length"),
        }
    }

    fn width(&self) -> std::num::NonZero<u32> {
        NonZeroU32::new(self.span.x.end.cast_unchecked())
            .expect("End must be > start, so it cannot be 0")
    }
}

impl<T: Ord + One + Copy + Add<Output = T>> Iterator for RectSpanIter<T> {
    type Item = Span<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.span.y < self.y_end {
            let r = Some(self.span);
            self.span.y = self.span.y + T::one();
            r
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::{Rect, Span};

    use super::*;
    const NON_ZERO_10: NonZeroU32 = NonZeroU32::new(10).unwrap();
    #[test]
    fn rect_iter() {
        let rect = Rect::new(10u32, 10, NON_ZERO_10, NON_ZERO_10);
        let iter = RectSpanIter::new(rect);
        let expected: Vec<Span<u32>> = (0..10).map(|y| Span::new(10..20, y + 10)).collect();
        assert_eq!(expected, iter.collect::<Vec<_>>());
    }
}
