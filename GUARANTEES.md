# Guarantees

> This is the target state. Not all of it holds today; see
> [GUARANTEES_PLAN.md](GUARANTEES_PLAN.md) for the plan to get there.

imask checks once, at the boundary, and then relies on its types:
**only construction can fail.** After that, operations on valid values don't
overflow and iterators don't fail partway through.

## The `SortedRanges<T>` invariant

`T` stores flat pixel indices. Coordinates use the type with half the bits, `T::Sqrt`:

| `T`   | coordinates (`T::Sqrt`) | largest image   |
|-------|-------------------------|-----------------|
| `u16` | `u8`                    | 255 × 255       |
| `u32` | `u16`                   | 65 535 × 65 535 |
| `u64` | `u32`                   | (2³²−1)²        |
| `u128`| `u64`                   | (2⁶⁴−1)²        |

Only these types are supported; `SortedRanges<u8>` doesn't compile.

Every `SortedRanges<T>` value satisfies:

1. **Non-empty:** it holds at least one range, and every range has at least one pixel.
   Ranges are sorted and disjoint.
2. **Representable bounds, by type:** the bounds are a `Roi<T::Sqrt>`, so every
   end fits `T::Sqrt`, exclusive ends included. A 1×1 roi at offset
   `T::Sqrt::MAX` can't be expressed.
3. **Contained:** every pixel lies inside `bounds`.

**Why this is enough:** width and height are at most 2ⁿ−1, where n is the bit
width of `T::Sqrt`. So every flat index computed while iterating stays below
`W·H + W ≤ (2ⁿ−1)·2ⁿ < T::MAX`, which leaves a full row of headroom, and every
coordinate fits in `T::Sqrt`. Iteration therefore needs no checks in debug or
release builds.

`SortedRangesMap` follows the same rules.

## Promises

- **Only construction fails.** Every checked constructor (from ranges, spans,
  bytes, …) validates the invariant and returns a `Result`.
- **`*_unchecked` is a deliberate escape hatch.** `NonZeroRange::new_unchecked`
  and `Roi::new_unchecked` are for code that has already checked the dimensions,
  for example `Union`, whose spans can't exceed its bounds. They use
  `debug_assert!` for their contract and skip the check in release builds,
  because checking everywhere is too expensive. Violating the contract is a bug.
- **Combining doesn't fail.** `union` of two `SortedRanges<T>` returns a
  `SortedRanges<T>`. `intersect`, `subtract` and `clip` return `Option`, where
  `None` means the result is empty; none of them returns an error. They need no
  checks: the result's bounds come from the inputs' bounds (at most their
  hull), so they are again a `Roi<T::Sqrt>`.
- **Native iteration doesn't fail.** `spans(&self) -> impl Iterator<Item = Span<T::Sqrt>>`
  returns no `Result`, never panics and checks nothing per item.
- **Other targets are checked once, before iterating.** `Span<usize>`, `Span<u8>`,
  local ranges, or global ranges at a different width are available through
  `try_*` methods. These check the bounds against the target when the iterator
  is built, never per item. A narrower target works whenever the bounds fit it.
- **Leaving the domain is explicit.** Operations that can grow past `T::Sqrt`
  (dilate, translate, affine transform) either fail at construction or clip to
  a roi the caller gives.
- **`Sqrtable` is the only bound on the type.** `SortedRanges<T: Sqrtable>`
  stores its bounds as `Roi<T::Sqrt>`. Every other bound (casts, arithmetic)
  goes on the methods that need it.

## Span pipelines

- An iterator's `ImageDimension::roi()` contains every span it yields.
- Combinators validate in their constructor (`Result<_, PipelineError>`), so
  `next()` never fails because of sizes.
- Types carry trust. Spans that come from valid `SortedRanges` and pass only
  through combinators that keep them inside the domain (union, intersect,
  subtract, clip) can be collected back without a fallible step. Spans from
  foreign iterators are validated while they are collected.

## Debug vs. release

- **Debug** builds check every unchecked step: arithmetic overflow, a
  `debug_assert!` on every narrowing cast (`UncheckedCast`), and the full
  invariant after every internal construction. A broken invariant panics at
  the first wrong step.
- **Release** builds rely on the invariant and skip per-item checks. If an
  invariant ever breaks (because of an imask bug or a broken `*_unchecked`
  contract), results are wrong, but there is no undefined behaviour: no
  `unsafe` code depends on these invariants.
