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
/// A strategy encapsulates how dilation transforms x coordinates — including how to deal
/// with input spans outside the input bounds `outer`: [`Self::compute_bounds`] derives the
/// coverage bounds from `outer` and rejects any combination whose dilated coordinates
/// don't fit `u32`, so [`Self::apply`] can use plain, unchecked arithmetic.
pub trait DilateStrategy<T> {
    /// Computes the bounds the dilated spans of inputs within `outer` occupy — `outer`
    /// dilated by the radius — or an error if they don't fit `u32`.
    fn compute_bounds(&self, outer: Rect<u32>) -> Result<Rect<u32>, PipelineError>;
    /// Maps an input span — in original coordinates, possibly outside the input bounds —
    /// to its dilated counterpart in absolute coordinates, or `None` if the span
    /// contributes nothing.
    ///
    /// Handling of out-of-bounds spans is the strategy's decision: [`DilateInPlace`]
    /// (constructed with the input bounds' x-range) clips and returns `None` for spans
    /// entirely outside, while [`DilateTranslated`] stays branch- and check-free by not
    /// clipping at all — it requires input spans within the input bounds. The returned
    /// `y` steers the sliding window (the span influences output rows
    /// `[y - offset, y + offset]`): [`DilateInPlace`] keeps it unchanged,
    /// [`DilateTranslated`] shifts it by `+radius`.
    fn apply(&self, span: Span<T>) -> Option<Span<T>>;
}

/// [`DilateStrategy`] keeping original (in-place) x coordinates — behaves like plain
/// dilation: `start` saturates at 0, `end` grows by `radius`.
///
/// [`DilateStrategy::apply`] clips input spans to `outer_x` (the x-range of the input
/// bounds, taken as constructor argument): spans (partially) outside contribute their
/// overlap, spans entirely outside become `None`.
pub struct DilateInPlace<T> {
    radius: T,
    /// X-range of the input bounds input spans are clipped to.
    outer_x: Range<T>,
}

impl<T> DilateInPlace<T> {
    /// `outer_x`: x-range of the input bounds (e.g. of `iter.bounds()`), input spans are
    /// clipped to.
    pub fn new(radius: T, outer_x: Range<T>) -> Self {
        Self { radius, outer_x }
    }
}

impl<T> DilateStrategy<T> for DilateInPlace<T>
where
    T: Ord + Copy + Debug + Add<Output = T> + SaturatingSub<Output = T> + UncheckedCast<u32>,
    T: TryFrom<u64, Error: Into<IncompatibleSizeError>>,
{
    fn compute_bounds(&self, outer: Rect<u32>) -> Result<Rect<u32>, PipelineError> {
        let radius = UncheckedCast::<u32>::cast_unchecked(self.radius);
        let (x, width) = calculate_bound_dim(outer.x, outer.width, radius)?;
        let (y, height) = calculate_bound_dim(outer.y, outer.height, radius)?;
        Ok(Rect::new(x, y, width, height))
    }

    #[inline]
    fn apply(&self, span: Span<T>) -> Option<Span<T>> {
        // Clip to `outer_x` first — dilation of nothing is nothing — then dilate.
        let start = span.x.start.max(self.outer_x.start);
        let end = span.x.end.min(self.outer_x.end);
        if start >= end {
            return None; // entirely outside the input bounds
        }
        Some(Span {
            x: NonZeroRange::new_unchecked(start.saturating_sub(&self.radius)..end + self.radius),
            y: span.y,
        })
    }
}

/// [`DilateStrategy`] translating the dilated spans by `+radius` — in x and y alike —
/// so `start` needs no underflow handling and nothing saturates at the top/left edges.
///
/// An input span `[start, end)` at row `y` maps to `[start, end + 2 * radius]` influencing
/// output rows `[y, y + 2 * radius]`: a single input row becomes `2 * radius + 1` rows.
/// Because nothing is clipped or saturated, a subsequent erosion can subtract `radius`
/// again without leaving the bounds — unlike [`DilateInPlace`], which loses the top/left
/// dilation at the image edges. To stay free of any clipping overhead,
/// [`DilateStrategy::apply`] does not clip: input spans must lie within the input bounds
/// (violations only surface as debug-mode overflow panics).
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
    T: Ord + Copy + Debug + Add<Output = T> + UncheckedCast<u32>,
    T: TryFrom<u64, Error: Into<IncompatibleSizeError>>,
{
    fn compute_bounds(&self, outer: Rect<u32>) -> Result<Rect<u32>, PipelineError> {
        let radius = UncheckedCast::<u32>::cast_unchecked(self.radius);
        // apply keeps `start` as-is and only grows the end by `2 * radius` — in x and y
        // alike. All arithmetic happens widened in `u64`.
        let x_end = u64::from(outer.x) + u64::from(outer.width.get()) + 2 * u64::from(radius);
        let y_end = u64::from(outer.y) + u64::from(outer.height.get()) + 2 * u64::from(radius);
        let width =
            u32::try_from(x_end - u64::from(outer.x)).map_err(|_| IntErrorKind::PosOverflow)?;
        let height =
            u32::try_from(y_end - u64::from(outer.y)).map_err(|_| IntErrorKind::PosOverflow)?;
        Ok(Rect::new(
            outer.x,
            outer.y,
            NonZeroU32::new(width).ok_or(IntErrorKind::PosOverflow)?,
            NonZeroU32::new(height).ok_or(IntErrorKind::PosOverflow)?,
        ))
    }

    #[inline]
    fn apply(&self, span: Span<T>) -> Option<Span<T>> {
        Some(Span {
            x: NonZeroRange::new_unchecked(span.x.start..span.x.end + self.radius + self.radius),
            y: span.y + self.radius,
        })
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
/// All arithmetic happens on `T` without checked operations: the [`DilateStrategy`] maps
/// each input span into the coverage bounds derived from the parent's [`ImageDimension`]
/// bounds — how out-of-bounds spans are treated is the strategy's job ([`DilateInPlace`]
/// clips them, [`DilateTranslated`] requires them in bounds) — and
/// [`Self::with_strategy`] rejects any `bounds`/`offset` combination for which the largest
/// intermediate value is not representable in `T`. This rules out every overflow and
/// out-of-bounds access the iterator could perform.
pub struct DilateSpanIterAcc<I, T, S = DilateInPlace<T>> {
    input: I,
    /// Peeked next input span — already dilated by the strategy (see [`Self::pull_next`]),
    /// so the window logic below can rely on its `y` — pulled lazily from `input`.
    next_input: Option<Span<T>>,
    offset: T,
    strategy: S,
    bounds: Rect<u32>,
    /// Exclusive end (absolute row) of the coverage: rows beyond can never gain coverage,
    /// so iteration stops before reaching it.
    cov_y_end: T,
    /// Window entries as `(x, y)` in `y` order, where `x` holds the dilated coverage
    /// indices (absolute) of the span — computed once by [`DilateStrategy::apply`] when the
    /// span enters the window, never again.
    active: VecDeque<(Range<T>, T)>,
    /// Per-column coverage accumulator, indexed by absolute `x`. Entry `> 0` means the
    /// column is alive.
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
    /// Creates an iterator dilating with the default [`DilateInPlace`] strategy, which
    /// clips input spans to the input bounds `iter` declares.
    pub fn new(iter: I, offset: T::NonZero) -> Result<Self, PipelineError> {
        let bounds = iter.bounds();
        let outer_x_end = u64::from(bounds.x) + u64::from(bounds.width.get());
        let outer_x = try_coordinate::<T>(u64::from(bounds.x))?..try_coordinate::<T>(outer_x_end)?;
        Self::with_strategy(iter, offset, DilateInPlace::new(offset.into(), outer_x))
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

        // Coverage bounds: `orig_bounds` dilated by the strategy (x) and `offset` (y).
        // Input spans are clipped to `orig_bounds`, so every coordinate the iterator
        // produces — coverage index or emitted span — stays within `bounds`: rejecting
        // bounds not representable in `T` rules out every overflow of the hot path.
        let bounds = strategy.compute_bounds(orig_bounds)?;
        let bounds_x_end = u64::from(bounds.x) + u64::from(bounds.width.get());
        let bounds_y_end = u64::from(bounds.y) + u64::from(bounds.height.get());
        try_coordinate::<T>(bounds_x_end)?;
        try_coordinate::<T>(bounds_y_end)?;

        let cov_y_start = try_coordinate::<T>(u64::from(bounds.y))?;
        let cov_y_end = try_coordinate::<T>(bounds_y_end)?;
        // Coverage is indexed by absolute `x`, so it also covers the padding left of
        // `bounds.x` (which `row_a` never scans).
        let cov_len = usize::try_from(bounds_x_end)?;

        let mut this = Self {
            input: iter,
            next_input: None,
            offset: offset_val,
            strategy,
            bounds,
            cov_y_end,
            active: VecDeque::new(),
            coverage: vec![0u16; cov_len],
            row_a: 0,
            row_b: 0,
            cursor: 0,
            cur_y: cov_y_start,
        };
        this.next_input = this.pull_next();
        if let Some(first) = this.next_input {
            // First row which can produce output: never before the coverage starts.
            this.cur_y = first.y.saturating_sub(&offset_val).max(cov_y_start);
        }
        if this.cur_y < this.cov_y_end {
            this.load_row();
            this.cursor = this.row_a;
        }
        Ok(this)
    }

    /// Pulls the next span from `input` and dilates it via [`DilateStrategy::apply`],
    /// skipping spans the strategy rejects (e.g. entirely outside the input bounds).
    fn pull_next(&mut self) -> Option<Span<T>> {
        loop {
            let span = self.input.next()?;
            if let Some(dilated) = self.strategy.apply(span) {
                debug_assert!(
                    UncheckedCast::<u64>::cast_unchecked(dilated.x.end)
                        <= self.coverage.len() as u64,
                    "dilated {dilated:?} escapes the coverage bounds — was the strategy \
                     constructed for the input bounds?"
                );
                return Some(dilated);
            }
        }
    }

    fn add_range(&mut self, range: &Range<T>) {
        let (a, b) = coverage_indices(range);
        // Counts provably stay within `2 * offset + 1 <= u16::MAX` (see `with_strategy`),
        // so wrapping cannot occur — plain add/sub keeps the loop vectorizable.
        for c in &mut self.coverage[a..b] {
            *c = c.wrapping_add(1);
        }
    }

    fn remove_range(&mut self, range: &Range<T>) {
        let (a, b) = coverage_indices(range);
        for c in &mut self.coverage[a..b] {
            *c = c.wrapping_sub(1);
        }
    }

    /// Expires spans that no longer influence `cur_y` and enqueues all pending input spans
    /// that start influencing it, keeping `coverage` in sync. Also re-derives the alive
    /// region `[row_a, row_b)`, so that `next` never scans dead padding columns.
    fn load_row(&mut self) {
        let row: u64 = UncheckedCast::cast_unchecked(self.cur_y);
        let offset: u64 = UncheckedCast::cast_unchecked(self.offset);
        while let Some((range, _)) = self
            .active
            .pop_front_if(|(_, y)| UncheckedCast::<u64>::cast_unchecked(*y) + offset < row)
        {
            self.remove_range(&range);
        }

        let enter_until = row + offset;
        loop {
            let span = match self.next_input {
                Some(span) if UncheckedCast::<u64>::cast_unchecked(span.y) <= enter_until => span,
                _ => break,
            };
            self.next_input = self.pull_next();
            // Spans which already fell out of the window (e.g. because `cur_y` was clamped
            // or jumped past them) can never influence a row to come: skip them.
            if UncheckedCast::<u64>::cast_unchecked(span.y) + offset < row {
                continue;
            }
            let range = span.x.start..span.x.end;
            self.add_range(&range);
            self.active.push_back((range, span.y));
        }

        let (mut row_a, mut row_b) = (usize::MAX, 0);
        for (range, _) in &self.active {
            let (a, b) = coverage_indices(range);
            row_a = row_a.min(a);
            row_b = row_b.max(b);
        }
        self.row_a = if row_a == usize::MAX { 0 } else { row_a };
        self.row_b = row_b;
    }

    /// Advances to the next output row, reloading the sliding window. Returns `false` once
    /// the iteration is done.
    fn advance_row(&mut self) -> bool {
        // `cur_y` stays below `cov_y_end` (which fits `T`), so this cannot overflow.
        let mut next_row = self.cur_y + T::one();
        if self.active.is_empty() {
            // Nothing is alive: jump directly to the first row the next input span
            // influences instead of scanning every empty row in between one by one.
            let Some(next_span) = self.next_input else {
                return false;
            };
            next_row = next_row.max(next_span.y.saturating_sub(&self.offset));
        }
        if next_row >= self.cov_y_end {
            // Rows beyond the coverage bounds can never gain coverage.
            return false;
        }
        self.cur_y = next_row;
        self.load_row();
        self.cursor = self.row_a;
        true
    }
}

/// Relative coverage indices `[a, b)` of an already clipped and dilated range.
fn coverage_indices<T: UncheckedCast<u32>>(range: &Range<T>) -> (usize, usize) {
    (
        UncheckedCast::<u32>::cast_unchecked(range.start) as usize,
        UncheckedCast::<u32>::cast_unchecked(range.end) as usize,
    )
}

fn try_coordinate<T>(value: u64) -> Result<T, IncompatibleSizeError>
where
    T: TryFrom<u64, Error: Into<IncompatibleSizeError>>,
{
    T::try_from(value).map_err(Into::into)
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
                    <u32 as UncheckedCast<T>>::cast_unchecked(start as u32),
                    <u32 as UncheckedCast<T>>::cast_unchecked(end as u32),
                ),
                y: self.cur_y,
            });
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<I, T, S> ImageDimension for DilateSpanIterAcc<I, T, S> {
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

    use crate::{
        DilateSpanIterAcc, DilateTranslated, ImageDimension, ImaskSet, PipelineError, Rect,
        SortedRanges, Span,
    };

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

    #[test]
    fn dilate_spans_entirely_outside_roi() {
        let roi = Rect::new(4, 0, NonZero::new(3).unwrap(), NonZero::new(1).unwrap());
        let result: Vec<_> = vec![
            Span::new(0..1, 0u32),
            Span::new(5..6, 0u32),
            Span::new(9..10, 0u32),
        ]
        .into_iter()
        .with_bounds(W, H)
        .dilate_within(NonZero::new(1u32).unwrap(), roi)
        .unwrap()
        .collect();

        assert_eq!(vec![Span::new(4..7, 0u32), Span::new(4..7, 1u32)], result);
    }

    #[test]
    fn dilate_span_crossing_roi_edge_is_clipped_to_roi() {
        let roi = Rect::new(4, 0, NonZero::new(3).unwrap(), NonZero::new(1).unwrap());
        let result: Vec<_> = vec![Span::new(3..8, 0u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate_within(NonZero::new(1u32).unwrap(), roi)
            .unwrap()
            .collect();

        assert_eq!(vec![Span::new(3..8, 0u32), Span::new(3..8, 1u32)], result);
    }

    #[test]
    fn dilate_spans_outside_roi_rows() {
        let roi = Rect::new(0, 4, NonZero::new(10).unwrap(), NonZero::new(2).unwrap());
        let result: Vec<_> = vec![
            Span::new(5..6, 0u32),
            Span::new(5..6, 4u32),
            Span::new(5..6, 9u32),
        ]
        .into_iter()
        .with_bounds(W, H)
        .dilate_within(NonZero::new(1u32).unwrap(), roi)
        .unwrap()
        .collect();

        assert_eq!(
            vec![
                Span::new(4..7, 3u32),
                Span::new(4..7, 4u32),
                Span::new(4..7, 5u32),
            ],
            result
        );
    }

    #[test]
    fn dilate_within_disjoint_roi_is_empty() {
        let roi = Rect::new(200, 200, NonZero::new(3).unwrap(), NonZero::new(1).unwrap());
        let result = vec![Span::new(0..1, 0u32)]
            .into_iter()
            .with_bounds(W, H)
            .dilate_within(NonZero::new(1u32).unwrap(), roi);
        assert!(matches!(result, Err(PipelineError::Empty)));
    }

    #[test]
    fn dilate_translated_translates_x_and_y() {
        let radius = NonZero::new(1u32).unwrap();
        let iter = DilateSpanIterAcc::with_strategy(
            vec![Span::new(5..6, 0u32), Span::new(5..6, 5u32)]
                .into_iter()
                .with_bounds(W, H),
            radius,
            DilateTranslated::new(1),
        )
        .unwrap();
        assert_eq!(
            Rect::new(0, 0, NonZero::new(102).unwrap(), NonZero::new(102).unwrap()),
            iter.bounds()
        );

        let result: Vec<_> = iter.collect();
        // DilateInPlace would emit 4..7 at rows 0,1 and 4..=6. Translated shifts x and y
        // by +radius without saturating: every input row y becomes the full
        // 2 * radius + 1 rows [y, y + 2 * radius].
        assert_eq!(
            vec![
                Span::new(5..8, 0u32),
                Span::new(5..8, 1u32),
                Span::new(5..8, 2u32),
                Span::new(5..8, 5u32),
                Span::new(5..8, 6u32),
                Span::new(5..8, 7u32),
            ],
            result
        );
    }

    #[test]
    fn dilate_translated_requires_y_space() {
        // 250 + 5 + 2 * radius = 257 doesn't fit u8: compute_bounds/with_strategy must
        // reject it, mirroring the x check.
        let roi = Rect::new(0, 250, NonZero::new(10).unwrap(), NonZero::new(5).unwrap());
        let result = DilateSpanIterAcc::with_strategy(
            vec![Span::new(0u8..1, 250u8)].into_iter().with_roi(roi),
            NonZero::new(1u8).unwrap(),
            DilateTranslated::new(1),
        );
        assert!(matches!(result, Err(PipelineError::IncompatibleSize(_))));
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
