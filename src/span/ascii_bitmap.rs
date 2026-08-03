use std::{
    array, fmt::Debug, iter::Flatten, marker::PhantomData, num::NonZeroU32, ops::Add, slice::Iter,
};

use crate::{ImageDimension, Rect, SignedNonZeroable, Span, UncheckedCast};

#[derive(Clone, Copy)]
pub(crate) struct AsciiBitmap<const WIDTH: usize, const HEIGHT: usize> {
    data: [[u8; WIDTH]; HEIGHT],
}

pub(crate) struct AsciiBitmapIter<T, const WIDTH: usize, const HEIGHT: usize> {
    bitmap: AsciiBitmap<WIDTH, HEIGHT>,
    data_y: usize,
    data_x: usize,
    phantom: PhantomData<T>,
}

const fn usize_to_nonzero_u32(x: usize) -> NonZeroU32 {
    let as_u32 = x as u32;
    if as_u32 as usize != x {
        panic!("Invalid cast")
    }
    NonZeroU32::new(as_u32).unwrap()
}

impl<const WIDTH: usize, const HEIGHT: usize> AsciiBitmap<WIDTH, HEIGHT> {
    pub fn new(data: [[u8; WIDTH]; HEIGHT]) -> Self {
        Self { data: data }
    }
    pub fn iter<T>(self) -> AsciiBitmapIter<T, WIDTH, HEIGHT> {
        AsciiBitmapIter {
            bitmap: self,
            phantom: PhantomData,
            data_y: 0,
            data_x: 0,
        }
    }
}

impl<T, const WIDTH: usize, const HEIGHT: usize> ImageDimension
    for AsciiBitmapIter<T, WIDTH, HEIGHT>
{
    fn bounds(&self) -> Rect<u32> {
        let height = const { usize_to_nonzero_u32(HEIGHT) };
        let width = self.width();
        Rect {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    fn width(&self) -> std::num::NonZero<u32> {
        const { usize_to_nonzero_u32(WIDTH) }
    }
}

impl<
    T: SignedNonZeroable + Eq + Ord + Add<Output = T> + Copy + Debug,
    const WIDTH: usize,
    const HEIGHT: usize,
> Iterator for AsciiBitmapIter<T, WIDTH, HEIGHT>
where
    usize: UncheckedCast<T>,
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let row = 'outer: loop {
            let row = self.bitmap.data.get(self.data_y)?;
            while let Some(x) = row.get(self.data_x) {
                if *x == b'#' {
                    break 'outer row;
                }
                self.data_x += 1;
            }

            self.data_y += 1;
            self.data_x = 0;
        };

        let start_x = self.data_x.cast_unchecked();
        self.data_x += 1;

        while let Some(x) = row.get(self.data_x) {
            if *x != b'#' {
                break;
            }
            self.data_x += 1;
        }
        let end_x = self.data_x.cast_unchecked();
        Some(Span::new(start_x..end_x, self.data_y.cast_unchecked()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        #[rustfmt::skip]
        let ascii = AsciiBitmap::new([
            *b".#..###.##",
            *b".#..###.#.",
            *b"..........",
            *b".#..###..#",
        ]).iter::<u16>();
        assert_eq!(
            vec![
                Span::new(1u16..2, 0),
                Span::new(4..7, 0),
                Span::new(8..10, 0),
                Span::new(1u16..2, 1),
                Span::new(4..7, 1),
                Span::new(8..9, 1),
                Span::new(1u16..2, 3),
                Span::new(4..7, 3),
                Span::new(9..10, 3)
            ],
            ascii.collect::<Vec<_>>()
        );
    }
    #[test]
    fn empty() {
        #[rustfmt::skip]
        let a = AsciiBitmap::new([
            *b"..........",
            *b"..........",
        ]).iter().collect::<Vec<Span<u16>>>();
        let b = AsciiBitmap::new([[b'.']])
            .iter()
            .collect::<Vec<Span<u16>>>();
        assert_eq!(a, b);
    }
}
