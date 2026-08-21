use std::collections::VecDeque;
use std::num::IntErrorKind;
use std::ops::{Add, Range};
use std::{fmt::Debug, num::NonZeroU32};

use num_traits::{One, SaturatingSub, Zero};

use crate::{
    CheckedAddSigned, CreateRange, ImageDimension, ImaskSet, IncompatibleSizeError, NonZeroRange,
    PipelineError, Rect, SignedNonZeroable, Span, UncheckedCast,
};

use super::union_all::UnionAll;

pub struct DilateSpanIter<I, T>
where
    I: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug + Add<Output = T> + CheckedAddSigned,
{
    inner: UnionAll<ShiftedSpanIter<I, T>>,
    offset: T,
    bounds: Rect<u32>,
}

impl<I, T> DilateSpanIter<I, T>
where
    I: Iterator<Item = Span<T>> + Clone + ImageDimension,
    T: Ord
        + Copy
        + Debug
        + Add<Output = T>
        + SaturatingSub<Output = T>
        + CheckedAddSigned
        + One
        + Zero
        + SignedNonZeroable
        + UncheckedCast<u32>,
{
    pub fn new(iter: I, offset: T::NonZero) -> Result<Self, PipelineError> {
        let bounds = iter.bounds();
        let x_offset: T = offset.into();
        let y_offset: T = offset.into();
        let mut iters: Vec<ShiftedSpanIter<I, T>> = Vec::new();

        for y_delta in T::one().iter_steps(offset) {
            iters.push(ShiftedSpanIter {
                parent: iter.clone(),
                x_offset,
                y_shift_unsigned: y_offset.saturating_sub(&y_delta),
            });
        }

        iters.push(ShiftedSpanIter {
            parent: iter.clone(),
            x_offset,
            y_shift_unsigned: y_offset,
        });

        for y_delta in T::one().iter_steps(offset) {
            iters.push(ShiftedSpanIter {
                parent: iter.clone(),
                x_offset,
                y_shift_unsigned: y_offset + y_delta,
            });
        }
        let (x, width) = calculate_bound_dim(bounds.x, bounds.width, x_offset.cast_unchecked())?;
        let (y, height) = calculate_bound_dim(bounds.y, bounds.height, y_offset.cast_unchecked())?;

        Ok(Self {
            inner: UnionAll::new(iters.with_roi(Rect::new(x, y, width, height)))?,
            offset: y_offset,
            bounds: Rect::new(x, y, width, height),
        })
    }
}

/// Dilated `(start, len)` dimension: `[max(0, start - offset), start + len + offset)`.
///
/// Left dilation saturates at 0; all arithmetic happens widened in `u64`, so it cannot
/// overflow. Only the (tight) result must still fit `u32`.
fn calculate_bound_dim(
    start: u32,
    len: NonZeroU32,
    offset: u32,
) -> Result<(u32, NonZeroU32), IncompatibleSizeError> {
    let (start, end) = (
        u64::from(start),
        u64::from(start) + u64::from(len.get()) + u64::from(offset),
    );
    let start = start.saturating_sub(u64::from(offset));
    let width = u32::try_from(end - start).map_err(|_| IntErrorKind::PosOverflow)?;
    Ok((
        start as u32,
        NonZeroU32::new(width).ok_or(IntErrorKind::PosOverflow)?,
    ))
}

impl<I, T> Iterator for DilateSpanIter<I, T>
where
    I: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug + Add<Output = T> + SaturatingSub<Output = T> + CheckedAddSigned,
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Span<T>> {
        loop {
            let span = self.inner.next()?;
            let y = match span.y.checked_add_signed(-T::into_signed(self.offset)) {
                Some(y) => y,
                None => continue,
            };
            return Some(Span {
                x: NonZeroRange::new_debug_checked_zeroable(
                    span.x.start.saturating_sub(&self.offset),
                    span.x.end.saturating_sub(&self.offset),
                ),
                y,
            });
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lo, hi) = self.inner.size_hint();
        (lo, hi)
    }
}

impl<I, T> ImageDimension for DilateSpanIter<I, T>
where
    I: Iterator<Item = Span<T>> + ImageDimension,
    T: Ord + Copy + Debug + Add<Output = T> + SaturatingSub<Output = T> + CheckedAddSigned,
{
    fn bounds(&self) -> Rect<u32> {
        self.bounds
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.bounds.width
    }
}

struct ShiftedSpanIter<I, T> {
    parent: I,
    x_offset: T,
    y_shift_unsigned: T,
}

// This is not a correct implementation!! This is expected to vanish soon
impl<I, T> ImageDimension for ShiftedSpanIter<I, T>
where
    I: ImageDimension + Iterator<Item = Span<T>>,
    T: Copy + Add<Output = T> + UncheckedCast<u32> + SignedNonZeroable,
{
    fn bounds(&self) -> Rect<u32> {
        let parent_bounds = self.parent.bounds();
        let x_offset = self.x_offset.cast_unchecked();
        let y_shift = self.y_shift_unsigned.cast_unchecked();

        let x = parent_bounds.x.saturating_sub(x_offset);
        let y = parent_bounds.y.saturating_sub(y_shift);
        let width = u32::create_non_zero(parent_bounds.width.get() + 2 * x_offset)
            .expect("dilated width is always non-zero");
        let height = parent_bounds.height;

        Rect::new(x, y, width, height)
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.bounds().width
    }
}

impl<I, T> Iterator for ShiftedSpanIter<I, T>
where
    I: Iterator<Item = Span<T>>,
    T: Ord + Copy + Debug + Add<Output = T>,
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Span<T>> {
        let span = self.parent.next()?;
        Some(Span {
            x: NonZeroRange::new_debug_checked_zeroable(
                span.x.start,
                span.x.end + self.x_offset + self.x_offset,
            ),
            y: span.y + self.y_shift_unsigned,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.parent.size_hint()
    }
}

/// Maps input span x-ranges to the coverage-index ranges of [`DilateSpanIterAcc`].
///
/// A strategy encapsulates how dilation transforms x coordinates: [`Self::check`] verifies
/// upfront — based on `parent.bounds()` — that every index it produces is representable in
/// `T`, so [`Self::apply`] can use plain, unchecked arithmetic.
pub trait DilateStrategy<T> {
    /// Returns an error if the coordinates this strategy produces for spans within `bounds`
    /// (which the parent iterator guarantees) are not representable in `T`.
    fn check(&self, bounds: Rect<u32>) -> Result<(), IncompatibleSizeError>;
    /// Maps the x-range of an input span to its dilated counterpart in the coverage (and
    /// output) coordinate space of [`DilateSpanIterAcc`].
    fn apply(&self, range: Range<T>) -> Range<T>;
}

/// [`DilateStrategy`] keeping original (in-place) x coordinates — behaves like plain
/// dilation: `start` saturates at 0, `end` grows by `radius`.
pub struct DilateInPlace<T> {
    radius: T,
}

impl<T> DilateInPlace<T> {
    pub fn new(radius: T) -> Self {
        Self { radius }
    }
}

impl<T> DilateStrategy<T> for DilateInPlace<T>
where
    T: Copy + Add<Output = T> + SaturatingSub<Output = T> + UncheckedCast<u32>,
    T: TryFrom<u64, Error: Into<IncompatibleSizeError>>,
{
    fn check(&self, bounds: Rect<u32>) -> Result<(), IncompatibleSizeError> {
        // The largest coordinate is the dilated end `bounds.x + bounds.width + radius`
        // (`end + radius` of any in-bounds span never exceeds `bounds.end + radius`).
        ensure_representable::<T>(
            u64::from(bounds.x)
                + u64::from(bounds.width.get())
                + u64::from(UncheckedCast::<u32>::cast_unchecked(self.radius)),
        )
    }

    #[inline]
    fn apply(&self, range: Range<T>) -> Range<T> {
        range.start.saturating_sub(&self.radius)..range.end + self.radius
    }
}

/// [`DilateStrategy`] adding `2 * radius` to the end: the dilated ranges are translated by
/// `radius` in x direction, so `start` needs no underflow handling.
///
/// The output spans (and [`ImageDimension::bounds`]) of the iterator are translated by
/// `+radius` compared to [`DilateInPlace`]. It only checks that the translated end
/// `new_bounds.width + new_bounds.x` (= `bounds.x + bounds.width + 2 * radius`) is
/// representable in `T`.
pub struct DilateTranslated<T> {
    radius: T,
}

impl<T> DilateTranslated<T> {
    pub fn new(radius: T) -> Self {
        Self { radius }
    }
}

impl<T> DilateStrategy<T> for DilateTranslated<T>
where
    T: Copy + Add<Output = T> + UncheckedCast<u32>,
    T: TryFrom<u64, Error: Into<IncompatibleSizeError>>,
{
    fn check(&self, bounds: Rect<u32>) -> Result<(), IncompatibleSizeError> {
        ensure_representable::<T>(
            u64::from(bounds.x)
                + u64::from(bounds.width.get())
                + 2 * u64::from(UncheckedCast::<u32>::cast_unchecked(self.radius)),
        )
    }

    fn apply(&self, range: Range<T>) -> Range<T> {
        range.start..range.end + self.radius + self.radius
    }
}

/// Alternative dilation implementation that avoids cloning the input iterator.
///
/// Instead of unioning `(2 * offset + 1)` shifted copies of the input, this keeps a sliding
/// window of the input spans that can still influence the current output row and maintains a
/// per-column "coverage" accumulator ([`Self::coverage`]). The accumulator counts, for each `x`
/// column, how many active input spans still want that field alive — if the count is `> 0` the
/// column is alive. Spans enter the window when they start affecting the current output row and
/// are removed (decrementing their range in the accumulator) once they fall out of the current
/// window.
///
/// Input spans are pulled lazily from the source iterator and only retained in [`Self::active`]
/// (a `VecDeque`) while they are inside the sliding window — they are never all buffered at
/// once. The spans of the current row are emitted directly from a scan cursor
/// ([`Self::cursor`]) over [`Self::coverage`], so apart from seeding [`Self::active`] and
/// [`Self::coverage`] once in [`Self::new`], iteration performs no allocations.
///
/// How x coordinates are dilated is determined by the [`DilateStrategy`] `S`
/// ([`DilateInPlace`] by default, which keeps original coordinates).
///
/// All arithmetic happens on `T` without checked operations: [`Self::new`] relies on
/// `parent.bounds()` — spans of a parent never leave their [`ImageDimension`] bounds — and on
/// `S::check`/`new` to reject any `bounds`/`offset` combination for which the largest
/// intermediate value is not representable in `T`. This rules out every overflow and
/// out-of-bounds access the iterator could perform.
pub struct DilateSpanIterAcc<I, T, S = DilateInPlace<T>> {
    input: I,
    /// Peeked next input span, pulled lazily from `input`.
    next_input: Option<Span<T>>,
    offset: T,
    strategy: S,
    bounds: Rect<u32>,
    /// Input spans whose Chebyshev distance to the current output row is `<= offset`, in `y` order.
    active: VecDeque<Span<T>>,
    /// Per-column coverage accumulator, indexed by absolute `x`. Entry `> 0` means the column is alive.
    coverage: Vec<u16>,
    /// Start of the alive region within `coverage` for the current row: min over all active
    /// spans of their coverage start. Columns below it are guaranteed dead.
    row_a: usize,
    /// Exclusive end of the alive region within `coverage` for the current row: max over all
    /// active spans of their coverage end. Columns at or above it are guaranteed dead.
    row_b: usize,
    /// Position of the next emitted span within `coverage` for the current output row.
    cursor: usize,
    /// Current output row (already in original coordinate space).
    cur_y: T,
}

impl<I, T> DilateSpanIterAcc<I, T, DilateInPlace<T>>
where
    I: Iterator<Item = Span<T>> + ImageDimension,
    T: Ord
        + Copy
        + Debug
        + Add<Output = T>
        + SaturatingSub<Output = T>
        + One
        + Zero
        + SignedNonZeroable
        + UncheckedCast<u32>
        + UncheckedCast<u64>
        + TryFrom<u64, Error: Into<IncompatibleSizeError>>,
    u32: UncheckedCast<T>,
{
    /// Creates an iterator dilating with the default [`DilateInPlace`] strategy.
    pub fn new(iter: I, offset: T::NonZero) -> Result<Self, PipelineError> {
        Self::with_strategy(iter, offset, DilateInPlace::new(offset.into()))
    }
}

impl<I, T, S> DilateSpanIterAcc<I, T, S>
where
    I: Iterator<Item = Span<T>> + ImageDimension,
    S: DilateStrategy<T>,
    T: Ord
        + Copy
        + Debug
        + Add<Output = T>
        + SaturatingSub<Output = T>
        + One
        + Zero
        + SignedNonZeroable
        + UncheckedCast<u32>
        + UncheckedCast<u64>
        + TryFrom<u64, Error: Into<IncompatibleSizeError>>,
    u32: UncheckedCast<T>,
{
    pub fn with_strategy(iter: I, offset: T::NonZero, strategy: S) -> Result<Self, PipelineError> {
        let orig_bounds = iter.bounds();
        let offset_val: T = offset.into();
        let off_u32: u32 = offset_val.cast_unchecked();

        // A column is covered by at most one span per row of the sliding window (inputs are
        // sorted & disjoint), so coverage counts stay within `2 * offset + 1` rows — which
        // must fit `u16` for the accumulator to stay exact.
        if u64::from(u32::from(u16::MAX)) <= 2 * u64::from(off_u32) + 1 {
            return Err(IntErrorKind::PosOverflow.into());
        }

        // x mapping (coverage length, output bounds and per-span indices) is validated by
        // the strategy. Relying on `parent.bounds()`: every span of the parent iterator
        // stays within `orig_bounds`, so its coordinates never exceed `x_end`/`y_end`.
        strategy.check(orig_bounds)?;
        // The largest y value of the hot path is `y_end + 2 * offset` (the largest
        // `enter_until`); rejecting anything not representable in `T` rules out every
        // overflow below.
        let y_end = u64::from(orig_bounds.y) + u64::from(orig_bounds.height.get());
        ensure_representable::<T>(y_end + 2 * u64::from(off_u32))?;

        let x_start = u64::from(orig_bounds.x);
        let x_end = x_start + u64::from(orig_bounds.width.get());

        let start_t: T =
            <T>::try_from(x_start).map_err(|e| IncompatibleSizeError::from(e.into()))?;
        let bounds_range =
            start_t..<T>::try_from(x_end).map_err(|e| IncompatibleSizeError::from(e.into()))?;
        let out_x = strategy.apply(bounds_range);
        let (cov_start, cov_end) = (
            UncheckedCast::<u64>::cast_unchecked(out_x.start),
            UncheckedCast::<u64>::cast_unchecked(out_x.end),
        );
        let cov_len = usize::try_from(cov_end)?;
        let x = u32::try_from(cov_start).map_err(|_| IntErrorKind::PosOverflow)?;
        let width = u32::try_from(cov_end - cov_start).map_err(|_| IntErrorKind::PosOverflow)?;
        let (y, height) = calculate_bound_dim(orig_bounds.y, orig_bounds.height, off_u32)?;
        let bounds = Rect::new(
            x,
            y,
            NonZeroU32::new(width).ok_or(IntErrorKind::PosOverflow)?,
            height,
        );

        let mut this = Self {
            input: iter,
            next_input: None,
            offset: offset_val,
            strategy,
            bounds,
            active: VecDeque::new(),
            coverage: vec![0u16; cov_len],
            row_a: 0,
            row_b: 0,
            cursor: 0,
            cur_y: T::zero(),
        };
        this.next_input = this.input.next();
        if let Some(first) = this.next_input {
            this.cur_y = first.y.saturating_sub(&offset_val);
        }
        this.load_row();
        this.cursor = this.row_a;
        Ok(this)
    }

    /// Coverage indices `[a, b)` of the x-dilation of `span`.
    ///
    /// `start` can only saturate at 0 when the span touches the left dilation edge; `end`
    /// always stays within `coverage`, because spans never leave their parent bounds (see
    /// [`Self::with_strategy`]).
    fn coverage_range(&self, span: &Span<T>) -> (usize, usize) {
        let range = self.strategy.apply(span.x.start..span.x.end);
        debug_assert!(
            u64::from(UncheckedCast::<u32>::cast_unchecked(range.end))
                <= self.coverage.len() as u64,
            "span {span:?} escapes its ImageDimension bounds"
        );
        (
            UncheckedCast::<u32>::cast_unchecked(range.start) as usize,
            UncheckedCast::<u32>::cast_unchecked(range.end) as usize,
        )
    }

    fn add_range(&mut self, span: &Span<T>) {
        let (a, b) = self.coverage_range(span);
        // Counts provably stay within `2 * offset + 1 <= u16::MAX` (see `new`), so wrapping
        // cannot occur — plain add/sub keeps the loop vectorizable.
        for c in &mut self.coverage[a..b] {
            *c = c.wrapping_add(1);
        }
    }

    fn remove_range(&mut self, span: &Span<T>) {
        let (a, b) = self.coverage_range(span);
        for c in &mut self.coverage[a..b] {
            *c = c.wrapping_sub(1);
        }
    }

    /// Expires spans that no longer influence `cur_y` and enqueues all pending input spans
    /// that start influencing it, keeping `coverage` in sync. Also re-derives the alive
    /// region `[row_a, row_b)`, so that `next` never scans dead padding columns.
    fn load_row(&mut self) {
        let row = self.cur_y;
        while let Some(span) = self
            .active
            .pop_front_if(|front| front.y + self.offset < row)
        {
            self.remove_range(&span);
        }

        let enter_until = row + self.offset;
        loop {
            let span = match self.next_input {
                Some(span) if span.y <= enter_until => span,
                _ => break,
            };
            self.add_range(&span);
            self.active.push_back(span);
            self.next_input = self.input.next();
        }

        let (mut row_a, mut row_b) = (usize::MAX, 0);
        for span in &self.active {
            let (a, b) = self.coverage_range(span);
            row_a = row_a.min(a);
            row_b = row_b.max(b);
        }
        self.row_a = if self.active.is_empty() { 0 } else { row_a };
        self.row_b = row_b;
    }

    /// Advances to the next output row, reloading the sliding window. Returns `false` once
    /// the iteration is done.
    fn advance_row(&mut self) -> bool {
        // `cur_y` never passes the last output row (see `new`), so this cannot overflow.
        let mut next_row = self.cur_y + T::one();
        if self.active.is_empty() {
            // Nothing is alive: jump directly to the first row the next input span
            // influences instead of scanning every empty row in between one by one.
            let Some(next_span) = self.next_input else {
                return false;
            };
            next_row = next_row.max(next_span.y.saturating_sub(&self.offset));
        }
        self.cur_y = next_row;
        self.load_row();
        self.cursor = self.row_a;
        true
    }
}

fn ensure_representable<T>(value: u64) -> Result<(), IncompatibleSizeError>
where
    T: TryFrom<u64, Error: Into<IncompatibleSizeError>>,
{
    T::try_from(value)
        .map(|_| ())
        .map_err(|e| IncompatibleSizeError::from(e.into()))
}

impl<I, T, S> Iterator for DilateSpanIterAcc<I, T, S>
where
    I: Iterator<Item = Span<T>> + ImageDimension,
    S: DilateStrategy<T>,
    T: Ord
        + Copy
        + Debug
        + Add<Output = T>
        + SaturatingSub<Output = T>
        + One
        + Zero
        + SignedNonZeroable
        + UncheckedCast<u32>
        + UncheckedCast<u64>
        + TryFrom<u64, Error: Into<IncompatibleSizeError>>,
    u32: UncheckedCast<T>,
{
    type Item = Span<T>;

    fn next(&mut self) -> Option<Span<T>> {
        loop {
            let rel = self.coverage[self.cursor..self.row_b]
                .iter()
                .position(|c| *c > 0);
            let Some(rel) = rel else {
                if !self.advance_row() {
                    return None;
                }
                continue;
            };
            let start = self.cursor + rel;
            let end = self.coverage[start + 1..self.row_b]
                .iter()
                .position(|c| *c == 0)
                .map_or(self.row_b, |rel| start + 1 + rel);
            self.cursor = end;
            return Some(Span {
                x: NonZeroRange::new_debug_checked_zeroable(
                    (start as u32).cast_unchecked(),
                    (end as u32).cast_unchecked(),
                ),
                y: self.cur_y,
            });
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<I, T> ImageDimension for DilateSpanIterAcc<I, T> {
    fn bounds(&self) -> Rect<u32> {
        self.bounds
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.bounds.width
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZero, NonZeroU8, NonZeroU32};

    use crate::{DilateSpanIterAcc, ImageDimension, ImaskSet, Rect, SortedRanges, Span};

    const W: NonZero<u32> = NonZero::new(100).unwrap();
    const H: NonZero<u32> = NonZero::new(100).unwrap();

    #[test]
    fn dilate_2x() {
        let rect = Rect::new(50u32, 5, NonZero::new(2).unwrap(), NonZero::new(2).unwrap());
        let radius = NonZero::new(2u32).unwrap();
        let result: Vec<_> = rect
            .into_spans()
            .dilate_within(radius, rect.expand(radius.get()))
            .unwrap()
            .collect();

        let expected: Vec<_> = (3..9).map(|y| Span::new(48..54, y)).collect();
        assert_eq!(expected, result);
    }

    #[test]
    fn dilate_1x_single_span() {
        let radius = NonZero::new(1u32).unwrap();
        let roi = Rect::new(0, 0, W, H).expand(radius.get());
        let result: Vec<_> = vec![Span::new(5..10, 3u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate_within(radius, roi)
            .unwrap()
            .collect();

        assert_eq!(
            vec![
                Span::new(4..11, 2u32),
                Span::new(4..11, 3u32),
                Span::new(4..11, 4u32),
            ],
            result
        );
    }

    #[test]
    fn dilate_at_top_edge() {
        let radius = NonZero::new(1u32).unwrap();
        let roi = Rect::new(0, 0, W, H).expand(radius.get());
        let result: Vec<_> = vec![Span::new(5..10, 0u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate_within(radius, roi)
            .unwrap()
            .collect();

        assert_eq!(
            vec![Span::new(4..11, 0u32), Span::new(4..11, 1u32),],
            result
        );
    }

    #[test]
    fn dilate_multiple_spans_same_row() {
        let radius = NonZero::new(1u32).unwrap();
        let roi = Rect::new(0, 0, W, H).expand(radius.get());
        let result: Vec<_> = vec![Span::new(0..3, 5u32), Span::new(7..10, 5u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate_within(radius, roi)
            .unwrap()
            .collect();

        assert_eq!(
            vec![
                Span::new(0..4, 4u32),
                Span::new(6..11, 4u32),
                Span::new(0..4, 5u32),
                Span::new(6..11, 5u32),
                Span::new(0..4, 6u32),
                Span::new(6..11, 6u32),
            ],
            result
        );
    }

    #[test]
    fn dilate_overlapping_rows() {
        let result: Vec<_> = vec![Span::new(5..10, 5u32), Span::new(5..10, 6u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate_within(NonZero::new(1u32).unwrap(), Rect::new(0, 0, W, H).expand(1))
            .unwrap()
            .collect();

        assert_eq!(
            vec![
                Span::new(4..11, 4u32),
                Span::new(4..11, 5u32),
                Span::new(4..11, 6u32),
                Span::new(4..11, 7u32),
            ],
            result
        );
    }

    #[test]
    fn dilate_overflow_skips_spans() {
        let result: Vec<_> = vec![Span::new(5..10, 0u32), Span::new(5..10, 5u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate_within(NonZero::new(3u32).unwrap(), Rect::new(0, 0, W, H).expand(3))
            .unwrap()
            .collect();

        assert_eq!(
            (0..=8).map(|y| Span::new(2..13, y)).collect::<Vec<_>>(),
            result
        );
    }

    #[test]
    fn correct_bounds() {
        const ELEVEN: NonZeroU32 = NonZeroU32::new(11).unwrap();
        const FIVE: NonZeroU8 = NonZeroU8::new(5).unwrap();
        let x = DilateSpanIterAcc::new(
            SortedRanges::from(Span::new(6u8..7, 7)).spans_owned::<u8>(),
            FIVE,
        )
        .unwrap();
        assert_eq!(Rect::new(1, 2, ELEVEN, ELEVEN), x.bounds());
    }

    // --- Equivalence tests between the union-based and accumulator-based dilation ---

    #[allow(deprecated)]
    fn run_both(spans: Vec<Span<u32>>, w: u32, h: u32, offset: u32) {
        let (w_nz, h_nz) = (NonZero::new(w).unwrap(), NonZero::new(h).unwrap());
        let offset = NonZero::new(offset).unwrap();
        // Both implementations require the input to be sorted by (y, x). The accumulator
        // uses the in-place strategy and must see the same extended input bounds that
        // `dilate` declares via `source.bounds().expand(radius)`.
        let roi = Rect::new(0, 0, w_nz, h_nz).expand(offset.get());
        let dilate = spans
            .iter()
            .copied()
            .with_bounds(w_nz, h_nz)
            .dilate(offset)
            .unwrap();
        let dilate_acc = DilateSpanIterAcc::new(
            spans.iter().copied().with_bounds(w_nz, h_nz).with_roi(roi),
            offset,
        )
        .unwrap();
        assert_eq!(dilate.bounds(), dilate_acc.bounds());
        let acc = SortedRanges::<u64>::try_from_span_iter(dilate_acc)
            .expect("Ranges are valid to be collected into SortedRanges")
            .spans_owned::<u32>()
            .collect::<Vec<_>>();
        let u = dilate.collect::<Vec<_>>();
        assert_eq!(u, acc, "mismatch for w={w} h={h} offset={offset}");
    }

    #[test]
    fn accumulator_matches_union_single_span() {
        run_both(vec![Span::new(5..10, 5u32)], 100, 100, 3);
    }

    #[test]
    fn accumulator_matches_union_edge() {
        run_both(vec![Span::new(0..3, 0u32)], 100, 100, 2);
    }

    #[test]
    fn accumulator_matches_union_disjoint_rows() {
        let spans = vec![Span::new(5..10, 0u32), Span::new(40..45, 50u32)];
        run_both(spans, 100, 100, 2);
    }

    #[test]
    fn accumulator_matches_union_overlapping() {
        let spans = vec![
            Span::new(0..4, 4u32),
            Span::new(5..10, 5u32),
            Span::new(8..13, 6u32),
        ];
        run_both(spans, 100, 100, 4);
    }

    #[test]
    fn don_t_connect_far_appart() {
        let spans = vec![Span::new(5..6, 0u32), Span::new(5..6, 6u32)];
        run_both(spans, 100, 100, 2);
    }
}
