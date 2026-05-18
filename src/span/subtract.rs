use std::cmp::Ordering;
use std::fmt::Debug;

use super::peekable::Peekable;
use crate::{CreateRange, ImageDimension, NonZeroRange, Rect, Span};

pub struct Subtract<TA: Iterator, TB: Iterator> {
    a: Peekable<TA>,
    b: Peekable<TB>,
}

impl<TA: Iterator + ImageDimension, TB: Iterator + ImageDimension> ImageDimension
    for Subtract<TA, TB>
{
    fn bounds(&self) -> Rect<u32> {
        self.a.parent.bounds()
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ImaskSet;

    #[test]
    fn no_overlap_different_lines() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16)],
            test_subtract(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 1)],
            )
        );
    }

    #[test]
    fn no_overlap_same_line() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..5).unwrap(), 0u16)],
            test_subtract(
                [Span::new(NonZeroRange::try_from(0..5).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(10..15).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn full_coverage() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_subtract(
                [Span::new(NonZeroRange::try_from(5..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(0..15).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn subtract_left() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(8..15).unwrap(), 0u16)],
            test_subtract(
                [Span::new(NonZeroRange::try_from(5..15).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(0..8).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn subtract_right() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..5).unwrap(), 0u16)],
            test_subtract(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(5..15).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn subtract_middle() {
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(0..5).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(15..20).unwrap(), 0u16),
            ],
            test_subtract(
                [Span::new(NonZeroRange::try_from(0..20).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(5..15).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn multiple_subtractions() {
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(0..3).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(6..10).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(14..20).unwrap(), 0u16),
            ],
            test_subtract(
                [Span::new(NonZeroRange::try_from(0..20).unwrap(), 0)],
                [
                    Span::new(NonZeroRange::try_from(3..6).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(10..14).unwrap(), 0),
                ],
            )
        );
    }

    #[test]
    fn b_extends_across_a_spans() {
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(0..3).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(12..15).unwrap(), 0u16),
            ],
            test_subtract(
                [
                    Span::new(NonZeroRange::try_from(0..5).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(8..15).unwrap(), 0),
                ],
                [Span::new(NonZeroRange::try_from(3..12).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn identical_spans() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_subtract(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn empty_mask() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16)],
            test_subtract(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                std::iter::empty(),
            )
        );
    }

    #[test]
    fn multiple_lines_mixed() {
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(0..5).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(3..10).unwrap(), 1u16),
                Span::new(NonZeroRange::try_from(0..3).unwrap(), 2u16),
                Span::new(NonZeroRange::try_from(7..10).unwrap(), 2u16),
            ],
            test_subtract(
                [
                    Span::new(NonZeroRange::try_from(0..10).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(0..10).unwrap(), 1),
                    Span::new(NonZeroRange::try_from(0..10).unwrap(), 2),
                ],
                [
                    Span::new(NonZeroRange::try_from(5..15).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(0..3).unwrap(), 1),
                    Span::new(NonZeroRange::try_from(3..7).unwrap(), 2),
                ],
            )
        );
    }

    #[test]
    fn b_before_all_a() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(5..10).unwrap(), 0u16)],
            test_subtract(
                [Span::new(NonZeroRange::try_from(5..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(0..3).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn b_after_all_a() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..5).unwrap(), 0u16)],
            test_subtract(
                [Span::new(NonZeroRange::try_from(0..5).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(10..15).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn touching_at_boundary() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16)],
            test_subtract(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(10..20).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn a_contained_in_b() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_subtract(
                [Span::new(NonZeroRange::try_from(5..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(3..12).unwrap(), 0)],
            )
        );
    }

    fn test_subtract(
        a: impl IntoIterator<Item = Span<u16>>,
        b: impl IntoIterator<Item = Span<u16>>,
    ) -> Vec<Span<u16>> {
        a.into_iter().subtract(b).collect()
    }
}
