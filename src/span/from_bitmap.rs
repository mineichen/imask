use std::fmt::Debug;
use std::iter::Enumerate;
use std::marker::PhantomData;
use std::num::NonZero;

use crate::{CreateRange, ImageDimension, NonZeroRange, Rect, Span, UncheckedCast};

fn byte_is_nonzero(b: &u8) -> bool {
    *b != 0
}

#[derive(Clone)]
pub struct BitmapToSpanIter<I, TOut = u32> {
    iter: Enumerate<I>,
    width: NonZero<u32>,
    height: NonZero<u32>,
    _marker: PhantomData<TOut>,
}

impl<I: Iterator, TOut> BitmapToSpanIter<I, TOut> {
    pub fn from_bool_iter(iter: I, width: NonZero<u32>, height: NonZero<u32>) -> Self {
        Self {
            iter: iter.enumerate(),
            width,
            height,
            _marker: PhantomData,
        }
    }
}

impl<I, TOut> ImageDimension for BitmapToSpanIter<I, TOut> {
    fn bounds(&self) -> crate::Rect<u32> {
        Rect::new(0, 0, self.width, self.height)
    }

    fn width(&self) -> NonZero<u32> {
        self.width
    }
}

impl<'a, TOut> BitmapToSpanIter<std::iter::Map<std::slice::Iter<'a, u8>, fn(&u8) -> bool>, TOut> {
    pub fn from_byte_slice(bytes: &'a [u8], width: NonZero<u32>) -> Self {
        let width_usize = NonZero::<usize>::try_from(width)
            .expect("All images, but certainly the width, could be hold in memory");
        let len = bytes.len();
        debug_assert_eq!(len % width_usize.get(), 0);

        let height_u32 =
            u32::try_from(len / width_usize.get()).expect("image dims mustn't be > u32::MAX");
        let height = NonZero::new(height_u32).expect("Expected at least one line");
        Self::from_bool_iter(
            bytes.iter().map(byte_is_nonzero as fn(&u8) -> bool),
            width,
            height,
        )
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

        let pos = self.iter.find(|(_, x)| *x)?.0;
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
        Some(Span {
            x: NonZeroRange::new_debug_checked_zeroable(
                x_start.cast_unchecked(),
                (x_start + run_len).cast_unchecked(),
            ),
            y: y.cast_unchecked(),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (_, hi) = self.iter.size_hint();
        (0, hi)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    const N1: NonZeroU32 = NonZeroU32::new(1).unwrap();
    const N2: NonZeroU32 = NonZeroU32::new(2).unwrap();
    const N3: NonZeroU32 = NonZeroU32::new(3).unwrap();
    const N4: NonZeroU32 = NonZeroU32::new(4).unwrap();

    #[test]
    fn all_false() {
        let data = [false; 8];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), N4, N2).collect();
        assert!(spans.is_empty());
    }

    #[test]
    fn all_true() {
        let data = [true; 8];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), N4, N2).collect();
        assert_eq!(spans, vec![Span::new(0..4, 0), Span::new(0..4, 1)]);
    }

    #[test]
    fn single_pixel() {
        let data = [false, true, false, false, false, false, false, false];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), N4, N2).collect();
        assert_eq!(spans, vec![Span::new(1..2, 0)]);
    }

    #[test]
    fn multiple_spans_per_row() {
        let data = [true, false, true, true, false, false, false, false];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), N4, N2).collect();
        assert_eq!(spans, vec![Span::new(0..1, 0), Span::new(2..4, 0)]);
    }

    #[test]
    fn row_split() {
        let data = [false, true, true, true, true, false, false, false];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), N4, N2).collect();
        assert_eq!(spans, vec![Span::new(1..4, 0), Span::new(0..1, 1)]);
    }

    #[test]
    fn full_row_then_gap() {
        let data = [
            true, true, true, true, false, false, true, true, true, true, true, true,
        ];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), N4, N2).collect();
        assert_eq!(
            spans,
            vec![Span::new(0..4, 0), Span::new(2..4, 1), Span::new(0..4, 2)]
        );
    }

    #[test]
    fn with_u16_output() {
        let data = [true, false, true, true];
        let spans: Vec<Span<u16>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), N4, N1).collect();
        assert_eq!(spans, vec![Span::new(0..1u16, 0), Span::new(2..4u16, 0)]);
    }

    #[test]
    fn run_to_end_of_data() {
        let data = [false, false, true, true];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), N4, N1).collect();
        assert_eq!(spans, vec![Span::new(2..4, 0)]);
    }

    #[test]
    fn run_crosses_multiple_rows() {
        let data = [true; 12];
        let spans: Vec<Span<u32>> =
            BitmapToSpanIter::from_bool_iter(data.iter().copied(), N4, N3).collect();
        assert_eq!(
            spans,
            vec![Span::new(0..4, 0), Span::new(0..4, 1), Span::new(0..4, 2)]
        );
    }

    #[test]
    fn from_byte_slice_basic() {
        let data: [u8; 8] = [0, 1, 1, 0, 1, 0, 0, 1];
        let spans: Vec<Span<u32>> = BitmapToSpanIter::from_byte_slice(&data, N4).collect();
        assert_eq!(
            spans,
            vec![Span::new(1..3, 0), Span::new(0..1, 1), Span::new(3..4, 1)]
        );
    }
}
