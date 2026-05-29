use std::fmt::Debug;
use std::iter::Enumerate;
use std::marker::PhantomData;
use std::num::NonZero;

use crate::{CreateRange, UncheckedCast};

fn byte_is_nonzero(b: &u8) -> bool {
    *b != 0
}

#[derive(Clone)]
pub struct BitmapToRangeIter<I, TRange = std::ops::Range<u32>> {
    iter: Enumerate<I>,
    _marker: PhantomData<TRange>,
}

impl<I: Iterator, TRange> BitmapToRangeIter<I, TRange> {
    pub fn from_bool_iter(iter: I) -> Self {
        Self {
            iter: iter.enumerate(),
            _marker: PhantomData,
        }
    }
}

impl<'a, TRange>
    BitmapToRangeIter<std::iter::Map<std::slice::Iter<'a, u8>, fn(&u8) -> bool>, TRange>
{
    pub fn from_byte_slice(bytes: &'a [u8], width: NonZero<u32>) -> Self {
        debug_assert_eq!(bytes.len() % width.get() as usize, 0);
        Self::from_bool_iter(bytes.iter().map(byte_is_nonzero as fn(&u8) -> bool))
    }
}

impl<I: Iterator<Item = bool>, TRange> Iterator for BitmapToRangeIter<I, TRange>
where
    TRange: CreateRange,
    TRange::Item: Copy + Debug + Ord,
    u32: UncheckedCast<TRange::Item>,
{
    type Item = TRange;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let pos = (&mut self.iter).filter(|(_, is_set)| *is_set).next()?.0;
        let start: u32 = pos.cast_unchecked();
        let mut end = start + 1;
        loop {
            match self.iter.next() {
                Some((_, true)) => end += 1,
                Some((_, false)) => break,
                None => break,
            }
        }
        return Some(TRange::new_debug_checked_zeroable(
            start.cast_unchecked(),
            end.cast_unchecked(),
        ));
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
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_bool_iter(data.iter().copied()).collect();
        assert!(ranges.is_empty());
    }

    #[test]
    fn all_true() {
        let data = [true; 8];
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_bool_iter(data.iter().copied()).collect();
        assert_eq!(ranges, vec![0..8]);
    }

    #[test]
    fn single_pixel() {
        let data = [false, true, false, false];
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_bool_iter(data.iter().copied()).collect();
        assert_eq!(ranges, vec![1..2]);
    }

    #[test]
    fn multiple_ranges() {
        let data = [true, false, true, true, false, false, false, false];
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_bool_iter(data.iter().copied()).collect();
        assert_eq!(ranges, vec![0..1, 2..4]);
    }

    #[test]
    fn run_spans_multiple_lines_not_split() {
        let data = [true; 12];
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_bool_iter(data.iter().copied()).collect();
        assert_eq!(ranges, vec![0..12]);
    }

    #[test]
    fn run_crosses_line_boundary_single_range() {
        let data = [false, true, true, true, true, true, false, false];
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_bool_iter(data.iter().copied()).collect();
        assert_eq!(ranges, vec![1..6]);
    }

    #[test]
    fn empty_iter() {
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_bool_iter(std::iter::empty()).collect();
        assert!(ranges.is_empty());
    }

    #[test]
    fn trailing_true() {
        let data = [false, false, true, true];
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_bool_iter(data.iter().copied()).collect();
        assert_eq!(ranges, vec![2..4]);
    }

    #[test]
    fn alternating() {
        let data = [true, false, true, false, true];
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_bool_iter(data.iter().copied()).collect();
        assert_eq!(ranges, vec![0..1, 2..3, 4..5]);
    }

    #[test]
    fn from_byte_slice_basic() {
        let data: [u8; 8] = [0, 1, 1, 0, 1, 1, 0, 0];
        let ranges: Vec<std::ops::Range<u32>> =
            BitmapToRangeIter::from_byte_slice(&data, W4).collect();
        assert_eq!(ranges, vec![1..3, 4..6]);
    }
}
