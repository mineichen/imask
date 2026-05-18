use std::fmt::Debug;
use std::iter::FusedIterator;
use std::num::NonZeroU32;
use std::ops::Add;

use num_traits::{CheckedSub, One, SaturatingSub, Zero};
use range_set_blaze_0_5::{CheckSortedDisjoint, DynSortedDisjoint, Integer, SortedDisjoint};

use crate::{
    CreateRange, ImageDimension, Rect, SanitizeSortedDisjoint, SignedNonZeroable, UncheckedCast,
};

// pub struct DilateIter<TIter>
// where
//     TIter: Iterator<
//         Item: CreateRange<
//             Item: Debug
//                       + Add<Output = <TIter::Item as CreateRange>::Item>
//                       + SaturatingSub<Output = <TIter::Item as CreateRange>::Item>
//                       + Copy,
//         >,
//     >,
// {
//     parent: SanitizeSortedDisjoint<DilateXIter<TIter>>,
// }
pub struct DilateIter<'a, T: CreateRange<Item: Integer>> {
    parent: DynSortedDisjoint<'a, T::Item>,
    bounds: Rect<u32>,
}
impl<'a, TItem> DilateIter<'a, TItem>
where
    TItem: 'static
        + CreateRange<
            Item: SignedNonZeroable
                      + Debug
                      + Add<Output = TItem::Item>
                      + SaturatingSub<Output = TItem::Item>
                      + CheckedSub<Output = TItem::Item>
                      + Copy
                      + Integer
                      + Zero
                      + One,
        >,
    u32: UncheckedCast<TItem::Item>,
{
    pub fn new<TIter>(
        iter: TIter,
        offset: <<TIter::Item as CreateRange>::Item as SignedNonZeroable>::NonZero,
    ) -> Self
    where
        TIter: 'a + FusedIterator<Item = TItem> + Clone + ImageDimension,
        SanitizeSortedDisjoint<DilateXIter<TIter>>: Iterator<Item = TIter::Item>,
    {
        let width = iter.width().get();
        let create_inner = |offset| {
            SanitizeSortedDisjoint::new(DilateXIter {
                offset,
                parent: iter.clone(),
            })
        };

        let mut before = <TIter::Item as CreateRange>::Item::one()
            .iter_steps(offset)
            .map(|o| {
                let one = <TIter::Item as CreateRange>::Item::one();
                let o_start = o * width.cast_unchecked();
                let o_end = o * width.cast_unchecked() + one;
                CheckSortedDisjoint::new(create_inner(offset.into()).filter_map(move |r| {
                    let end = r.end().checked_sub(&o_end)?;
                    let start = r.start().saturating_sub(&o_start);
                    Some(start..=end)
                }))
            });
        let after = <TIter::Item as CreateRange>::Item::one()
            .iter_steps(offset)
            .map(|o| {
                let one = <TIter::Item as CreateRange>::Item::one();
                let o_start = o * width.cast_unchecked();
                let o_end = o_start - one;
                CheckSortedDisjoint::new(create_inner(offset.into()).map(move |r| {
                    let start = r.start() + o_start;
                    let end = r.end() + o_end;
                    start..=end
                }))
            });
        let original = CheckSortedDisjoint::new(iter.clone().map(|r| {
            let one = <TIter::Item as CreateRange>::Item::one();
            let start = r.start();
            let end = r.end() - one;
            start..=end
        }));

        let first = before
            .next()
            .expect("Always ads at least one per direction");
        let acc = before.fold(DynSortedDisjoint::new(first), |acc, n| {
            DynSortedDisjoint::new(acc.union(n))
        });
        let acc = after.fold(acc, |acc, n| DynSortedDisjoint::new(acc.union(n)));
        let acc: DynSortedDisjoint<'a, <<TIter as Iterator>::Item as CreateRange>::Item> =
            DynSortedDisjoint::new(acc.union(original));

        Self {
            parent: acc,
            bounds: iter.bounds(),
        }
    }
}

impl<'a, TRange> Iterator for DilateIter<'a, TRange>
where
    TRange: 'static
        + CreateRange<
            Item: Add<Output = TRange::Item>
                      + SaturatingSub<Output = TRange::Item>
                      + Copy
                      + Debug
                      + Integer
                      + One,
        >,
{
    type Item = TRange;

    fn next(&mut self) -> Option<Self::Item> {
        let x = self.parent.next()?;
        let start: TRange::Item = *x.start();
        let end: TRange::Item = *x.end() + TRange::Item::one();

        Some(TRange::new_debug_checked_zeroable(start, end))
    }
}

impl<'a, T: CreateRange<Item: range_set_blaze_0_5::Integer>> ImageDimension for DilateIter<'a, T> {
    fn bounds(&self) -> Rect<u32> {
        self.bounds
    }

    fn width(&self) -> NonZeroU32 {
        self.bounds.width
    }
}

pub struct DilateXIter<TIter: Iterator<Item: CreateRange>> {
    parent: TIter,
    offset: <TIter::Item as CreateRange>::Item,
}

impl<TIter> Iterator for DilateXIter<TIter>
where
    TIter: Iterator<
        Item: CreateRange<
            Item: Add<Output = <TIter::Item as CreateRange>::Item>
                      + SaturatingSub<Output = <TIter::Item as CreateRange>::Item>
                      + Copy,
        >,
    >,
{
    type Item = TIter::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.parent.next()?;
        let start = item.start();
        let end = item.end();

        Some(TIter::Item::new_debug_checked_zeroable(
            start.saturating_sub(&self.offset),
            end + self.offset,
        ))
    }
}

impl<TIter> FusedIterator for DilateXIter<TIter>
where
    TIter: FusedIterator<Item: CreateRange>,
    DilateXIter<TIter>: Iterator,
{
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::{ImaskSet, Rect};

    const NONZERO_80: NonZeroU32 = NonZeroU32::new(80).unwrap();

    #[test]
    fn dilate_2x() {
        let top = 5u32 * 80 + 50..5 * 80 + 52;
        let bottom = 6 * 80 + 50..6 * 80 + 52;
        let data = [top, bottom].with_roi(Rect::new(0, 10, NONZERO_80, NONZERO_80));
        let data_dilate = data
            .dilate_range(const { NonZeroU32::new(2).unwrap() })
            .collect::<Vec<_>>();
        let expected = (0..6)
            .map(|offset| (3 + offset) * 80 + 48..(3 + offset) * 80 + 54)
            .collect::<Vec<_>>();
        assert_eq!(data_dilate, expected);
    }
}
