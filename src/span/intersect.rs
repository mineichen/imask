use std::cmp::Ordering;
use std::fmt::Debug;

use super::peekable::Peekable;
use crate::{CreateRange, ImageDimension, NonZeroRange, Rect, Span};

pub struct Intersect<TA: Iterator, TB: Iterator> {
    a: Peekable<TA>,
    b: Peekable<TB>,
    #[cfg(debug_assertions)]
    last_a: Option<TA::Item>,
    #[cfg(debug_assertions)]
    last_b: Option<TB::Item>,
}

#[cfg(debug_assertions)]
fn assert_sorted_and_disjoint<T: Ord + Copy + Debug>(last: &Option<Span<T>>, current: &Span<T>) {
    if let Some(last) = last {
        assert!(
            current.y > last.y || (current.y == last.y && current.x.start > last.x.end),
            "Intersect: input spans must be sorted and disjoint (no overlapping or touching), got {:?} followed by {:?}",
            last,
            current
        );
    }
}

impl<TA: Iterator + ImageDimension, TB: Iterator + ImageDimension> ImageDimension
    for Intersect<TA, TB>
{
    fn bounds(&self) -> Rect<u32> {
        let a = self.a.parent.bounds();
        let b = self.b.parent.bounds();
        let x = a.x.max(b.x);
        let y = a.y.max(b.y);
        let x_end = (a.x + a.width.get()).min(b.x + b.width.get());
        let y_end = (a.y + a.height.get()).min(b.y + b.height.get());
        Rect {
            x,
            y,
            width: std::num::NonZero::new(x_end.saturating_sub(x))
                .unwrap_or_else(|| std::num::NonZero::new(1).unwrap()),
            height: std::num::NonZero::new(y_end.saturating_sub(y))
                .unwrap_or_else(|| std::num::NonZero::new(1).unwrap()),
        }
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.a.parent.width().min(self.b.parent.width())
    }
}

impl<TA: Iterator<Item: Clone> + Clone, TB: Iterator<Item: Clone> + Clone> Clone
    for Intersect<TA, TB>
{
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            b: self.b.clone(),
            #[cfg(debug_assertions)]
            last_a: self.last_a.clone(),
            #[cfg(debug_assertions)]
            last_b: self.last_b.clone(),
        }
    }
}

impl<TA: Iterator, TB: Iterator> Intersect<TA, TB> {
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
            #[cfg(debug_assertions)]
            last_a: None,
            #[cfg(debug_assertions)]
            last_b: None,
        }
    }
}

impl<TA: Iterator<Item = Span<T>>, TB: Iterator<Item = Span<T>>, T: Ord + Copy + Debug> Iterator
    for Intersect<TA, TB>
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (Some(next_a), Some(next_b)) = (self.a.peek(), self.b.peek()) else {
                return None;
            };

            let next_a = *next_a;
            let next_b = *next_b;

            #[cfg(debug_assertions)]
            {
                if self.last_a != Some(next_a) {
                    assert_sorted_and_disjoint(&self.last_a, &next_a);
                    self.last_a = Some(next_a);
                }
                if self.last_b != Some(next_b) {
                    assert_sorted_and_disjoint(&self.last_b, &next_b);
                    self.last_b = Some(next_b);
                }
            }

            match next_a.y.cmp(&next_b.y) {
                Ordering::Less => {
                    self.a.next();
                    continue;
                }
                Ordering::Greater => {
                    self.b.next();
                    continue;
                }
                Ordering::Equal => {}
            }

            if next_a.x.end <= next_b.x.start {
                self.a.next();
                continue;
            }
            if next_b.x.end <= next_a.x.start {
                self.b.next();
                continue;
            }

            let result_x = next_a.x.intersection(&next_b.x).unwrap();

            match next_a.x.end.cmp(&next_b.x.end) {
                Ordering::Less => {
                    self.a.next();
                    self.b.pending = Some(Span {
                        x: NonZeroRange::new_debug_checked_zeroable(next_a.x.end, next_b.x.end),
                        y: next_b.y,
                    });
                    #[cfg(debug_assertions)]
                    {
                        self.last_b = self.b.pending;
                    }
                }
                Ordering::Equal => {
                    self.a.next();
                    self.b.next();
                }
                Ordering::Greater => {
                    self.b.next();
                    self.a.pending = Some(Span {
                        x: NonZeroRange::new_debug_checked_zeroable(next_b.x.end, next_a.x.end),
                        y: next_a.y,
                    });
                    #[cfg(debug_assertions)]
                    {
                        self.last_a = self.a.pending;
                    }
                }
            }

            return Some(Span {
                x: result_x,
                y: next_a.y,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_overlap_different_lines() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 1)],
            )
        );
    }

    #[test]
    fn no_overlap_same_line() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect(
                [Span::new(NonZeroRange::try_from(0..5).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(10..15).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn identical_spans() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16)],
            test_intersect(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn a_contained_in_b() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(5..10).unwrap(), 0u16)],
            test_intersect(
                [Span::new(NonZeroRange::try_from(5..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(3..12).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn b_contained_in_a() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(3..12).unwrap(), 0u16)],
            test_intersect(
                [Span::new(NonZeroRange::try_from(0..20).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(3..12).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn overlapping_both() {
        assert_eq!(
            vec![Span::new(NonZeroRange::try_from(2..10).unwrap(), 0u16)],
            test_intersect(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(2..12).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn touching_at_boundary_no_overlap() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                [Span::new(NonZeroRange::try_from(10..20).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn empty_a() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect(
                std::iter::empty(),
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn empty_b() {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect(
                [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0)],
                std::iter::empty(),
            )
        );
    }

    #[test]
    fn multiple_overlaps_same_line() {
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(3..5).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(10..15).unwrap(), 0u16),
            ],
            test_intersect(
                [
                    Span::new(NonZeroRange::try_from(0..5).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(8..15).unwrap(), 0),
                ],
                [
                    Span::new(NonZeroRange::try_from(3..6).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(10..20).unwrap(), 0),
                ],
            )
        );
    }

    #[test]
    fn span_extends_across_other_spans() {
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(3..5).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(8..12).unwrap(), 0u16),
            ],
            test_intersect(
                [
                    Span::new(NonZeroRange::try_from(0..5).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(8..15).unwrap(), 0),
                ],
                [Span::new(NonZeroRange::try_from(3..12).unwrap(), 0)],
            )
        );
    }

    #[test]
    fn multiple_lines() {
        assert_eq!(
            vec![
                Span::new(NonZeroRange::try_from(5..10).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(3..10).unwrap(), 1u16),
                Span::new(NonZeroRange::try_from(3..7).unwrap(), 2u16),
            ],
            test_intersect(
                [
                    Span::new(NonZeroRange::try_from(0..10).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(0..10).unwrap(), 1),
                    Span::new(NonZeroRange::try_from(0..10).unwrap(), 2),
                ],
                [
                    Span::new(NonZeroRange::try_from(5..15).unwrap(), 0),
                    Span::new(NonZeroRange::try_from(3..12).unwrap(), 1),
                    Span::new(NonZeroRange::try_from(3..7).unwrap(), 2),
                ],
            )
        );
    }

    #[test]
    fn is_commutative() {
        let a = vec![
            Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16),
            Span::new(NonZeroRange::try_from(5..15).unwrap(), 1u16),
        ];
        let b = vec![
            Span::new(NonZeroRange::try_from(3..12).unwrap(), 0u16),
            Span::new(NonZeroRange::try_from(0..8).unwrap(), 1u16),
        ];

        let ab = Intersect::new(a.clone().into_iter(), b.clone().into_iter()).collect::<Vec<_>>();
        let ba = Intersect::new(b.into_iter(), a.into_iter()).collect::<Vec<_>>();
        assert_eq!(ab, ba);
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "must be sorted and disjoint")
    )]
    fn unsorted_input_panics() {
        let _ = Intersect::new(
            [
                Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(2..5).unwrap(), 0u16),
            ]
            .into_iter(),
            [Span::new(NonZeroRange::try_from(0..10).unwrap(), 1u16)].into_iter(),
        )
        .collect::<Vec<_>>();
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "must be sorted and disjoint")
    )]
    fn overlapping_input_panics() {
        let _ = Intersect::new(
            [
                Span::new(NonZeroRange::try_from(0..5).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(3..10).unwrap(), 0u16),
            ]
            .into_iter(),
            [Span::new(NonZeroRange::try_from(0..10).unwrap(), 1u16)].into_iter(),
        )
        .collect::<Vec<_>>();
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "must be sorted and disjoint")
    )]
    fn touching_input_panics() {
        let _ = Intersect::new(
            [
                Span::new(NonZeroRange::try_from(0..5).unwrap(), 0u16),
                Span::new(NonZeroRange::try_from(5..10).unwrap(), 0u16),
            ]
            .into_iter(),
            [Span::new(NonZeroRange::try_from(0..10).unwrap(), 0u16)].into_iter(),
        )
        .collect::<Vec<_>>();
    }

    fn test_intersect(
        a: impl IntoIterator<Item = Span<u16>> + Clone,
        b: impl IntoIterator<Item = Span<u16>> + Clone,
    ) -> Vec<Span<u16>> {
        let a_first =
            Intersect::new(a.clone().into_iter(), b.clone().into_iter()).collect::<Vec<_>>();
        let b_first = Intersect::new(b.into_iter(), a.into_iter()).collect::<Vec<_>>();
        assert_eq!(a_first, b_first);
        a_first
    }
}
