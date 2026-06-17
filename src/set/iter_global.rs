use std::{
    cmp::min,
    iter::FusedIterator,
    num::NonZeroU32,
    ops::{Add, Div, Mul, Rem, Sub},
};

use crate::{CreateRange, ImageDimension, Rect, SignedNonZeroable, UncheckedCast};

pub struct SortedRangesIterGlobal<I, E, T: CreateRange> {
    included: I,
    excluded: E,
    pos: T::Item,
    remaining: T::Item,
    old_width: T::Item,
    new_width_out: T::Item,
    new_width: NonZeroU32,
    offset_x: T::Item,
    offset_y: T::Item,
    /// Buffered output range end for the shrinking path (`old_width >= new_width`).
    /// When non-zero, a range starting at `pending_start` is being accumulated;
    /// it is flushed when the next segment is not adjacent.
    pending_start: T::Item,
    pending_end: T::Item,
    new_height: NonZeroU32,
}

impl<I, E, T: CreateRange> SortedRangesIterGlobal<I, E, T>
where
    u32: UncheckedCast<T::Item>,
    T::Item: Copy + Default,
{
    pub(crate) fn new(
        included: I,
        excluded: E,
        old_width: NonZeroU32,
        new_width: NonZeroU32,
        new_height: NonZeroU32,
        offset_x: T::Item,
        offset_y: T::Item,
    ) -> Self {
        Self {
            included,
            excluded,
            pos: T::Item::default(),
            remaining: T::Item::default(),
            old_width: old_width.get().cast_unchecked(),
            new_width,
            new_height,
            new_width_out: new_width.get().cast_unchecked(),
            offset_x,
            offset_y,
            pending_start: T::Item::default(),
            pending_end: T::Item::default(),
        }
    }
}

impl<I, E, T: CreateRange> ImageDimension for SortedRangesIterGlobal<I, E, T> {
    fn width(&self) -> std::num::NonZero<u32> {
        self.new_width
    }

    fn bounds(&self) -> crate::Rect<u32> {
        Rect {
            x: 0,
            y: 0,
            width: self.new_width,
            height: self.new_height,
        }
    }
}

impl<TI, TE, TOut> Iterator for SortedRangesIterGlobal<TI, TE, TOut>
where
    TI: Iterator<Item: UncheckedCast<TOut::Item>>,
    TE: Iterator<Item: UncheckedCast<TOut::Item>>,
    TOut: CreateRange<
        Item: Copy
                  + Default
                  + SignedNonZeroable
                  + Add<Output = TOut::Item>
                  + Sub<Output = TOut::Item>
                  + Mul<Output = TOut::Item>
                  + Div<Output = TOut::Item>
                  + Rem<Output = TOut::Item>
                  + Ord,
    >,
{
    type Item = TOut;

    fn next(&mut self) -> Option<Self::Item> {
        let zero = TOut::Item::default();
        if self.remaining > zero {
            let col_local = self.pos % self.old_width;
            let row_local = self.pos / self.old_width;
            let take = min(self.remaining, self.old_width - col_local);
            self.pos = self.pos + take;
            self.remaining = self.remaining - take;
            let col = self.offset_x + col_local;
            let row = self.offset_y + row_local;
            let s = row * self.new_width_out + col;
            return Some(TOut::new_debug_checked_zeroable(s, s + take));
        }

        while let Some(gap) = self.excluded.next() {
            self.pos = self.pos + gap.cast_unchecked();
            let include: TOut::Item = self.included.next()?.cast_unchecked();

            if self.old_width < self.new_width_out {
                // Expanding: pieces have gaps in output space, no merging needed
                self.remaining = include;
                return self.next();
            }

            // Shrinking: remap through row/col, clamping columns to new_width.
            // Merge adjacent output segments at row boundaries.
            let end = self.pos + include;
            let remap = |p: TOut::Item| {
                let local_col = p % self.old_width;
                let local_row = p / self.old_width;
                let col = self.offset_x + local_col;
                let row = self.offset_y + local_row;
                row * self.new_width_out + min(col, self.new_width_out)
            };
            let (s, e) = (remap(self.pos), remap(end));
            self.pos = end;

            if self.pending_end == s {
                self.pending_end = e;
            } else if s < e {
                let prev_start = std::mem::replace(&mut self.pending_start, s);
                let prev_end = std::mem::replace(&mut self.pending_end, e);
                if prev_start < prev_end {
                    return Some(TOut::new_debug_checked_zeroable(prev_start, prev_end));
                }
            }
        }
        // Flush pending range from shrinking path
        (self.pending_start < self.pending_end).then(|| {
            let r = TOut::new_debug_checked_zeroable(self.pending_start, self.pending_end);
            self.pending_start = self.pending_end;
            r
        })
    }
}

impl<TI, TE, TOut: CreateRange> FusedIterator for SortedRangesIterGlobal<TI, TE, TOut> where
    Self: Iterator
{
}

#[cfg(feature = "range-set-blaze-0_5")]
mod range_set_blaze_0_5_interop {
    use super::*;
    use range_set_blaze_0_5::{Integer, SortedDisjoint, SortedStarts};
    use std::ops::RangeInclusive;

    impl<TI, TE, T> SortedStarts<T> for SortedRangesIterGlobal<TI, TE, RangeInclusive<T>>
    where
        TI: FusedIterator<Item: UncheckedCast<T>>,
        TE: Iterator<Item: UncheckedCast<T>>,
        T: Copy
            + Default
            + SignedNonZeroable
            + std::ops::Add<Output = T>
            + std::ops::Sub<Output = T>
            + std::ops::Mul<Output = T>
            + std::ops::Div<Output = T>
            + std::ops::Rem<Output = T>
            + Ord
            + num_traits::One
            + Integer,
    {
    }
    impl<TI, TE, T> SortedDisjoint<T> for SortedRangesIterGlobal<TI, TE, RangeInclusive<T>>
    where
        SortedRangesIterGlobal<TI, TE, RangeInclusive<T>>: SortedStarts<T>,
        T: Copy
            + Default
            + SignedNonZeroable
            + std::ops::Add<Output = T>
            + std::ops::Sub<Output = T>
            + std::ops::Mul<Output = T>
            + std::ops::Div<Output = T>
            + std::ops::Rem<Output = T>
            + Ord
            + num_traits::One
            + Integer,
    {
    }
}
