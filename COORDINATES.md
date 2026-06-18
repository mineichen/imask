# Coordinate Systems in imask

## The three coordinate systems

### 1. Serialized format — LOCAL

Ranges in the serialized byte format are always gap-encoded **local** coordinates,
relative to the ROI origin (0,0 within the region). The ROI offset (x, y) is
stored only in the header as metadata.

Example: ROI at offset (1, 2), width 100. A pixel at local index 10 serializes
as `gap=10`, not `gap=211` (which would be the global index 2*100+1+10).

### 2. SortedRanges internal storage — LOCAL

`SortedRanges` stores `included` (lengths) and `excluded` (gaps) as **local**
1D positions at `bounds.width`. The accumulator in `iter_roi` walks forward
from 0. The bounds (x, y) are metadata — they are not baked into the stored
values.

- `iter_roi` → spits out **local** ranges
- `from_serialized` → reads raw (gap, len) pairs directly, no arithmetic

### 3. Spans — GLOBAL

Spans (`Span { y, x }`) are always in the **global** coordinate system. `y` is
the absolute image row, `x` is the absolute image column range. The bounds
offset is applied so that `span.y = bounds.y + local_row`.

## iter_global_* — LOCAL → output frame (0,0)

`iter_global_with` / `iter_global_owned_with` convert from local storage
coordinates to an output frame rooted at **(0, 0)** with the requested new
width/height. Internally, the source bounds' offset (x, y) is applied to map
local positions to their correct global pixel locations, but the output's
`ImageDimension` always reports `x: 0, y: 0` — the values are absolute positions
within that (0,0,width,height) frame, not relative to the original ROI offset.

```
Source SortedRanges (local, bounds={x:1, y:2, w:100, h:200})
                           │
           iter_global_with(150)
                           │
                           ▼
Output frame (0,0) at width 150:
  ImageDimension::bounds() = {x:0, y:0, w:150, h:202}
  Values = absolute pixel indices in the full image
```

Because the output has no offset metadata, it cannot be fed back into
`SortedRanges` constructors that expect local coordinates. This is intentional:
`iter_global_*` is a read-only projection.

### Loss of spatial information

`iter_global_*` **destroys the ImageDimension metadata**. Before iterating, the
SortedRanges knows exactly where its pixels live (e.g. a 10×10 rect at offset
5,5 in a 100×100 image). After `iter_global_*`, the bounds collapse to
`(0, 0, 100, 100)` — a mostly-empty bounding box that no longer tells you where
the actual content is. The pixel positions are baked into the range values, but
the spatial context (the ROI) is gone.

This is the reason `iter_global_*` output is read-only: you can iterate the
pixels, but you cannot reconstruct a meaningful SortedRanges from the result.

## Quick reference

| Component             | Coordinate system | Offset in values? |
|-----------------------|-------------------|-------------------|
| Serialized bytes      | LOCAL             | No (in header)    |
| SortedRanges storage  | LOCAL             | No (in bounds)    |
| `iter_roi`            | LOCAL             | No                |
| `iter_global_*`       | Output frame (0,0)| Yes (baked in)    |
| `ImageDimension` of `iter_global_*` | (0,0) | —      |
| Spans (target)        | GLOBAL            | Yes               |
| `from_serialized`     | passthrough       | No arithmetic     |
