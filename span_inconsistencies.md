# Span Coordinate System Inconsistencies

## Stated Semantic

Spans should always be in the **global** coordinate system (i.e. `span.y` is the
absolute image row, `span.x` is the absolute image column range).

## Current State: Inconsistent

Different span producers and consumers disagree on whether `span.y` is LOCAL
(relative to the ROI origin) or GLOBAL (absolute image position).

### Producers

| Producer                         | File:Line        | Span `y` semantic                  |
|----------------------------------|------------------|------------------------------------|
| `SortedRangesSpanIter::next`     | `span.rs:145`    | **LOCAL** (`start / width`)        |
| `BitmapToSpanIter::next`         | `from_bitmap.rs:53` | LOCAL (relative to bitmap origin) |
| `RectSpanIter::new`/`next`       | `span/rect.rs:15`   | **GLOBAL** (`rect.y`)             |
| `DilateSpanIter::next`           | `dilate.rs:98-108`  | Preserves input frame             |
| `ClipSpanIter::next`             | `clip.rs:84`        | Preserves input frame             |
| `AffineTransformHeap`            | `affine_transform.rs:289` | bounds-frame (verifies `span.y >= bounds.y`) |

### Consumers

| Consumer                         | File:Line        | Expected span frame                |
|----------------------------------|------------------|------------------------------------|
| `try_from_span_iter`             | `set.rs:399-410` | **LOCAL** (`y*width + x`, no offset) |
| `SpanIntoRangesIter::next`       | `into_ranges.rs:65-67` | **GLOBAL** (subtracts `static_offset`) |

## Detailed Findings

### 1. `SortedRangesSpanIter` produces LOCAL y

`span.rs:144-145`:
```rust
let width = self.parent.width().get().cast_unchecked();
let y = start / width;
```

`y` is computed purely from the 1D storage position divided by `bounds.width`.
Since `SortedRanges` stores LOCAL coordinates (positions relative to the ROI
origin starting at 0), `y` is a LOCAL row index in `[0, bounds.height)`.

The offset (`bounds.x`, `bounds.y`) is **never added** to `span.y`.

### 2. `RectSpanIter` produces GLOBAL/bounds-frame y

`span/rect.rs:15`:
```rust
Span::new(rect.x..rect.len_x(), rect.y)
```

`y` is set directly to `rect.y`, which is the absolute image row. These spans
are in the GLOBAL coordinate system.

### 3. `SpanIntoRangesIter` expects GLOBAL spans (subtracts offset)

`into_ranges.rs:32`:
```rust
let static_offset = (bounds.x + bounds.y * bounds.width.get())
```

`into_ranges.rs:65-67`:
```rust
let offset = next.y * self.bounds.width.into();
let start = offset + next.x.start - self.static_offset;
let end   = offset + next.x.end   - self.static_offset;
```

This **subtracts** `static_offset = bounds.x + bounds.y * bounds.width`,
which is the inverse of the GLOBAL convention. It assumes its input spans
have GLOBAL `y` (as produced by `RectSpanIter`).

### 4. `try_from_span_iter` expects LOCAL spans (no offset)

`set.rs:398-401`:
```rust
let y: u64 = first.y.try_into().map_err(invalid_data)?;
let mut merge_start = y * width_u64 + first.x.start.try_into()...?;
let mut merge_end   = y * width_u64 + first.x.end.try_into()...?;
```

No offset is subtracted or added. This expects LOCAL spans (as produced by
`SortedRangesSpanIter`).

### 5. `SortedRangesSpanIter::bounds()` drops the offset

`span.rs:89-101` forwards to `SortedRangesIter::bounds()` (`set/iter.rs:44-51`),
which **hardcodes `x:0, y:0`**:
```rust
fn bounds(&self) -> crate::Rect<u32> {
    Rect { x: 0, y: 0, width: self.width, height: self.height }
}
```

This silently drops the bounds offset that `SortedRanges::bounds()` retains
(`set.rs:596-598`). So `sorted_ranges.bounds()` reports the ROI offset, but
`sorted_ranges.spans().bounds()` reports `x:0, y:0`.

### 6. `SortedRanges::new` asserts offset is zero

`set.rs:345-346`:
```rust
assert!(bounds.x == 0);
assert!(bounds.y == 0);
```

Direct construction requires the offset to be zero, suggesting the design
intent was that spans operate in a zero-offset frame.

## Concrete Bug: `spans().into_ranges()` chain is broken with offset

The chain `sorted_ranges_with_offset.spans().into_ranges()` is **broken** when
`bounds.x + bounds.y * bounds.width > 0`:

1. `spans()` produces spans with LOCAL `y` (via `SortedRangesSpanIter`)
2. `into_ranges()` assumes GLOBAL `y` and **subtracts** `static_offset`
3. Result: underflow (in debug) or silently wrong positions (in release)

Example: `SortedRanges` with `bounds = {x:1, y:2, w:100, h:200}`:
- `static_offset = 1 + 2*100 = 201`
- A span at LOCAL `y=0, x=10..20` would compute:
  `start = 0*100 + 10 - 201 = -191` (underflow in unsigned arithmetic)

## Recommended Fix Direction

To make spans consistently GLOBAL:

1. **`SortedRangesSpanIter::next`** (`span.rs:145`): Add `bounds.y` to the
   computed `y`: `let y = start / width + offset_y;`
2. **`SortedRangesSpanIter`** needs access to `bounds.y` (currently only has
   `width` and `height` via the parent's `ImageDimension`). May need to pass
   the full bounds or offset to the span iterator.
3. **`SortedRangesSpanIter::bounds()`**: Should forward the actual bounds
   (with offset) instead of hardcoding `x:0, y:0`.
4. **`try_from_span_iter`** (`set.rs:399`): Should subtract the offset when
   converting GLOBAL spans to LOCAL storage positions.
5. **`SpanIntoRangesIter`** is already correct for GLOBAL spans — no change
   needed once producers are fixed.

Alternatively, to make spans consistently LOCAL, fix `RectSpanIter` and
`SpanIntoRangesIter` instead. But the stated semantic is "spans are always
global", so the first direction is preferred.
