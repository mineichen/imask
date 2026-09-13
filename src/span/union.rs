use std::fmt::Debug;

use super::peekable::Peekable;
use crate::{CreateRange, ImageDimension, NonZeroRange, Roi, Span};

pub struct Union<TA: Iterator, TB: Iterator> {
    a: Peekable<TA>,
    b: Peekable<TB>,
}

impl<TA: Iterator + ImageDimension, TB: Iterator + ImageDimension> ImageDimension
    for Union<TA, TB>
{
    fn roi(&self) -> Roi<u32> {
        let a_bounds = self.a.parent.roi();
        let b_bounds = self.b.parent.roi();
        debug_assert_eq!(
            a_bounds.width(),
            self.a.parent.width(),
            "Union parent A width must equal its roi().width()"
        );
        debug_assert_eq!(
            b_bounds.width(),
            self.b.parent.width(),
            "Union parent B width must equal its roi().width()"
        );
        a_bounds.union(&b_bounds)
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.roi().width()
    }
}

impl<TA: Iterator<Item: Clone> + Clone, TB: Iterator<Item: Clone> + Clone> Clone for Union<TA, TB> {
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            b: self.b.clone(),
        }
    }
}

impl<TA: Iterator, TB: Iterator> Union<TA, TB> {
    pub fn new(a: TA, b: TB) -> Self {
        Self {
            a: Peekable::new(a),
            b: Peekable::new(b),
        }
    }
}

fn extract<T: Ord + Copy + Debug>(
    a_iter: &mut Peekable<impl Iterator<Item = Span<T>>>,
    b_iter: &mut Peekable<impl Iterator<Item = Span<T>>>,
) -> Option<Span<T>> {
    let a = a_iter.next().unwrap();
    let b = b_iter.next().unwrap();
    let y = a.y;
    let start = a.x.start.min(b.x.start);
    let mut end = a.x.end.max(b.x.end);
    let mut a_end = a.x.end;
    let mut b_end = b.x.end;

    loop {
        if a_end <= b_end {
            match a_iter.peek() {
                Some(next) if next.y == y && next.x.start <= end => {
                    let consumed = a_iter.next().unwrap();
                    a_end = consumed.x.end;
                    end = end.max(a_end);
                }
                _ => break,
            }
        } else {
            match b_iter.peek() {
                Some(next) if next.y == y && next.x.start <= end => {
                    let consumed = b_iter.next().unwrap();
                    b_end = consumed.x.end;
                    end = end.max(b_end);
                }
                _ => break,
            }
        }
    }

    let x = NonZeroRange::new_debug_checked_zeroable(start, end);
    Some(Span { x, y })
}

impl<TA: Iterator<Item = Span<T>>, TB: Iterator<Item = Span<T>>, T: Ord + Copy + Debug> Iterator
    for Union<TA, TB>
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Self::Item> {
        match (self.a.peek(), self.b.peek()) {
            (None, None) => None,
            (None, Some(_)) => self.b.next(),
            (Some(_), None) => self.a.next(),
            (Some(next_a), Some(next_b)) => match next_a.y.cmp(&next_b.y) {
                std::cmp::Ordering::Less => self.a.next(),
                std::cmp::Ordering::Greater => self.b.next(),
                std::cmp::Ordering::Equal if next_a.x.end < next_b.x.start => self.a.next(),
                std::cmp::Ordering::Equal if next_b.x.end < next_a.x.start => self.b.next(),
                std::cmp::Ordering::Equal => extract(&mut self.a, &mut self.b),
            },
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (a_lo, a_hi) = self.a.size_hint_total();
        let (b_lo, b_hi) = self.b.size_hint_total();
        let lo = a_lo.max(b_lo);
        let hi = match (a_hi, b_hi) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            _ => None,
        };
        (lo, hi)
    }
}

#[cfg(test)]
mod tests {

    use crate::ImaskSet;

    use super::*;

    #[test]
    fn bounds_are_combined() {
        let a = Roi::new(10u32..20, 10..20).into_spans();
        let b = Roi::new(8u32..18, 6..16).into_spans();
        let rect = a.union(b).roi();
        assert_eq!(Roi::new(8u32..20, 6..20), rect);
    }

    #[test]
    fn width_matches_bounds_for_offset_inputs() {
        let a = Roi::new(0u32..10, 0..10).into_spans();
        let b = Roi::new(5u32..15, 0..10).into_spans();
        let union = a.union(b);
        assert_eq!(
            union.width(),
            union.roi().width(),
            "width() must equal roi().width()"
        );
    }

    #[test]
    fn combine_multiline() {
        assert_eq!(
            vec![Span::new(0..10, 0), Span::new(0..11, 1)],
            test_both_ways(
                std::iter::once(Span::new(0..10, 0)),
                std::iter::once(Span::new(0..11, 1)),
            )
        );
    }
    #[test]
    fn combine_contained_sameline() {
        assert_eq!(
            vec![Span::new(0..22, 0)],
            test_both_ways(
                [Span::new(0..10, 0), Span::new(12..22, 0)],
                [Span::new(8..14, 0), Span::new(18..20, 0)],
            )
        );
    }
    #[test]
    fn combine_non_overlapping_sameline() {
        assert_eq!(
            vec![Span::new(0..22, 0)],
            test_both_ways(
                [Span::new(0..10, 0), Span::new(12..20, 0)],
                [Span::new(8..14, 0), Span::new(18..22, 0)],
            )
        );
    }

    #[test]
    fn combine_contained_or_wrapping() {
        assert_eq!(
            vec![Span::new(0..12, 0)],
            test_both_ways(
                std::iter::once(Span::new(2..10, 0)),
                std::iter::once(Span::new(0..12, 0)),
            )
        );
    }
    #[test]
    fn combine_overlapping_both() {
        assert_eq!(
            vec![Span::new(0..12, 0)],
            test_both_ways(
                std::iter::once(Span::new(0..10, 0)),
                std::iter::once(Span::new(2..12, 0)),
            )
        );
    }
    #[test]
    fn combine_overlapping() {
        assert_eq!(
            vec![Span::new(0..12, 0)],
            test_both_ways(
                std::iter::once(Span::new(0..10, 0)),
                std::iter::once(Span::new(0..12, 0)),
            )
        );
    }
    #[test]
    fn combine_same() {
        assert_eq!(
            vec![Span::new(0..10, 0)],
            test_both_ways(
                std::iter::once(Span::new(0..10, 0)),
                std::iter::once(Span::new(0..10, 0)),
            )
        );
    }

    #[test]
    fn combine_touching() {
        assert_eq!(
            vec![Span::new(0..20, 0)],
            test_both_ways(
                std::iter::once(Span::new(0..10, 0)),
                std::iter::once(Span::new(10..20, 0)),
            )
        );
    }

    fn test_both_ways(
        a: impl IntoIterator<Item = Span<u16>, IntoIter: Clone>,
        b: impl IntoIterator<Item = Span<u16>, IntoIter: Clone>,
    ) -> Vec<Span<u16>> {
        let a = a.into_iter();
        let b = b.into_iter();
        let first = Union::new(a.clone(), b.clone()).collect::<Vec<_>>();
        let second = Union::new(b, a).collect::<Vec<_>>();

        assert_eq!(first, second);
        first
    }
}
