# imask

Works on top of range-set-blaze to represent image masks as iterator of ranges(e.g. Annotation-Masks). It adds 2-dimensional iterator operators (e.g. dillute, erode) which are orders of magnitude smaller than a bitmap represetation. If range-set-blaze becomes more stable, it will be added as a required dependency. Until then, multiple versions can can be supported via feature-flags. This project aims to upstream general changes to range-set-blaze if they fit.

Collections and wire-formats usually store a ROI (region of interest, which specifies top-left offset (x, y), width and height) of the image mask and store the ranges in the inner coordinate system for maximum storage efficiency. You can ask for both global ranges (IntoIterator) or iter_roi, to get ranges in the ROI coordinate system.
The Iterator-Combinators don't consider the offset and may inherit the ImageWidth from the parent Iterator via `ImageDimension`-trait if it's needed.

# Error Philosophy

When a Iterator is used, the values are expected to successfuly cast to the value of the accumulator. We expect `SortedRanges::<u16>::iter::<u32>()` not to overflow -> This is checked for debug builds, but not if `cfg!(not(debug_assertions))`, as it causes significant slowdown otherwise. You can therefore not rely on ranges not to be empty in unsafe code. This library continues processing if this ever happens, but might add some lightweight assertions in release-mode (e.g. check, if accumulator is > biggest single element).
When comeing from the unchecked places, error-detection is usually provided by returning a Result or having a method `into_result` for cases where error detection can only happen if the `Iterator` was consumed. Iterators stop after a error occurs. If into_result is not called, it causes the iterator to panic if debug_assertions are enabled.

`core::ops::RangeInclusive` and `core::ops::Range` are both expected to have `start < end` in most scenarios, except when loading them from a unchecked iterator.

# Iterator
Combinators are allowed or even encouraged to consume iterator elements from their parent during construction. This allows for optimizations like knowing, that each next() call has a previous item. This differs from the core::Iterators, which assume that you can put a `&mut impl Iterator` into any iterator and know, that it remains unchanged unless you poll the outer iterator. This has the ergonomic disadvantage of fallability during construction, which has to be handled or raised by the caller.

# ImageDimensions
It's not clear, if ImageDimensions are kept. The main benefits are:
- Bounds can be used in e.g. a GPU-Shader to only affect certain sub-areas, but these could also be generated while collecting.
- We can detect wrong AffineTransformations before running them on data.
- Store Rectangles very efficiently (see Line overlapping)
- If affine Transforms need a bitmap as intermediate response (todays Transforms are slow), it has a way of knowing the intermediate buffer size.
- Cheap check if the iterating number type can even hold the Range or Span.

## Line overlapping
The Serialization-Format and SortedRanges can both represent a arbitrary rectangle as a single entry. In the beginning, the Iterators aso worked on these entries directly.
It turned out, that most combinators had to separate them into what we now know as Spans, which meant repated splitting and merging of lines which caused significant overhead.
Most real annotations are not squares and don't benefit from the compact representation, but suffer from this overhead.

If line overlapping wasn't implemented (at least in SortedRanges), we could make the Dimensions depend on T instead of always u32 (use u16 for Pixels of u32, u32 for Pixels of u64 etc). The idea of having Varint in SortedRanges would be vanished. Drawback: We cannot make width bigger by making height smaller... Very wide images would be a bad fit: u32::MAX + 1 wide image with height 1 would require u128 for Ranges and u64 for bounds.

# Spans vs Ranges
Ranges are able to cover multiple lines at once, while Spans are created pre column. A Rect(10, 10, 10, 10) `Range<T>` could be represented by a single Range, but requires 10 Spans. Ranges always operate in the local coordinate system, so Range 1..2 actually means Col 11..12 in Row 10 in the Rect example above. Spans offers a different abstraction, which always separates Rows and works with Global coordinates, so the first Span from a SpanRectIter would be Span(10..20, 10). You should pefer Spans, if you are working with Spans of different sizes or you need to know about Line, Min-Column, Max-Column. These are needed very often, and combinators don't have to recalculate them and merge/Split lines over and over again. If you just want to know the number of pixels and your inputs are mostly squares, Ranges API might be more performant.
