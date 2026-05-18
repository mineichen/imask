use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::binary_heap::PeekMut;
use std::fmt::Debug;

use crate::{CreateRange, NonZeroRange, Span};

pub struct UnionAll<I: Iterator> {
    heap: BinaryHeap<PendingIter<I>>,
    accumulator: Option<I::Item>,
}

impl<I: Iterator<Item: Ord>> UnionAll<I> {
    pub fn new(iters: impl IntoIterator<Item = I>) -> Self {
        let mut heap = BinaryHeap::new();
        for mut iter in iters {
            if let Some(pending) = iter.next() {
                heap.push(PendingIter {
                    pending: Some(pending),
                    iter,
                });
            }
        }
        Self {
            heap,
            accumulator: None,
        }
    }
}

impl<I, T> Iterator for UnionAll<I>
where
    I: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug,
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Span<T>> {
        loop {
            let item = match self.heap.peek_mut() {
                Some(mut entry) => {
                    let item = entry.pending.take().unwrap();
                    entry.pending = entry.iter.next();
                    if entry.pending.is_none() {
                        PeekMut::pop(entry);
                    }
                    item
                }
                None => return self.accumulator.take(),
            };

            match self.accumulator.take() {
                None => {
                    self.accumulator = Some(item);
                }
                Some(acc) => {
                    if item.y == acc.y && item.x.start <= acc.x.end {
                        self.accumulator = Some(Span {
                            x: NonZeroRange::new_debug_checked_zeroable(
                                acc.x.start,
                                acc.x.end.max(item.x.end),
                            ),
                            y: acc.y,
                        });
                    } else {
                        self.accumulator = Some(item);
                        return Some(acc);
                    }
                }
            }
        }
    }
}

struct PendingIter<I: Iterator> {
    pending: Option<I::Item>,
    iter: I,
}

impl<I: Iterator<Item: Ord>> PartialEq for PendingIter<I> {
    fn eq(&self, other: &Self) -> bool {
        self.pending == other.pending
    }
}

impl<I: Iterator<Item: Ord>> Eq for PendingIter<I> {}

impl<I: Iterator<Item: Ord>> PartialOrd for PendingIter<I> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<I: Iterator<Item: Ord>> Ord for PendingIter<I> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.pending, &other.pending) {
            (None, None) => Ordering::Equal,
            (_, None) => Ordering::Greater,
            (None, _) => Ordering::Less,
            (Some(a), Some(b)) => b.cmp(a),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ImaskSet;

    #[test]
    fn empty() {
        let result: Vec<Span<u16>> =
            UnionAll::new(std::iter::empty::<std::vec::IntoIter<Span<u16>>>()).collect();
        assert!(result.is_empty());
    }

    #[test]
    fn single_iterator() {
        let iter: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0))
            .collect();
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16)],
            UnionAll::new(std::iter::once(iter.into_iter())).collect::<Vec<_>>()
        );
    }

    #[test]
    fn two_non_overlapping() {
        let a: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(0..5).unwrap(), 0))
            .collect();
        let b: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(10..15).unwrap(), 0))
            .collect();
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(0..5).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(10..15).unwrap(), 0u16),
            ],
            UnionAll::new([a.into_iter(), b.into_iter()]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn two_overlapping() {
        let a: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0))
            .collect();
        let b: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(5..15).unwrap(), 0))
            .collect();
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..15).unwrap(), 0u16)],
            UnionAll::new([a.into_iter(), b.into_iter()]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn three_overlapping() {
        let a: Vec<_> = vec![Span::new(NonZeroRange::try_from(0..5).unwrap(), 0)];
        let b: Vec<_> = vec![Span::new(NonZeroRange::try_from(3..8).unwrap(), 0)];
        let c: Vec<_> = vec![Span::new(NonZeroRange::try_from(6..12).unwrap(), 0)];
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..12).unwrap(), 0u16)],
            UnionAll::new([a.into_iter(), b.into_iter(), c.into_iter()])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn same_spans() {
        let a: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0))
            .collect();
        let b: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0))
            .collect();
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16)],
            UnionAll::new([a.into_iter(), b.into_iter()]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn different_lines() {
        let a: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0))
            .collect();
        let b: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 1))
            .collect();
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(0..10).unwrap(), 1u16),
            ],
            UnionAll::new([a.into_iter(), b.into_iter()]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn some_empty_iterators() {
        let a: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0))
            .collect();
        let b: Vec::<Span<u16>> = vec![];
        let c: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(5..15).unwrap(), 0))
            .collect();
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..15).unwrap(), 0u16)],
            UnionAll::new([a.into_iter(), b.into_iter(), c.into_iter()])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn complex_merge() {
        let a: Vec<_> = vec![
            Span::new(NonZeroRange::try_from(0..5).unwrap(), 0u16),
            Span::new(NonZeroRange::try_from(0..5).unwrap(), 1),
        ];
        let b: Vec<_> = vec![
            Span::new(NonZeroRange::try_from(3..8).unwrap(), 0u16),
            Span::new(NonZeroRange::try_from(0..5).unwrap(), 2),
        ];
        let c: Vec<_> = vec![
            Span::new(NonZeroRange::try_from(6..10).unwrap(), 0u16),
            Span::new(NonZeroRange::try_from(3..8).unwrap(), 1),
        ];
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(0..8).unwrap(), 1u16),
                Span::new(NonZeroRange::try_from(0..5).unwrap(), 2u16),
            ],
            UnionAll::new([a.into_iter(), b.into_iter(), c.into_iter()])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn via_imaskset() {
        let a: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0))
            .collect();
        let b: Vec<_> = std::iter::once(Span::new(NonZeroRange::try_from(5..15).unwrap(), 0))
            .collect();
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..15).unwrap(), 0u16)],
            vec![a.into_iter(), b.into_iter()]
                .into_iter()
                .union_all()
                .collect::<Vec<_>>()
        );
    }
}
