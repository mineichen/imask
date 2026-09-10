use std::ops::{Add, Sub};

use num_traits::{Bounded, One};

use crate::{PipelineEmptyError, Rect, SignedNonZeroable, Span};

/// Aggregates `min/max` `x`/`y` over [`Span`]s without any `Option` in the
/// hot path.
///
/// The initial state uses `T::MAX` for the minima and `T::MIN` for the maxima
/// as sentinels. [`SpanBoundsBuilder::build`] detects the untouched sentinel
/// state (`max < min`) and returns `None` for empty input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpanBoundsBuilder<T> {
    min_x: T,
    max_x_end: T,
    min_y: T,
    max_y: T,
}
impl<T: Bounded> Default for SpanBoundsBuilder<T> {
    fn default() -> Self {
        Self {
            min_x: T::max_value(),
            max_x_end: T::min_value(),
            min_y: T::max_value(),
            max_y: T::min_value(),
        }
    }
}

impl<T: Copy + Ord + Bounded> SpanBoundsBuilder<T> {
    /// Folds one span into the aggregated min/max state.
    ///
    /// Accepts spans over any `I: Into<T>`, so e.g. a
    /// `SpanBoundsBuilder<u32>` can aggregate `Span<u8>`/`Span<u16>` without
    /// converting upfront. `x.end` is exclusive and tracked as such.
    #[inline]
    pub fn add<I: Into<T>>(&mut self, span: Span<I>) {
        let x: std::ops::Range<I> = span.x.into();
        let x_start: T = x.start.into();
        let x_end: T = x.end.into();
        let y: T = span.y.into();
        if x_start < self.min_x {
            self.min_x = x_start;
        }
        if x_end > self.max_x_end {
            self.max_x_end = x_end;
        }
        if y < self.min_y {
            self.min_y = y;
        }
        if y > self.max_y {
            self.max_y = y;
        }
    }

    /// Merges another builder's state into `self` (useful for parallel folds).
    #[inline]
    pub fn merge(&mut self, other: Self) {
        if other.min_x < self.min_x {
            self.min_x = other.min_x;
        }
        if other.max_x_end > self.max_x_end {
            self.max_x_end = other.max_x_end;
        }
        if other.min_y < self.min_y {
            self.min_y = other.min_y;
        }
        if other.max_y > self.max_y {
            self.max_y = other.max_y;
        }
    }

    /// no span was added.
    ///
    /// `width = max_x_end - min_x`, `height = max_y - min_y + 1`.
    pub fn build(self) -> Result<Rect<T>, PipelineEmptyError>
    where
        T: SignedNonZeroable + Sub<Output = T> + Add<Output = T> + One,
    {
        if self.max_y < self.min_y {
            return Err(PipelineEmptyError);
        }
        debug_assert!(self.max_x_end > self.min_x);
        let width = T::create_non_zero(self.max_x_end - self.min_x)
            .expect("non-empty spans imply non-zero width");
        let height = T::create_non_zero((self.max_y - self.min_y) + T::one())
            .expect("non-empty spans imply non-zero height");
        Ok(Rect::new(self.min_x, self.min_y, width, height))
    }
}

impl<T: Copy + Ord + Bounded> Extend<Span<T>> for SpanBoundsBuilder<T> {
    #[inline]
    fn extend<I: IntoIterator<Item = Span<T>>>(&mut self, iter: I) {
        for span in iter {
            self.add(span);
        }
    }
}

impl<T: Copy + Ord + Bounded> FromIterator<Span<T>> for SpanBoundsBuilder<T> {
    #[inline]
    fn from_iter<I: IntoIterator<Item = Span<T>>>(iter: I) -> Self {
        let mut builder = Self::default();
        builder.extend(iter);
        builder
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZero, NonZeroU32};

    use super::*;

    #[test]
    fn empty_builds_none() {
        assert_eq!(
            SpanBoundsBuilder::<u32>::default().build(),
            Err(PipelineEmptyError)
        );
    }

    #[test]
    fn single_span() {
        let mut builder = SpanBoundsBuilder::<u32>::default();
        let span = Span::new(10u32..20, 2u32);
        builder.add(span);
        let expected: Rect<u32> = span.into();
        assert_eq!(builder.build(), Ok(expected));
    }

    #[test]
    fn aggregates_min_max() {
        let mut builder = SpanBoundsBuilder::<u32>::default();
        builder.add(Span::new(10u32..20, 5u32));
        builder.add(Span::new(2u32..8, 1u32));
        builder.add(Span::new(4u32..30, 9u32));
        let expected = Rect::new(2u32, 1, NonZero::new(28).unwrap(), NonZero::new(9).unwrap());
        assert_eq!(builder.build(), Ok(expected));
    }

    #[test]
    fn merge_combines_state() {
        let mut a = SpanBoundsBuilder::<u32>::default();
        a.add(Span::new(10u32..20, 5u32));
        let mut b = SpanBoundsBuilder::<u32>::default();
        b.add(Span::new(2u32..8, 1u32));
        a.merge(b);
        let expected = Rect::new(2u32, 1, NonZero::new(18).unwrap(), NonZero::new(5).unwrap());
        assert_eq!(a.build(), Ok(expected));

        let mut empty = SpanBoundsBuilder::<u32>::default();
        empty.merge(SpanBoundsBuilder::<u32>::default());
        assert!(empty.build().is_err());
    }

    #[test]
    fn from_iterator_and_extend() {
        let spans = vec![Span::new(2u32..5, 1u32), Span::new(2u32..5, 2u32)];
        let builder: SpanBoundsBuilder<u32> = spans.clone().into_iter().collect();
        let expected = Rect::new(2u32, 1, NonZero::new(3).unwrap(), NonZero::new(2).unwrap());
        assert_eq!(builder.build(), Ok(expected));

        let mut builder = SpanBoundsBuilder::<u32>::default();
        builder.extend(spans);
        assert_eq!(builder.build(), Ok(expected));
    }

    #[test]
    fn single_pixel() {
        let mut builder = SpanBoundsBuilder::<u32>::default();
        builder.add(Span::new(5u32..6, 7u32));
        let expected = Rect::new(5u32, 7, NonZeroU32::MIN, NonZeroU32::MIN);
        assert_eq!(builder.build(), Ok(expected));
    }

    #[test]
    fn works_for_u8_u16_u64_usize() {
        let mut b8 = SpanBoundsBuilder::<u8>::default();
        b8.add(Span::new(2u8..5, 1u8));
        assert!(b8.build().is_ok());

        let mut b16 = SpanBoundsBuilder::<u16>::default();
        b16.add(Span::new(2u16..5, 1u16));
        assert!(b16.build().is_ok());

        let mut b64 = SpanBoundsBuilder::<u64>::default();
        b64.add(Span::new(2u64..5, 1u64));
        assert!(b64.build().is_ok());

        let mut bsize = SpanBoundsBuilder::<usize>::default();
        bsize.add(Span::new(2usize..5, 1usize));
        assert!(bsize.build().is_ok());
    }
}
