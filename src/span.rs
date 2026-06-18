use std::{
    fmt::Debug,
    ops::{Add, Div, Mul, Rem, Sub},
};

use crate::{CreateRange, ImageDimension, NonZeroRange, Rect, UncheckedCast};

mod affine_transform;
mod clip;
mod dilate;
mod from_bitmap;
mod from_bitmap_range;
mod intersect;
mod into_ranges;
pub(crate) mod peekable;
mod rect;
mod subtract;
mod union;
mod union_all;

pub use affine_transform::*;
pub use clip::*;
pub use dilate::*;
pub use from_bitmap::*;
pub use from_bitmap_range::*;
pub use intersect::*;
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

pub struct SortedRangesSpanIter<TParent>
where
    TParent: Iterator<Item: CreateRange>,
{
    parent: TParent,
    pending: Option<NonZeroRange<<TParent::Item as CreateRange>::Item>>,
}

impl<TParent> Clone for SortedRangesSpanIter<TParent>
where
    TParent: Iterator<Item: CreateRange> + Clone,
    <TParent::Item as CreateRange>::Item: Clone,
{
    fn clone(&self) -> Self {
        Self {
            parent: self.parent.clone(),
            pending: self.pending.clone(),
        }
    }
}

impl<TParent: Iterator<Item: CreateRange> + ImageDimension> SortedRangesSpanIter<TParent> {
    pub fn new(parent: TParent) -> Self {
        Self {
            parent,
            pending: None,
        }
    }
}

impl<TParent: Iterator<Item: CreateRange> + ImageDimension> ImageDimension
    for SortedRangesSpanIter<TParent>
where
    u32: TryInto<<TParent::Item as CreateRange>::Item, Error: Debug>,
{
    fn bounds(&self) -> Rect<u32> {
        let bounds = self.parent.bounds();
        #[cfg(debug_assertions)]
        if let Err(e) = (bounds.width.get() * bounds.height.get()).try_into() {
            panic!(
                "{}*{} overflows {}: {e:?}",
                bounds.width,
                bounds.height,
                std::any::type_name::<<TParent::Item as CreateRange>::Item>()
            );
        }
        bounds
    }

    fn width(&self) -> std::num::NonZero<u32> {
        let width = self.parent.width();
        #[cfg(debug_assertions)]
        if let Err(e) = (width.get() * width.get()).try_into() {
            panic!(
                "{width}^2 overflows {}: {e:?}",
                std::any::type_name::<<TParent::Item as CreateRange>::Item>()
            )
        };

        width
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
                          + Sub<Output = <TParent::Item as CreateRange>::Item>
                          + Rem<Output = <TParent::Item as CreateRange>::Item>
                          + Ord
                          + Debug,
            >,
        > + ImageDimension,
    u32: UncheckedCast<<TParent::Item as CreateRange>::Item>,
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
        let local_y = start / width;
        let row_start = local_y * width;
        let cut = row_start + width;
        let bounds = self.parent.bounds();
        let offset_x = bounds.x.cast_unchecked();
        let offset_y = bounds.y.cast_unchecked();
        let global_y = local_y + offset_y;
        let x = if let Ok(rest) = NonZeroRange::try_from(cut..end) {
            self.pending = Some(rest);
            NonZeroRange::new_debug_checked_zeroable(start - row_start + offset_x, width + offset_x)
        } else {
            NonZeroRange::new_debug_checked_zeroable(
                start - row_start + offset_x,
                end - row_start + offset_x,
            )
        };
        Some(Span { x, y: global_y })
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
            vec!(Span::new(0..10, 0), Span::new(1..10, 1)),
            span.collect::<Vec<_>>()
        )
    }
    #[test]
    fn test_cut() {
        let iter = [0u32..20].with_bounds(NONZERO_10, NONZERO_10);
        let span = SortedRangesSpanIter::new(iter);
        assert_eq!(
            vec!(Span::new(0..10, 0), Span::new(0..10, 1)),
            span.collect::<Vec<_>>()
        );
    }
}
