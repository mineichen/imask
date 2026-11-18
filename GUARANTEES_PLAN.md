# Plan: reaching GUARANTEES.md

Each step leaves the crate compiling with tests passing. Breaking API changes
are fine (version 0.0.1). Remove an item from *Gaps today* when the step that
closes it lands.

## Gaps today

1. Bounds are never checked against `T::Sqrt`. `try_from_ordered_iter`,
   `try_from_span_iter` and `from_serialized` accept any `Roi<u32>`. The
   `map_inplace` doctest even builds a `SortedRanges<u16>` with 1000×1000 bounds.
2. Containment isn't checked. `try_from_ordered_iter` and `from_serialized`
   accept ranges past `W·H`. `try_from_span_iter` (`span/builder/roi_hint.rs`)
   rejects spans left of or above the roi, but a span ending past `x.end`
   silently wraps into the next row.
3. `From<Span<T::Sqrt>>` computes `y + 1` in `u32`. That overflows for
   `SortedRanges<u64>` at `y = u32::MAX`; for smaller `T` it produces bounds
   outside `T::Sqrt`, for example at `y = 255` for `u16`.
4. `spans::<U>()`, `iter_roi::<R>()` and `iter_global_with` accept any target
   and depend on `UncheckedCast`, so release builds truncate silently. Only
   `try_into_spans` checks up front, and it is the model for step 2.3.
5. The debug check in `SortedRangesSpanIter::roi()` computes `width * height`
   in `u32`, so the check itself overflows for large `u64` masks.
6. `SortedRanges::new` (deprecated) builds a value without any check.
7. Tests use `SortedRanges<u8>`, which isn't supported. `Sqrtable` is
   implemented for `u128`, which isn't supported before Phase 6.
8. Dozens of bare `as` casts bypass the debug checks.
9. `Roi::expand_saturating` saturates at `u32::MAX` instead of `T::Sqrt::MAX`.
10. Bounds are `Roi<u32>` for every `T`, so invariant 2 isn't enforced by the
    type, and `u128` can't use its `u64` coordinates.

## Decisions

- **`SortedRanges<u8>`** only appears in tests. Replace it with `SortedRanges<u16>` (step 1.1).
- **`SortedRanges<u128>`** isn't supported until Phase 6. It comes back once
  bounds are `Roi<T::Sqrt>`, with `u64` coordinates.
- **`*_unchecked` constructors stay public** and are checked only by
  `debug_assert!`. Callers such as `Union` already know their spans are in range,
  so checking everywhere would be too expensive. A hit assert is a bug (step 1.4).
- **Where `T: Sqrtable` goes:** until Phase 6, only on the methods that need
  it, and bounds stay `Roi<u32>` with a runtime check. In Phase 6 the type
  becomes `SortedRanges<T: Sqrtable>` with `bounds: Roi<T::Sqrt>`. Generic code
  then repeats `T: Sqrtable`, because Rust doesn't imply struct bounds.
- *Open, blocks step 6.3:* **`ImageDimension::roi()`** returns `Roi<u32>`, which
  can't hold `u64` coordinates. Should it follow the span coordinate type? That
  would also remove the remaining `TryFrom<u32>` checks in span combinators
  such as `clip`.
- *Open:* **constructor error type.** Today it is `io::Error` for ranges and
  bytes and `PipelineError` for spans. Unifying them isn't required for the guarantees.

## Phase 0 – Safety net (no behaviour change)

**0.1 `check_invariants`.** Add a crate-internal
`fn check_invariants(&self) -> Result<(), InvariantError>` for `T: Sqrtable`
that checks invariants 1–3 of GUARANTEES.md. It is the single executable
definition of the invariant.
*Done when:* each violation has a test, including a 1×1 roi at offset `T::Sqrt::MAX`.

**0.2 Test both profiles.** `cargo test` proves the debug asserts hold.
`cargo test --release` proves no logic hides inside `debug_assert!`. Run both in
the flake check or in CI.
*Done when:* both run on every change.

**0.3 Boundary tests on `u16`.** With `u16` storage the coordinates are `u8`, so
`T::Sqrt::MAX` is cheap to reach, and the logic is generic, so coverage carries
over to the wider types. Cases: a full 255×255 roi; rois ending at 255 in x
and y; a 1×1 roi at offset 254 (valid) and at 255 (rejected); a span at
`y = 254`; the union of masks in opposite corners. Assert the *target*
behaviour, and mark tests that fail today with `#[ignore = "GUARANTEES_PLAN x.y"]`.
*Done when:* every test exists and each ignore names the step that will fix it.

## Phase 1 – Construction is the only check

**1.1 Narrow the supported types.** Remove `impl Sqrtable for u128` and
`impl Squareable for u64`, so the constructors from step 1.2 rule out
`SortedRanges<u128>` until step 6.3 adds them back. Replace `SortedRanges<u8>`
with `SortedRanges<u16>` in tests, and shrink their bounds to at most 255×255
instead of widening `T`, so the edge coverage of `u16` stays. Tests that
checked "the gap doesn't fit `u8`" become "the bounds exceed `T::Sqrt`" tests
in step 1.2. Move other tests and doctests whose bounds don't fit, such as the
1000×1000 `map_inplace` doctest, to `u32`.
*Done when:* every `SortedRanges` built in tests passes `check_invariants` (gap 7 is closed).

**1.2 Validate in every public constructor**, in release builds too. Add
`T: Sqrtable` to `try_from_ordered_iter`, `try_from_span_iter(_minbounds)`,
`SortedRangesSpanBuilder`, `from_serialized`, `map_inplace`,
`map_span_inplace` and `clip_inplace`, and check:
- `bounds.{x,y}.end <= T::Sqrt::MAX`: an O(1) check before any input is consumed.
- containment: each range ends at or before `W·H`, and each span satisfies
  `local_x_end <= width` and `local_y < height`.
- non-empty, sorted and disjoint, as today.

End each constructor with `debug_assert!(self.check_invariants().is_ok())`.
Report domain violations as `IncompatibleSizeError`.
*Done when:* the Phase 0.3 construction tests are no longer ignored (gaps 1 and 2 are closed).

**1.3 Fix the infallible conversions.** `From<Span<T::Sqrt>>` becomes `TryFrom`,
because a span at `y = T::Sqrt::MAX` has no representable exclusive end.
`From<Roi<T::Sqrt>>` stays infallible because it is correct by type. Remove the
deprecated `SortedRanges::new`.
*Done when:* gaps 3 and 6 are closed.

**1.4 Document the unchecked contract.** Keep `NonZeroRange::new_unchecked` and
`Roi::new_unchecked` public, each with a `debug_assert!` of its contract.
Their docs state that violating the contract is a caller bug: debug builds
panic, and release builds give wrong results but no undefined behaviour. Any
future rkyv/serde `Deserialize` must go through the validating constructor;
today only `Archive` is derived.
*Done when:* the only safe public way to get a `SortedRanges` that fails
`check_invariants` is to break a documented `*_unchecked` contract.

## Phase 2 – Native iteration never fails

**2.1 Separate the accumulator type from the output type.**
`SortedRangesSpanIter` accumulates flat indices in `T` and emits
`Span<T::Sqrt>` through a narrowing `UncheckedCast`, which is checked in debug
builds. Remove the debug-only checks in its `ImageDimension` impl (gap 5);
the invariant now guarantees what they checked.

**2.2 Native API without generics.** Add
`spans(&self) -> impl Iterator<Item = Span<T::Sqrt>> + ImageDimension` and
`spans_owned(self)`. `IntoIterator` yields `Span<T::Sqrt>`, and `iter_roi`
yields ranges of `T`. The only bound is `T: Sqrtable`.

**2.3 `try_*` for every other target:** `try_spans::<U>()`,
`try_into_spans::<U>()` (which exists already), `try_iter_roi::<R>()` and
`try_iter_global::<R>(width)`. Each does one O(1) check of the bounds when
built. For global ranges the check is that `(y_end − 1) · width + x_end` fits.
Deprecate the generic unchecked variants, migrate the crate, benches and fuzz
targets, then remove the deprecated variants.
*Done when:* no public `SortedRanges` method has an `UncheckedCast` bound, and
the iteration boundary tests are no longer ignored (gap 4 is closed).

## Phase 3 – Combining never fails

**3.1 Internal trusted collector.** Builds `SortedRanges<T>` from spans that
are already known to be valid, with no per-span checks in release builds.
Debug builds check every span and then run `check_invariants`.

**3.2 Inherent operations:** `union(&self, &Self) -> Self`, and
`intersect`/`subtract(&self, &Self) -> Option<Self>`. `clip_inplace` returns
`Option<Self>`, because its `IncompatibleSize` error can no longer happen. All
of them are built on the span combinators and 3.1.
*Done when:* the union of masks in opposite corners at 255 passes without a `Result`.

**3.3 Trust in types** (optional, larger). Add a sealed marker trait, for
example `TrustedSpans<S>`, similar in spirit to `TrustedLen`. The
`SortedRanges` span iterators implement it, and so do
`Union`/`Intersect`/`Subtract`/`Clip` when all of their inputs implement it.
`SortedRanges::from_trusted(iter)` is then infallible for user-composed
pipelines. Everything else goes through `try_from_span_iter`.

**3.4 Operations that leave the domain.** In their constructors, dilate and
affine transform (and translate later) check the output roi against the
target's `T::Sqrt::MAX`, or clip to it explicitly as `dilate_within` does.
Make `Roi::expand_saturating` saturate at `T::Sqrt::MAX` instead of
`u32::MAX` (gap 9).

**3.5 `SortedRangesMap`.** Apply Phases 1–3. Decide then which of
`TIncluded`/`TExcluded` defines the domain.

## Phase 4 – Thorough debug checking

**4.1 No bare casts.** Replace `as` in non-test code with `From`/`TryFrom` for
widening or fallible casts, or with `UncheckedCast` for narrowing that an
invariant justifies. Enforce it with `#![deny(clippy::as_conversions)]`,
allowing it in tests (gap 8).

**4.2 Name the justification.** Every `*_unchecked` call gets a one-line
`// INVARIANT:` comment that names the invariant making it correct, the way
`// SAFETY:` comments work for `unsafe`.
*Done when:* clippy passes and `grep _unchecked` shows a justification for each call.

## Phase 5 – Prove it

**5.1 Fuzz `from_serialized`.** Arbitrary bytes must produce either `Err`, or a
value that passes `check_invariants` and whose `spans()` runs to the end without
panicking. The fuzz profile already enables `debug-assertions`.

**5.2 Fuzz the combinators.** For two arbitrary valid `SortedRanges<u16>`,
union, intersect and subtract must not panic, and each result must pass
`check_invariants` and match a bitmap reference (same pattern as `cluster_reference`).

**5.3 Exhaustive edges on `u16`.** For every roi with an edge at 254 or 255:
construct it, iterate it, and round-trip it through bytes and spans.

**5.4 Docs.** Shrink the README "Error Philosophy" section to a summary that
links to GUARANTEES.md, and remove its statement that release builds don't
check. Delete *Gaps today* once it is empty.

## Phase 6 – Bounds in `T::Sqrt` (eventually)

**6.1 Decide `ImageDimension`** (see the open decision). Steps 6.2 and 6.3
depend on the answer: `u16`–`u64` widen losslessly to `Roi<u32>`, but `u128`
does not.

**6.2 Store bounds as `Roi<T::Sqrt>`.** Change the type to
`SortedRanges<T: Sqrtable>` with `bounds: Roi<T::Sqrt>`, and add `T: Sqrtable`
to every impl and generic user. Wherever a `Roi<u32>` comes in (from the
input's `ImageDimension`, or from the serialized header), convert it with
`Roi::try_cast`. That conversion replaces the bounds check from step 1.2.
Invariant 2 now holds by type, so drop it from `check_invariants`. The
`T::Sqrt::MAX` checks from step 3.4 become the coordinate type's own `MAX`.
*Done when:* `SortedRanges` stores no `Roi<u32>` (gap 10 is closed).

**6.3 Bring back `u128`.** Add `impl Sqrtable for u128` and
`impl Squareable for u64` again. The serialized header stores 4×`u32`, so
either a new protocol version adds `u64` bounds, or serializing a `u128` mask
whose bounds exceed `u32` fails. Add `u128` to the boundary tests at
`u64::MAX`.
*Done when:* `SortedRanges<u128>` passes the same tests as `SortedRanges<u64>`.
