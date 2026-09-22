//! Span builders for [`SortedRanges`](crate::set::SortedRanges): construction
//! from a stream of [`Span`]s, merging touching spans in a single pass.

pub(crate) mod roi_hint;
pub(crate) mod tight;

pub use roi_hint::SortedRangesSpanBuilder;
pub use tight::SortedRangesTightSpanBuilder;
