use std::ops::Add;
use std::{fmt::Debug, num::NonZeroU32};

use num_traits::{One, SaturatingSub, Zero};

use crate::{
    CheckedAddSigned, CreateRange, ImageDimension, ImaskSet, NonZeroRange, Rect, SignedNonZeroable,
    Span, UncheckedCast,
};

use super::union_all::UnionAll;

pub struct DilateSpanIter<I, T>
where
    I: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug + Add<Output = T> + SaturatingSub<Output = T> + CheckedAddSigned,
{
    inner: UnionAll<ShiftedSpanIter<I, T>>,
    offset: T,
    bounds: Rect<u32>,
}

impl<I, T> DilateSpanIter<I, T>
where
    I: Iterator<Item = Span<T>> + Clone + ImageDimension,
    T: Ord
        + Copy
        + Debug
        + Add<Output = T>
        + SaturatingSub<Output = T>
        + CheckedAddSigned
        + One
        + Zero
        + SignedNonZeroable
        + UncheckedCast<u32>,
{
    pub fn new(iter: I, offset: T::NonZero) -> Option<Self> {
        let bounds = iter.bounds();
        let x_offset: T = offset.into();
        let y_offset: T = offset.into();
        let mut iters: Vec<ShiftedSpanIter<I, T>> = Vec::new();

        for y_delta in T::one().iter_steps(offset) {
            iters.push(ShiftedSpanIter {
                parent: iter.clone(),
                x_offset,
                y_shift_unsigned: y_offset.saturating_sub(&y_delta),
            });
        }

        iters.push(ShiftedSpanIter {
            parent: iter.clone(),
            x_offset,
            y_shift_unsigned: y_offset,
        });

        for y_delta in T::one().iter_steps(offset) {
            iters.push(ShiftedSpanIter {
                parent: iter.clone(),
                x_offset,
                y_shift_unsigned: y_offset + y_delta,
            });
        }
        let roi = Rect::new(
            bounds.x.saturating_sub(x_offset.cast_unchecked()),
            bounds.y.saturating_sub(y_offset.cast_unchecked()),
            NonZeroU32::new(bounds.width.get() + x_offset.cast_unchecked()).unwrap(),
            NonZeroU32::new(bounds.height.get() + y_offset.cast_unchecked()).unwrap(),
        );

        Some(Self {
            inner: UnionAll::new(iters.with_roi(roi))?,
            offset: y_offset,
            bounds,
        })
    }
}

impl<I, T> Iterator for DilateSpanIter<I, T>
where
    I: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug + Add<Output = T> + SaturatingSub<Output = T> + CheckedAddSigned,
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Span<T>> {
        loop {
            let span = self.inner.next()?;
            let y = match span.y.checked_add_signed(-T::into_signed(self.offset)) {
                Some(y) => y,
                None => continue,
            };
            return Some(Span {
                x: NonZeroRange::new_debug_checked_zeroable(
                    span.x.start.saturating_sub(&self.offset),
                    span.x.end.saturating_sub(&self.offset),
                ),
                y,
            });
        }
    }
}

impl<I, T> ImageDimension for DilateSpanIter<I, T>
where
    I: Iterator<Item = Span<T>> + ImageDimension,
    T: Ord + Copy + Debug + Add<Output = T> + SaturatingSub<Output = T> + CheckedAddSigned,
{
    fn bounds(&self) -> Rect<u32> {
        self.bounds
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.bounds.width
    }
}

struct ShiftedSpanIter<I, T>
where
    I: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug + Add<Output = T>,
{
    parent: I,
    x_offset: T,
    y_shift_unsigned: T,
}

impl<I, T> Iterator for ShiftedSpanIter<I, T>
where
    I: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug + Add<Output = T>,
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Span<T>> {
        let span = self.parent.next()?;
        Some(Span {
            x: NonZeroRange::new_debug_checked_zeroable(
                span.x.start,
                span.x.end + self.x_offset + self.x_offset,
            ),
            y: span.y + self.y_shift_unsigned,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use crate::{ImaskSet, Rect, Span};

    const W: NonZero<u32> = NonZero::new(100).unwrap();
    const H: NonZero<u32> = NonZero::new(100).unwrap();

    #[test]
    fn dilate_2x() {
        let rect = Rect::new(50u32, 5, NonZero::new(2).unwrap(), NonZero::new(2).unwrap());
        let result: Vec<_> = rect
            .into_spans()
            .dilate(NonZero::new(2u32).unwrap())
            .unwrap()
            .collect();

        let expected: Vec<_> = (3..9).map(|y| Span::new(48..54, y)).collect();
        assert_eq!(expected, result);
    }

    #[test]
    fn dilate_1x_single_span() {
        let result: Vec<_> = vec![Span::new(5..10, 3u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate(NonZero::new(1u32).unwrap())
            .unwrap()
            .collect();

        assert_eq!(
            vec![
                Span::new(4..11, 2u32),
                Span::new(4..11, 3u32),
                Span::new(4..11, 4u32),
            ],
            result
        );
    }

    #[test]
    fn dilate_at_top_edge() {
        let result: Vec<_> = vec![Span::new(5..10, 0u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate(NonZero::new(1u32).unwrap())
            .unwrap()
            .collect();

        assert_eq!(
            vec![Span::new(4..11, 0u32), Span::new(4..11, 1u32),],
            result
        );
    }

    #[test]
    fn dilate_multiple_spans_same_row() {
        let result: Vec<_> = vec![Span::new(0..3, 5u32), Span::new(7..10, 5u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate(NonZero::new(1u32).unwrap())
            .unwrap()
            .collect();

        assert_eq!(
            vec![
                Span::new(0..4, 4u32),
                Span::new(6..11, 4u32),
                Span::new(0..4, 5u32),
                Span::new(6..11, 5u32),
                Span::new(0..4, 6u32),
                Span::new(6..11, 6u32),
            ],
            result
        );
    }

    #[test]
    fn dilate_overlapping_rows() {
        let result: Vec<_> = vec![Span::new(5..10, 5u32), Span::new(5..10, 6u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate(NonZero::new(1u32).unwrap())
            .unwrap()
            .collect();

        assert_eq!(
            vec![
                Span::new(4..11, 4u32),
                Span::new(4..11, 5u32),
                Span::new(4..11, 6u32),
                Span::new(4..11, 7u32),
            ],
            result
        );
    }

    #[test]
    fn dilate_overflow_skips_spans() {
        let result: Vec<_> = vec![Span::new(5..10, 0u32), Span::new(5..10, 5u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate(NonZero::new(3u32).unwrap())
            .unwrap()
            .collect();

        assert_eq!(
            (0..=8).map(|y| Span::new(2..13, y)).collect::<Vec<_>>(),
            result
        );
    }
}
