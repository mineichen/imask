use std::fmt::Debug;

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

    loop {
        if let Some(next) = a_iter.peek() {
            if next.y == y && next.x.start <= end {
                end = end.max(a_iter.next().unwrap().x.end);
                continue;
            }
        }
        if let Some(next) = b_iter.peek() {
            if next.y == y && next.x.start <= end {
                end = end.max(b_iter.next().unwrap().x.end);
                continue;
            }
        }
        break;
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
                std::cmp::Ordering::Equal if next_a.x.end <= next_b.x.start => self.a.next(),
                std::cmp::Ordering::Equal if next_b.x.end <= next_a.x.start => self.b.next(),
                std::cmp::Ordering::Equal => extract(&mut self.a, &mut self.b),
            },
        }
    }
}

#[derive(Clone)]
struct Peekable<TInner: Iterator> {
    parent: TInner,
    pending: Option<TInner::Item>,
}

impl<TInner: Iterator> Peekable<TInner> {
    fn next(&mut self) -> Option<TInner::Item> {
        let mut pending = self.parent.next();

        #[cfg(debug_assertions)]
        {
            if pending.is_some() {
                assert!(
                    self.pending.is_some(),
                    "Expects, that peek() is called before"
                );
            }
        }
        std::mem::swap(&mut pending, &mut self.pending);
        pending
    }
    fn peek(&mut self) -> Option<&TInner::Item> {
        match &mut self.pending {
            Some(x) => Some(x),
            r => {
                *r = self.parent.next();
                r.as_ref()
            }
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
