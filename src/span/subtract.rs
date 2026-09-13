use std::cmp::Ordering;
use std::fmt::Debug;
use std::iter::FusedIterator;

use super::peekable::Peekable;
use crate::{CreateRange, ImageDimension, NonZeroRange, Roi, Span};

pub struct Subtract<TA: Iterator, TB: Iterator> {
    a: Peekable<TA>,
    b: Peekable<TB>,
}

impl<TA: Iterator + ImageDimension, TB: Iterator> ImageDimension for Subtract<TA, TB> {
    fn roi(&self) -> Roi<u32> {
        self.a.parent.roi()
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.a.parent.width()
    }
}

impl<TA: Iterator<Item: Clone> + Clone, TB: Iterator<Item: Clone> + Clone> Clone
    for Subtract<TA, TB>
{
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            b: self.b.clone(),
        }
    }
}

impl<TA: Iterator, TB: Iterator> Subtract<TA, TB> {
    pub fn new(a: TA, b: TB) -> Self {
        Self {
            a: Peekable {
                parent: a,
                pending: None,
            },
            b: Peekable {
                parent: b,
                pending: None,
            },
        }
    }
}

impl<TA: Iterator<Item = Span<T>>, TB: Iterator<Item = Span<T>>, T: Ord + Copy + Debug> Iterator
    for Subtract<TA, TB>
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let mut cur = self.a.pending_or_fetch()?;

            loop {
                let Some(peek_b) = self.b.peek() else {
                    return Some(cur);
                };

                match peek_b.y.cmp(&cur.y) {
                    Ordering::Greater => return Some(cur),
                    Ordering::Less => {
                        self.b.next();
                        continue;
                    }
                    Ordering::Equal => {}
                }

                if peek_b.x.end <= cur.x.start {
                    self.b.next();
                    continue;
                }
                if peek_b.x.start >= cur.x.end {
                    return Some(cur);
                }

                if peek_b.x.start <= cur.x.start {
                    if peek_b.x.end >= cur.x.end {
                        break;
                    } else {
                        cur = Span {
                            x: NonZeroRange::new_debug_checked_zeroable(peek_b.x.end, cur.x.end),
                            y: cur.y,
                        };
                        self.b.next();
                        continue;
                    }
                } else {
                    let left = Span {
                        x: NonZeroRange::new_debug_checked_zeroable(cur.x.start, peek_b.x.start),
                        y: cur.y,
                    };
                    if peek_b.x.end >= cur.x.end {
                        return Some(left);
                    } else {
                        self.a.pending = Some(Span {
                            x: NonZeroRange::new_debug_checked_zeroable(peek_b.x.end, cur.x.end),
                            y: cur.y,
                        });
                        self.b.next();
                        return Some(left);
                    }
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // todo: Depends on bounds
        let (_, hi) = self.a.size_hint_total();
        (0, hi)
    }
}

impl<TA, TB, T> FusedIterator for Subtract<TA, TB>
where
    TA: Iterator<Item = Span<T>> + FusedIterator,
    TB: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug,
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImaskSet, SortedRanges};

    #[test]
    fn subtract_has_correct_bounds() {
        let a = SortedRanges::from(Span::new(10u32..20, 2));
        let b = SortedRanges::from(Span::new(10u32..20, 3));
        let sub = a.spans::<u8>().subtract(b.spans::<u8>());
        assert_eq!(a.roi(), sub.roi());
    }

    #[test]
    fn no_overlap_different_lines() {
        assert_eq!(
            vec![Span::new(0..10, 0u16)],
            test_subtract([Span::new(0..10, 0)], [Span::new(0..10, 1)],)
        );
    }

    #[test]
    fn no_overlap_same_line() {
        assert_eq!(
            vec![Span::new(0..5, 0u16)],
            test_subtract([Span::new(0..5, 0)], [Span::new(10..15, 0)],)
        );
    }

    #[test]
    fn full_coverage() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_subtract([Span::new(5..10, 0)], [Span::new(0..15, 0)],)
        );
    }

    #[test]
    fn subtract_left() {
        assert_eq!(
            vec![Span::new(8..15, 0u16)],
            test_subtract([Span::new(5..15, 0)], [Span::new(0..8, 0)],)
        );
    }

    #[test]
    fn subtract_right() {
        assert_eq!(
            vec![Span::new(0..5, 0u16)],
            test_subtract([Span::new(0..10, 0)], [Span::new(5..15, 0)],)
        );
    }

    #[test]
    fn subtract_middle() {
        assert_eq!(
            vec![Span::new(0..5, 0u16), Span::new(15..20, 0u16),],
            test_subtract([Span::new(0..20, 0)], [Span::new(5..15, 0)],)
        );
    }

    #[test]
    fn multiple_subtractions() {
        assert_eq!(
            vec![
                Span::new(0..3, 0u16),
                Span::new(6..10, 0u16),
                Span::new(14..20, 0u16),
            ],
            test_subtract(
                [Span::new(0..20, 0)],
                [Span::new(3..6, 0), Span::new(10..14, 0),],
            )
        );
    }

    #[test]
    fn b_extends_across_a_spans() {
        assert_eq!(
            vec![Span::new(0..3, 0u16), Span::new(12..15, 0u16),],
            test_subtract(
                [Span::new(0..5, 0), Span::new(8..15, 0),],
                [Span::new(3..12, 0)],
            )
        );
    }

    #[test]
    fn identical_spans() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_subtract([Span::new(0..10, 0)], [Span::new(0..10, 0)],)
        );
    }

    #[test]
    fn empty_mask() {
        assert_eq!(
            vec![Span::new(0..10, 0u16)],
            test_subtract([Span::new(0..10, 0)], std::iter::empty(),)
        );
    }

    #[test]
    fn multiple_lines_mixed() {
        assert_eq!(
            vec![
                Span::new(0..5, 0u16),
                Span::new(3..10, 1u16),
                Span::new(0..3, 2u16),
                Span::new(7..10, 2u16),
            ],
            test_subtract(
                [
                    Span::new(0..10, 0),
                    Span::new(0..10, 1),
                    Span::new(0..10, 2),
                ],
                [Span::new(5..15, 0), Span::new(0..3, 1), Span::new(3..7, 2),],
            )
        );
    }

    #[test]
    fn b_before_all_a() {
        assert_eq!(
            vec![Span::new(5..10, 0u16)],
            test_subtract([Span::new(5..10, 0)], [Span::new(0..3, 0)],)
        );
    }

    #[test]
    fn b_after_all_a() {
        assert_eq!(
            vec![Span::new(0..5, 0u16)],
            test_subtract([Span::new(0..5, 0)], [Span::new(10..15, 0)],)
        );
    }

    #[test]
    fn touching_at_boundary() {
        assert_eq!(
            vec![Span::new(0..10, 0u16)],
            test_subtract([Span::new(0..10, 0)], [Span::new(10..20, 0)],)
        );
    }

    #[test]
    fn a_contained_in_b() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_subtract([Span::new(5..10, 0)], [Span::new(3..12, 0)],)
        );
    }

    fn test_subtract(
        a: impl IntoIterator<Item = Span<u16>>,
        b: impl IntoIterator<Item = Span<u16>>,
    ) -> Vec<Span<u16>> {
        a.into_iter().subtract(b).collect()
    }
}
