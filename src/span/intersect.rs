use std::cmp::Ordering;
use std::fmt::Debug;
use std::iter::FusedIterator;

use super::peekable::Peekable;
use crate::{CreateRange, ImageDimension, NonZeroRange, PipelineEmptyError, Roi, Span};

pub struct Intersect<TA: Iterator, TB: Iterator> {
    a: Peekable<TA>,
    b: Peekable<TB>,
    roi: Roi<u32>,
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
    fn roi(&self) -> Roi<u32> {
        self.roi
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.roi.width()
    }
}

impl<TA: Iterator<Item: Clone> + Clone, TB: Iterator<Item: Clone> + Clone> Clone
    for Intersect<TA, TB>
{
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            b: self.b.clone(),
            roi: self.roi,
            #[cfg(debug_assertions)]
            last_a: self.last_a.clone(),
            #[cfg(debug_assertions)]
            last_b: self.last_b.clone(),
        }
    }
}

impl<TA: Iterator, TB: Iterator> Intersect<TA, TB> {
    pub fn new(a: TA, b: TB) -> Result<Self, PipelineEmptyError>
    where
        TA: ImageDimension,
        TB: ImageDimension,
    {
        let roi = a.roi().intersection(&b.roi()).ok_or(PipelineEmptyError)?;
        Ok(Self {
            a: Peekable::new(a),
            b: Peekable::new(b),
            roi,
            #[cfg(debug_assertions)]
            last_a: None,
            #[cfg(debug_assertions)]
            last_b: None,
        })
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

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (_, a_hi) = self.a.size_hint_total();
        let (_, b_hi) = self.b.size_hint_total();
        let hi = match (a_hi, b_hi) {
            (Some(a), Some(b)) => Some(a.min(b)),
            _ => None,
        };
        (0, hi)
    }
}

impl<TA, TB, T> FusedIterator for Intersect<TA, TB>
where
    TA: Iterator<Item = Span<T>>,
    TB: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug,
{
}

#[cfg(test)]
mod tests {
    use testresult::TestResult;

    use crate::{ImaskSet, Roi};
    use crate::{NonZeroRange, SortedRanges};

    use super::*;

    const TEST_BOUNDS: Roi<u32> = Roi {
        x: NonZeroRange::<u32>::new_const(0..50),
        y: NonZeroRange::<u32>::new_const(0..10),
    };

    fn w(
        a: impl IntoIterator<Item = Span<u16>>,
    ) -> impl Iterator<Item = Span<u16>> + ImageDimension {
        a.into_iter().with_roi(TEST_BOUNDS)
    }

    #[test]
    fn width_matches_bounds_for_offset_inputs() -> TestResult {
        let a = SortedRanges::from(Span::new(0u16..10, 0));
        let b = SortedRanges::from(Span::new(5u16..15, 0));
        let intersect = Intersect::new(a.spans::<u32>(), b.spans::<u32>())?;
        assert_eq!(
            intersect.width(),
            intersect.roi().width(),
            "width() must equal roi().width()"
        );
        Ok(())
    }

    #[test]
    fn disjoint_bounds_returns_empty_error() {
        let a = Roi::new(0u32..10, 0..10).into_spans();
        let b = Roi::new(20u32..30, 20..30).into_spans();
        assert_eq!(Intersect::new(a, b).err(), Some(PipelineEmptyError));
    }

    #[test]
    fn no_overlap_different_lines() -> TestResult {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect([Span::new(0..10, 0)], [Span::new(0..10, 1)],)?
        );
        Ok(())
    }

    #[test]
    fn no_overlap_same_line() -> TestResult {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect([Span::new(0..5, 0)], [Span::new(10..15, 0)],)?
        );
        Ok(())
    }

    #[test]
    fn identical_spans() -> TestResult {
        assert_eq!(
            vec![Span::new(0..10, 0u16)],
            test_intersect([Span::new(0..10, 0)], [Span::new(0..10, 0)],)?
        );
        Ok(())
    }

    #[test]
    fn a_contained_in_b() -> TestResult {
        assert_eq!(
            vec![Span::new(5..10, 0u16)],
            test_intersect([Span::new(5..10, 0)], [Span::new(3..12, 0)],)?
        );
        Ok(())
    }

    #[test]
    fn b_contained_in_a() -> TestResult {
        assert_eq!(
            vec![Span::new(3..12, 0u16)],
            test_intersect([Span::new(0..20, 0)], [Span::new(3..12, 0)],)?
        );
        Ok(())
    }

    #[test]
    fn overlapping_both() -> TestResult {
        assert_eq!(
            vec![Span::new(2..10, 0u16)],
            test_intersect([Span::new(0..10, 0)], [Span::new(2..12, 0)],)?
        );
        Ok(())
    }

    #[test]
    fn touching_at_boundary_no_overlap() -> TestResult {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect([Span::new(0..10, 0)], [Span::new(10..20, 0)],)?
        );
        Ok(())
    }

    #[test]
    fn empty_a() -> TestResult {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect(std::iter::empty(), [Span::new(0..10, 0)],)?
        );
        Ok(())
    }

    #[test]
    fn empty_b() -> TestResult {
        assert_eq!(
            Vec::<Span<u16>>::new(),
            test_intersect([Span::new(0..10, 0)], std::iter::empty(),)?
        );
        Ok(())
    }

    #[test]
    fn multiple_overlaps_same_line() -> TestResult {
        assert_eq!(
            vec![Span::new(3..5, 0u16), Span::new(10..15, 0u16),],
            test_intersect(
                [Span::new(0..5, 0), Span::new(8..15, 0),],
                [Span::new(3..6, 0), Span::new(10..20, 0),],
            )?
        );
        Ok(())
    }

    #[test]
    fn span_extends_across_other_spans() -> TestResult {
        assert_eq!(
            vec![Span::new(3..5, 0u16), Span::new(8..12, 0u16),],
            test_intersect(
                [Span::new(0..5, 0), Span::new(8..15, 0),],
                [Span::new(3..12, 0)],
            )?
        );
        Ok(())
    }

    #[test]
    fn multiple_lines() -> TestResult {
        assert_eq!(
            vec![
                Span::new(5..10, 0u16),
                Span::new(3..10, 1u16),
                Span::new(3..7, 2u16),
            ],
            test_intersect(
                [
                    Span::new(0..10, 0),
                    Span::new(0..10, 1),
                    Span::new(0..10, 2),
                ],
                [Span::new(5..15, 0), Span::new(3..12, 1), Span::new(3..7, 2),],
            )?
        );
        Ok(())
    }

    #[test]
    fn is_commutative() -> TestResult {
        let a = vec![Span::new(0..10, 0u16), Span::new(5..15, 1u16)];
        let b = vec![Span::new(3..12, 0u16), Span::new(0..8, 1u16)];

        let ab = Intersect::new(w(a.clone()), w(b.clone()))?.collect::<Vec<_>>();
        let ba = Intersect::new(w(b), w(a))?.collect::<Vec<_>>();
        assert_eq!(ab, ba);
        Ok(())
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "must be sorted and disjoint")
    )]
    fn unsorted_input_panics() {
        let Ok(iter) = Intersect::new(
            w([Span::new(0..10, 0u16), Span::new(2..5, 0u16)]),
            w([Span::new(0..10, 1u16)]),
        ) else {
            panic!("expected overlapping bounds");
        };
        let _ = iter.collect::<Vec<_>>();
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "must be sorted and disjoint")
    )]
    fn overlapping_input_panics() {
        let Ok(iter) = Intersect::new(
            w([Span::new(0..5, 0u16), Span::new(3..10, 0u16)]),
            w([Span::new(0..10, 1u16)]),
        ) else {
            panic!("expected overlapping bounds");
        };
        let _ = iter.collect::<Vec<_>>();
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "must be sorted and disjoint")
    )]
    fn touching_input_panics() {
        let Ok(iter) = Intersect::new(
            w([Span::new(0..5, 0u16), Span::new(5..10, 0u16)]),
            w([Span::new(0..10, 0u16)]),
        ) else {
            panic!("expected overlapping bounds");
        };
        let _ = iter.collect::<Vec<_>>();
    }

    fn test_intersect(
        a: impl IntoIterator<Item = Span<u16>> + Clone,
        b: impl IntoIterator<Item = Span<u16>> + Clone,
    ) -> Result<Vec<Span<u16>>, PipelineEmptyError> {
        let a_first = Intersect::new(w(a.clone()), w(b.clone()))?.collect::<Vec<_>>();
        let b_first = Intersect::new(w(b), w(a))?.collect::<Vec<_>>();
        assert_eq!(a_first, b_first);
        Ok(a_first)
    }
}
