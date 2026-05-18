use std::{
    fmt::Debug,
    ops::{Add, Div, Mul, Rem},
};

use crate::{CreateRange, ImageDimension, NonZeroRange, UncheckedCast};

mod clip;
mod dilate;
mod into_ranges;
pub(crate) mod peekable;
mod rect;
mod subtract;
mod union;
mod union_all;

pub use clip::*;
pub use dilate::*;
pub use into_ranges::*;
pub use rect::*;
pub use subtract::*;
pub use union::*;
pub use union_all::*;

pub trait IntoSpanIter<T> {
    type Item;
    type IntoIter: Iterator<Item = NonZeroRange<Self::Item>> + ImageDimension;

    fn into_span_iter(self) -> Self::IntoIter;
}

/// x_end is exclusive
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Ord, Eq)]
pub struct Span<T> {
    pub y: T,
    pub x: NonZeroRange<T>,
}

impl<T: Debug + Ord + Copy> Span<T> {
    pub fn new(x: impl CreateRange<Item = T>, y: T) -> Self {
        let x = NonZeroRange::new_debug_checked_zeroable(x.start(), x.end());
        Self { x, y }
    }
}

struct SortedRangesSpanIter<TParent>
where
    TParent: Iterator<Item: CreateRange>,
{
    parent: TParent,
    pending: Option<NonZeroRange<<TParent::Item as CreateRange>::Item>>,
}

impl<TParent: Iterator<Item: CreateRange>> SortedRangesSpanIter<TParent> {
    fn new(parent: TParent) -> Self {
        Self {
            parent,
            pending: None,
        }
    }
}

impl<TParent> Iterator for SortedRangesSpanIter<TParent>
where
    TParent: Iterator<
            Item: CreateRange<
                Item: Copy
                          + Div<Output = <TParent::Item as CreateRange>::Item>
                          + Mul<Output = <TParent::Item as CreateRange>::Item>
                          + Add<Output = <TParent::Item as CreateRange>::Item>
                          + Rem<Output = <TParent::Item as CreateRange>::Item>
                          + Ord
                          + Debug,
            >,
        > + ImageDimension,
    u32: UncheckedCast<<TParent::Item as CreateRange>::Item>,
    //NonZeroRange<<TParent::Item as CreateRange>::Item>: CreateRange,
{
    type Item = Span<<TParent::Item as CreateRange>::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        let range = self.pending.take().or_else(|| {
            self.parent
                .next()
                .map(|x| NonZeroRange::new_debug_checked_zeroable(x.start(), x.end()))
        })?;
        let start = range.start();
        let end = range.end();
        let width = self.parent.width().get().cast_unchecked();
        let y = start / width;
        let cut = y * width + width;
        let x = if let Ok(rest) = NonZeroRange::try_from(cut..end) {
            self.pending = Some(rest);
            NonZeroRange::new_debug_checked_zeroable(start, cut)
        } else {
            NonZeroRange::new_debug_checked_zeroable(start, end)
        };
        Some(Span { x, y })
    }
}

// impl<T> IntoSpanIter<T> for SortedRanges<T, T> {
//     type Item = T;

//     type IntoIter = ;

//     fn into_span_iter(self) {
//         self.iter_roi_owned()
//         todo!()
//     }
// }
#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::ImaskSet;

    use super::*;

    const NONZERO_10: NonZeroU32 = NonZeroU32::new(10).unwrap();

    #[test]
    fn test_nocut() {
        let iter = [0u32..10, 11..20].with_bounds(NONZERO_10, NONZERO_10);
        let span = SortedRangesSpanIter::new(iter);
        assert_eq!(
            vec!(Span::new(0..10, 0), Span::new(11..20, 1)),
            span.collect::<Vec<_>>()
        );
    }
    #[test]
    fn test_cut() {
        let iter = [0u32..20].with_bounds(NONZERO_10, NONZERO_10);
        let span = SortedRangesSpanIter::new(iter);
        assert_eq!(
            vec!(Span::new(0..10, 0), Span::new(10..20, 1)),
            span.collect::<Vec<_>>()
        );
    }
}
