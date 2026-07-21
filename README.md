# imask

Works on top of range-set-blaze to represent image masks as iterator of ranges(e.g. Annotation-Masks). It adds 2-dimensional iterator operators (e.g. dillute, erode) which are orders of magnitude smaller than a bitmap represetation. If range-set-blaze becomes more stable, it will be added as a required dependency. Until then, multiple versions can can be supported via feature-flags. This project aims to upstream general changes to range-set-blaze if they fit.

Collections and wire-formats usually store a ROI (region of interest, which specifies top-left offset (x, y), width and height) of the image mask and store the ranges in the inner coordinate system for maximum storage efficiency. You can ask for both global ranges (IntoIterator) or iter_roi, to get ranges in the ROI coordinate system.
The Iterator-Combinators don't consider the offset and may inherit the ImageWidth from the parent Iterator via `ImageDimension`-trait if it's needed.

# Error Philosophy

When a Iterator is used, the values are expected to successfuly cast to the value of the accumulator. We expect `SortedRanges::<u16>::iter::<u32>()` not to overflow -> This is checked for debug builds, but not if `cfg!(not(debug_assertions))`, as it causes significant slowdown otherwise. You can therefore not rely on ranges not to be empty in unsafe code. This library continues processing if this ever happens, but might add some lightweight assertions in release-mode (e.g. check, if accumulator is > biggest single element).
When comeing from the unchecked places, error-detection is usually provided by returning a Result or having a method `into_result` for cases where error detection can only happen if the `Iterator` was consumed. Iterators stop after a error occurs. If into_result is not called, it causes the iterator to panic if debug_assertions are enabled.

`core::ops::RangeInclusive` and `core::ops::Range` are both expected to have `start < end` in most scenarios, except when loading them from a unchecked iterator.

# ImageDimensions
It's not clear, if ImageDimensions are kept. The main benefits are:
- Bounds can be used in e.g. a GPU-Shader to only affect certain sub-areas, but these could also be generated while collecting.
- We can detect wrong AffineTransformations before running them on data.
- Store Rectangles very efficiently (see Line overlapping)

## Line overlapping
The Serialization-Format and SortedRanges can both represent a arbitrary rectangle as a single entry. In the beginning, the Iterators aso worked on these entries directly.
It turned out, that most combinators had to separate them into what we now know as Spans, which meant repated splitting and merging of lines which caused significant overhead.
Most real annotations are not squares and don't benefit from the compact representation, but suffer from this overhead.

If line overlapping wasn't implemented (at least in SortedRanges), we could make the Dimensions depend on T instead of always u32 (use u16 for Pixels of u32, u32 for Pixels of u64 etc). The idea of having Varint in SortedRanges would be vanished. Drawback: We cannot make width bigger by making height smaller... Very wide images would be a bad fit: u32::MAX + 1 wide image with height 1 would require u128 for Ranges and u64 for bounds.
