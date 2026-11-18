# Guarantees

> Target state, not yet reality. See [GUARANTEES_PLAN.md](GUARANTEES_PLAN.md).

imask validates up front: at construction, or when an iterator is built.
After that, nothing overflows and no iterator fails midway.

## `SortedRanges<T, TBounds = T::Sqrt>`

`T` holds flat pixel indices, `TBounds` the coordinates of bounds and spans.
The struct has no trait bounds; methods carry what they need.

| `T`    | default `TBounds` | largest image   |
| ------ | ----------------- | --------------- |
| `u16`  | `u8`              | 255 × 255       |
| `u32`  | `u16`             | 65 535 × 65 535 |
| `u64`  | `u32`             | (2³²−1)²        |
| `u128` | `u64`             | (2⁶⁴−1)²        |

Other `TBounds` are allowed, e.g. `SortedRanges<u32, u32>` for a (2³¹−1) × 1 image.

## Invariant

1. At least one range; ranges are non-empty, sorted and disjoint.
2. Bounds are a `Roi<TBounds>`, so every coordinate fits, exclusive ends included.
3. `W·H + W ≤ T::MAX`, so every flat index fits `T`. Free for the default
   `TBounds`, checked at construction otherwise.
4. Every pixel lies inside the bounds.

## Promises

- **Fallible means `try_*`.** Everything else is infallible.
- **Same types never fail.** `spans()` yields `Span<TBounds>`. `intersect`,
  `subtract` and `clip` return `Option` (`None` = empty).
- **Except `union`:** the hull can outgrow `T`, so `union` is infallible only
  for the default `TBounds`, and `try_union` otherwise.
- **Mixed types are checked once.** Another span type, another `TBounds`,
  ranges or another width: `try_*` checks the bounds when the iterator is
  built, never per item.
- **`*_unchecked` trusts the caller.** It's meant for code that has already
  checked the dimensions (e.g. `Union`) and is only `debug_assert!`ed. A
  violation is a bug.
- **Debug checks every unchecked step; release trusts the invariant.** A broken
  invariant gives wrong results, never UB.

---

*Editing this file:* it is written for humans. Keep it very concise and limited
to the most important facts. Details and rationale go in GUARANTEES_PLAN.md.
