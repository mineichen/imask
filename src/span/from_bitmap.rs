use std::fmt::Debug;
use std::iter::Enumerate;
use std::marker::PhantomData;
use std::num::NonZero;

use crate::{CreateRange, NonZeroRange, Span, UncheckedCast};

fn byte_is_nonzero(b: &u8) -> bool {
    *b != 0
}

#[derive(Clone)]
pub struct BitmapToSpanIter<I, TOut = u32> {
    iter: Enumerate<I>,
    width: NonZero<u32>,
    _marker: PhantomData<TOut>,
}

impl<I: Iterator, TOut> BitmapToSpanIter<I, TOut> {
    pub fn from_bool_iter(iter: I, width: NonZero<u32>) -> Self {
        Self {
            iter: iter.enumerate(),
            width,
            _marker: PhantomData,
        }
    }

    pub fn width(&self) -> NonZero<u32> {
        self.width
    }
}

impl<'a, TOut> BitmapToSpanIter<std::iter::Map<std::slice::Iter<'a, u8>, fn(&u8) -> bool>, TOut> {
    pub fn from_byte_slice(bytes: &'a [u8], width: NonZero<u32>) -> Self {
        debug_assert_eq!(bytes.len() % width.get() as usize, 0);
        Self::from_bool_iter(bytes.iter().map(byte_is_nonzero as fn(&u8) -> bool), width)
    }
}

impl<I: Iterator<Item = bool>, TOut> Iterator for BitmapToSpanIter<I, TOut>
where
    TOut: Copy + Debug + Ord,
    u32: UncheckedCast<TOut>,
{
    type Item = Span<TOut>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let width = self.width.get();

        let pos = (&mut self.iter).filter(|(_, x)| *x).next()?.0;
        let pos: u32 = pos.cast_unchecked();
        let y = pos / width;
        let x_start = pos - y * width;
        let mut run_len = 1u32;
        for _ in 0..width - x_start - 1 {
            match self.iter.next() {
                Some((_, true)) => run_len += 1,
                Some((_, false)) => break,
                None => break,
            }
        }
        return Some(Span {
            x: NonZeroRange::new_debug_checked_zeroable(
                x_start.cast_unchecked(),
                (x_start + run_len).cast_unchecked(),
            ),
            y: y.cast_unchecked(),
        });
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    const W4: NonZeroU32 = NonZeroU32::new(4).unwrap();

    #[test]
    fn all_false() {
        let data = [false; 8];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), W4).collect();
        assert!(spans.is_empty());
    }

    #[test]
    fn all_true() {
        let data = [true; 8];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), W4).collect();
        assert_eq!(spans, vec![Span::new(0..4, 0), Span::new(0..4, 1)]);
    }

    #[test]
    fn single_pixel() {
        let data = [false, true, false, false, false, false, false, false];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), W4).collect();
        assert_eq!(spans, vec![Span::new(1..2, 0)]);
    }

    #[test]
    fn multiple_spans_per_row() {
        let data = [true, false, true, true, false, false, false, false];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), W4).collect();
        assert_eq!(spans, vec![Span::new(0..1, 0), Span::new(2..4, 0)]);
    }

    #[test]
    fn row_split() {
        let data = [false, true, true, true, true, false, false, false];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), W4).collect();
        assert_eq!(spans, vec![Span::new(1..4, 0), Span::new(0..1, 1)]);
    }

    #[test]
    fn full_row_then_gap() {
        let data = [
            true, true, true, true, false, false, true, true, true, true, true, true,
        ];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), W4).collect();
        assert_eq!(
            spans,
            vec![Span::new(0..4, 0), Span::new(2..4, 1), Span::new(0..4, 2)]
        );
    }

    #[test]
    fn with_u16_output() {
        let data = [true, false, true, true];
        let spans: Vec<Span<u16>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), W4).collect();
        assert_eq!(spans, vec![Span::new(0..1u16, 0), Span::new(2..4u16, 0)]);
    }

    #[test]
    fn run_to_end_of_data() {
        let data = [false, false, true, true];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), W4).collect();
        assert_eq!(spans, vec![Span::new(2..4, 0)]);
    }

    #[test]
    fn run_crosses_multiple_rows() {
        let data = [true; 12];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), W4).collect();
        assert_eq!(
            spans,
            vec![Span::new(0..4, 0), Span::new(0..4, 1), Span::new(0..4, 2)]
        );
    }

    #[test]
    fn empty_iter() {
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(std::iter::empty(), W4).collect();
        assert!(spans.is_empty());
    }

    #[test]
    fn from_byte_slice_basic() {
        let data: [u8; 8] = [0, 1, 1, 0, 1, 0, 0, 1];
        let spans: Vec<Span<u32>> = BitmapToSpanIter::from_byte_slice(&data, W4).collect();
        assert_eq!(
            spans,
            vec![Span::new(1..3, 0), Span::new(0..1, 1), Span::new(3..4, 1)]
        );
    }
}
