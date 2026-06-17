#![doc = include_str!("../README.md")]

///
/// Working with ranges or collections/iterators of ranges
///
mod assert_sorted_iter;
mod checked_add_signed;
mod create_range;
#[cfg(feature = "async-io")]
mod io;
mod map;
mod non_zero;
mod rect;
mod set;
mod span;
mod unchecked_cast;
mod with_bounds;
mod with_roi;

use std::num::NonZero;

pub use assert_sorted_iter::*;
pub(crate) use checked_add_signed::CheckedAddSigned;
pub use create_range::*;
#[cfg(feature = "async-io")]
pub use io::*;
pub use map::*;
pub use non_zero::*;
pub use rect::*;
pub use set::*;
pub use span::*;
pub use unchecked_cast::*;
pub use with_bounds::*;
pub use with_roi::*;

#[derive(Debug, Eq, PartialEq)]
pub struct OrderedRangeItem<TMeta> {
    pub range: NonZeroRange<u32>,
    pub meta: TMeta,
    pub priority: u32,
}

impl<TMeta> OrderedRangeItem<TMeta> {
    pub fn comparator(&self) -> (u32, u32) {
        (self.range.start, u32::MAX - self.priority)
    }
}

pub trait ImageDimension {
    fn bounds(&self) -> Rect<u32>;
    fn width(&self) -> NonZero<u32>;
}
