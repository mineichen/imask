use std::fmt::Debug;

use super::peekable::Peekable;
use crate::{CreateRange, ImageDimension, NonZeroRange, Rect, Span};

pub struct Union<TA: Iterator, TB: Iterator> {
    a: Peekable<TA>,
    b: Peekable<TB>,
}

impl<TA: Iterator + ImageDimension, TB: Iterator + ImageDimension> ImageDimension
    for Union<TA, TB>
{
    fn bounds(&self) -> Rect<u32> {
        self.a.parent.bounds().bounds(&self.b.parent.bounds())
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.a.parent.width().max(self.b.parent.width())
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
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::ImaskSet;

    use super::*;

    const NON_ZERO_10: NonZeroU32 = NonZeroU32::new(10).unwrap();
    const NON_ZERO_12: NonZeroU32 = NonZeroU32::new(12).unwrap();
    const NON_ZERO_14: NonZeroU32 = NonZeroU32::new(14).unwrap();

    #[test]
    fn bounds_are_combined() {
        let a = Rect::new(10u32, 10, NON_ZERO_10, NON_ZERO_10).into_spans();
        let b = Rect::new(8u32, 6, NON_ZERO_10, NON_ZERO_10).into_spans();
        let rect = a.union(b).bounds();
        assert_eq!(Rect::new(8u32, 6u32, NON_ZERO_12, NON_ZERO_14), rect);
    }

    #[test]
    fn combine_multiline() {
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(0..10).unwrap(), 0),
                Span::new(NonZeroRange::try_from(0..11).unwrap(), 1)
            ],
            test_both_ways(
                std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)),
                std::iter::once(Span::new(NonZeroRange::try_from(0..11).unwrap(), 1)),
            )
        );
    }
    #[test]
    fn combine_contained_sameline() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..22).unwrap(), 0)],
            test_both_ways(
                [
                    Span::new(NonZeroRange::try_from(0..10).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(12..22).unwrap(), 0)
                ],
                [
                    Span::new(NonZeroRange::try_from(8..14).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(18..20).unwrap(), 0)
                ],
            )
        );
    }
    #[test]
    fn combine_non_overlapping_sameline() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..22).unwrap(), 0)],
            test_both_ways(
                [
                    Span::new(NonZeroRange::try_from(0..10).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(12..20).unwrap(), 0)
                ],
                [
                    Span::new(NonZeroRange::try_from(8..14).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(18..22).unwrap(), 0)
                ],
            )
        );
    }

    #[test]
    fn combine_contained_or_wrapping() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..12).unwrap(), 0)],
            test_both_ways(
                std::iter::once(Span::new(NonZeroRange::try_from(2..10).unwrap(), 0)),
                std::iter::once(Span::new(NonZeroRange::try_from(0..12).unwrap(), 0)),
            )
        );
    }
    #[test]
    fn combine_overlapping_both() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..12).unwrap(), 0)],
            test_both_ways(
                std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)),
                std::iter::once(Span::new(NonZeroRange::try_from(2..12).unwrap(), 0)),
            )
        );
    }
    #[test]
    fn combine_overlapping() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..12).unwrap(), 0)],
            test_both_ways(
                std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)),
                std::iter::once(Span::new(NonZeroRange::try_from(0..12).unwrap(), 0)),
            )
        );
    }
    #[test]
    fn combine_same() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
            test_both_ways(
                std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)),
                std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)),
            )
        );
    }

    #[test]
    fn combine_touching() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..20).unwrap(), 0)],
            test_both_ways(
                std::iter::once(Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)),
                std::iter::once(Span::new(NonZeroRange::try_from(10..20).unwrap(), 0)),
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
