use std::{cell::RefCell, marker::PhantomData, num::NonZero, rc::Rc};

use crate::{CreateRange, ImageDimension, SignedNonZeroable, UncheckedCast};

/// result of
pub struct ChunkByRowRanges<T: Iterator, R> {
    shared: Rc<RefCell<Shared<T>>>,
    range: PhantomData<R>,
}

impl<T: Iterator, R: CreateRange> ChunkByRowRanges<T, R> {
    /// # Panics
    /// If the previous RowIterator is kept when getting the next RowIterator
    pub(crate) fn new(source: T) -> Self {
        ChunkByRowRanges {
            shared: Rc::new(RefCell::new(Shared {
                source,
                pending_nextline: None,
            })),
            range: PhantomData,
        }
    }
}

struct Shared<T: Iterator> {
    source: T,
    pending_nextline: Option<T::Item>,
}

impl<T, R> Iterator for ChunkByRowRanges<T, R>
where
    T: Iterator<Item = R> + ImageDimension,
    R: CreateRange,
    R::Item: Copy
        + Ord
        + std::ops::Add<Output = R::Item>
        + std::ops::Sub<Output = R::Item>
        + std::ops::Div<Output = R::Item>
        + std::ops::Mul<Output = R::Item>,
    u32: UncheckedCast<R::Item>,
{
    type Item = (R::Item, ChunkByRowRangesRowIter<T, R>);

    fn next(&mut self) -> Option<Self::Item> {
        debug_assert!(
            Rc::strong_count(&self.shared) == 1,
            "The active SubIterator must be dropped before requesting the next row"
        );

        let mut shared = self.shared.borrow_mut();
        let width: R::Item = shared.source.width().get().cast_unchecked();

        let range = shared
            .pending_nextline
            .take()
            .or_else(|| shared.source.next())?;

        let start = range.start();
        let row = start / width;
        let next_line_start = start / width * width + width;

        Some((
            row,
            ChunkByRowRangesRowIter {
                shared: Rc::clone(&self.shared),
                pending: Some(range),
                phantom: PhantomData,
                next_line_start,
            },
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let pending = usize::from(self.shared.pending_nextline.is_some());
        let (lo, hi) = self.shared.source.size_hint();
        let lo = usize::from(lo > 0 || pending > 0);
        (lo, hi.map(|h| h.saturating_add(pending)))
    }
}
pub struct ChunkByRowRangesRowIter<T, R>
where
    T: Iterator<Item = R>,
    R: CreateRange,
    R::Item: Copy + Ord + std::ops::Sub<Output = R::Item>,
{
    shared: Rc<RefCell<Shared<T>>>,
    pending: Option<T::Item>,
    phantom: PhantomData<R>,
    next_line_start: R::Item,
}

impl<T, R> Drop for ChunkByRowRangesRowIter<T, R>
where
    T: Iterator<Item = R>,
    R: CreateRange,
    R::Item: Copy + Ord + std::ops::Sub<Output = R::Item>,
{
    fn drop(&mut self) {
        let mut shared = self.shared.borrow_mut();
        if self.pending.take().is_some() && shared.pending_nextline.is_none() {
            for r in &mut shared.source {
                if r.start() >= self.next_line_start {
                    shared.pending_nextline = Some(r);
                    break;
                } else if r.end() > self.next_line_start {
                    let remaining_len = r.end() - self.next_line_start;
                    shared.pending_nextline = Some(R::new_debug_checked_zeroable(
                        self.next_line_start,
                        remaining_len,
                    ));
                    break;
                }
            }
        }
    }
}

impl<T, R> Iterator for ChunkByRowRangesRowIter<T, R>
where
    T: Iterator<Item = R>,
    R: CreateRange,
    R::Item: Copy + Ord + std::ops::Sub<Output = R::Item>,
{
    type Item = R;

    fn next(&mut self) -> Option<Self::Item> {
        let mut shared = self.shared.borrow_mut();
        let range = self.pending.take().or_else(|| {
            shared.source.next().and_then(|r| {
                if r.start() >= self.next_line_start {
                    shared.pending_nextline = Some(r);
                    return None;
                }
                Some(r)
            })
        })?;

        if range.end() > self.next_line_start {
            let remaining_len = unsafe {
                SignedNonZeroable::create_non_zero_unchecked(range.end() - self.next_line_start)
            };

            shared.pending_nextline =
                Some(R::new_debug_checked(self.next_line_start, remaining_len));

            let clip_len = unsafe {
                SignedNonZeroable::create_non_zero_unchecked(self.next_line_start - range.start())
            };
            Some(R::new_debug_checked(range.start(), clip_len))
        } else {
            Some(range)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let pending = self.pending.is_some() as usize;
        let (lo, hi) = self.shared.borrow().source.size_hint();
        let lo = usize::from(lo > 0 || pending > 0);
        (lo, hi.map(|h| h.saturating_add(pending)))
    }
}

impl<T, R> ImageDimension for ChunkByRowRanges<T, R>
where
    T: Iterator<Item = R> + ImageDimension,
    R: CreateRange<Item: SignedNonZeroable>,
{
    fn width(&self) -> NonZero<u32> {
        self.shared.borrow().source.width()
    }

    fn bounds(&self) -> crate::Rect<u32> {
        self.shared.borrow().source.bounds()
    }
}

impl<T, R> ImageDimension for ChunkByRowRangesRowIter<T, R>
where
    T: Iterator<Item = R> + ImageDimension,
    R: CreateRange,
    R::Item: Copy + Ord + std::ops::Sub<Output = R::Item>,
{
    fn width(&self) -> NonZero<u32> {
        self.shared.borrow().source.width()
    }
    fn bounds(&self) -> crate::Rect<u32> {
        self.shared.borrow().source.bounds()
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZero, ops::Range};

    use super::*;
    use crate::{ImageDimension, ImaskSet};
    const WIDTH_U32: NonZero<u32> = NonZero::new(10u32).unwrap();

    #[test]
    fn split_without_linebreak() {
        let source = [0..4, 5..10, 11..20].with_bounds(WIDTH_U32, WIDTH_U32);
        let chunked = ChunkByRowRanges::<_, Range<usize>>::new(source);
        assert_eq!(chunked.width(), WIDTH_U32);
        let sums = chunked
            .map(|(row, i)| (row, i.collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        assert_eq!(sums, vec!((0, vec![0..4, 5..10]), (1, vec![11..20])));
    }
    #[test]
    fn split_with_linebreak() {
        let source = [0..20].with_bounds(WIDTH_U32, WIDTH_U32);
        let sums = ChunkByRowRanges::<_, Range<usize>>::new(source)
            .map(|(row, i)| (row, i.map(|x| x.len()).sum::<usize>()))
            .collect::<Vec<_>>();
        assert_eq!(sums, vec!((0, 10), (1, 10)));
    }
    #[test]
    fn split_with_filtered() {
        let source = [0..3, 6..11, 12..20].with_bounds(WIDTH_U32, WIDTH_U32);
        let sums = ChunkByRowRanges::<_, Range<usize>>::new(source)
            .skip(1)
            .map(|(row, i)| (row, i.collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        assert_eq!(sums, vec!((1, vec![10..11, 12..20])));
    }
    #[test]
    fn split_with_filtered_empty() {
        let source = [0..3, 4..5, 6..11, 12..20].with_bounds(WIDTH_U32, WIDTH_U32);
        let sums = ChunkByRowRanges::<_, Range<usize>>::new(source)
            .skip(1)
            .map(|(row, i)| (row, i.map(|x| x.len()).sum::<usize>()))
            .collect::<Vec<_>>();
        assert_eq!(sums, vec!((1, 9)));
    }
}
