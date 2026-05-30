use std::{
    fmt::{Debug, Display},
    num::NonZero,
    ops::Range,
};

use num_traits::Zero;

use crate::{
    CreateRange, ImageDimension, NonZeroRange, Rect, SignedNonZeroable, SortedRangesIter,
    SortedRangesSpanIter, UncheckedCast,
};

mod iter;
mod map_inplace;
mod offsets_iter;

pub use iter::*;
pub use map_inplace::*;
pub use offsets_iter::*;

/// Represents areas on images. It's designed to efficiently support various image sizes.
/// Both, TIncluded and TExcluded are expected to always be > 0. Use non-zero signed types
/// Included represents the number of pixels to include, excluded encodes the gap between two included ranges
///
/// Included.len() = excluded.len()
///
/// Meta is expected to be indexable for each included range
#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive))]
pub struct SortedRangesMap<TIncluded, TExcluded, TMeta> {
    included: Vec<TIncluded>,
    excluded: Vec<TExcluded>,
    meta: TMeta,
    bounds: Rect<u32>,
}
impl<TIncluded: UncheckedCast<u64>, TExcluded: UncheckedCast<u64>, TMeta: Debug> Debug
    for SortedRangesMap<TIncluded, TExcluded, Vec<TMeta>>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct DebugIter<T>(T);
        impl<T: IntoIterator<Item: Debug> + Clone> Debug for DebugIter<T> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_list().entries(self.0.clone()).finish()
            }
        }

        const NUMBER_OF_DEBUG_ELEMENTS: usize = 10;
        let mut s = f.debug_struct("SortedRangesMap");
        s.field("elements", &DebugIter(self.iter::<Range<u64>>()));
        s.field("bounds", &self.bounds);
        let more = self.included.len().saturating_sub(NUMBER_OF_DEBUG_ELEMENTS);
        if more > 0 {
            s.field(
                "additional_elements",
                &format_args!("...and {} more", self.included.len()),
            );
        }
        s.finish()
    }
}

type CopiedSliceIter<'a, T> = std::iter::Copied<std::slice::Iter<'a, T>>;

impl<TIncluded, TExcluded, TMeta> SortedRangesMap<TIncluded, TExcluded, Vec<TMeta>> {
    pub fn new<TRange>(r: NonZeroRange<TRange>, meta: TMeta, bounds: Rect<u32>) -> Self
    where
        TRange:
            UncheckedCast<TIncluded> + UncheckedCast<TExcluded> + std::ops::Sub<Output = TRange>,
    {
        assert!(bounds.x == 0);
        assert!(bounds.y == 0);
        Self {
            included: vec![r.len().cast_unchecked()],
            excluded: vec![r.start.cast_unchecked()],
            meta: vec![meta],
            bounds,
        }
    }
    pub fn try_from_ordered_iter<TRange>(
        iter: impl IntoIterator<Item = (Range<TRange>, TMeta), IntoIter: ImageDimension>,
    ) -> Result<Self, String>
    where
        TRange: Into<u64>,
        TIncluded: TryFrom<u64, Error: Display>,
        TExcluded: TryFrom<u64, Error: Display>,
    {
        let iter = iter.into_iter();
        let bounds = iter.bounds();
        assert!(bounds.x == 0);
        assert!(bounds.y == 0);
        fn create_checked<T: TryFrom<u64, Error: Display>>(
            start: u64,
            end: u64,
        ) -> Result<T, String> {
            if end <= start {
                return Err(format!("{} must be > {}", end, start));
            }
            T::try_from(end - start).map_err(|e| e.to_string())
        }

        let mut iter = iter.map(|(range, meta)| {
            let start = range.start.into();
            let end = range.end.into();
            create_checked::<TIncluded>(start, end).map(|x| (start..end, x, meta))
        });

        let Some((first_range, first_len, first_meta)) = iter.next().transpose()? else {
            return Err("Requires at least one item".into());
        };
        let initial_offset = TExcluded::try_from(first_range.start).map_err(|e| e.to_string())?;

        let mut included = Vec::<TIncluded>::with_capacity(iter.size_hint().0 + 1);
        let mut excluded = Vec::<TExcluded>::with_capacity(iter.size_hint().0 + 1);
        let mut meta = Vec::<TMeta>::with_capacity(iter.size_hint().0 + 1);

        included.push(first_len);
        excluded.push(initial_offset);
        meta.push(first_meta);

        let mut cur_pos = first_range.end;
        for x in iter {
            let (next_range, next_len, next_meta) = x?;
            excluded.push(create_checked(cur_pos, next_range.start)?);
            included.push(next_len);
            meta.push(next_meta);
            cur_pos = next_range.end;
        }

        Ok(Self {
            included,
            excluded,
            meta,
            bounds,
        })
    }
    pub fn iter<T: CreateRange<Item: Zero>>(
        &self,
    ) -> SortedRangesMapIter<
        CopiedSliceIter<'_, TIncluded>,
        CopiedSliceIter<'_, TExcluded>,
        std::slice::Iter<'_, TMeta>,
        T,
    >
    where
        TIncluded: UncheckedCast<T::Item>,
        TExcluded: UncheckedCast<T::Item>,
    {
        SortedRangesMapIter::new(
            self.included.iter().copied(),
            self.excluded.iter().copied(),
            self.meta.iter(),
            Zero::zero(),
        )
    }

    #[allow(clippy::len_without_is_empty, reason = "is_empty would always be true")]
    pub fn len(&self) -> usize {
        self.included.len()
    }

    pub fn len_nonzero(&self) -> NonZero<usize> {
        NonZero::new(self.included.len())
            .expect("Constructors make sure, there is always at least one Range")
    }

    pub fn iter_owned<T: CreateRange<Item: Zero>>(
        self,
    ) -> SortedRangesMapIter<
        std::vec::IntoIter<TIncluded>,
        std::vec::IntoIter<TExcluded>,
        std::vec::IntoIter<TMeta>,
        T,
    >
    where
        TIncluded: UncheckedCast<T::Item>,
        TExcluded: UncheckedCast<T::Item>,
    {
        SortedRangesMapIter::new(
            self.included.into_iter(),
            self.excluded.into_iter(),
            self.meta.into_iter(),
            Zero::zero(),
        )
    }

    pub fn ranges<T: CreateRange>(
        &self,
    ) -> SortedRangesIter<
        std::iter::Copied<std::slice::Iter<'_, TIncluded>>,
        std::iter::Copied<std::slice::Iter<'_, TExcluded>>,
        T,
    >
    where
        TIncluded: UncheckedCast<T::Item>,
        TExcluded: UncheckedCast<T::Item>,
        T::Item: Default + Copy + SignedNonZeroable + std::ops::Add<Output = T::Item>,
    {
        SortedRangesIter::new(
            self.included.iter().copied(),
            self.excluded.iter().copied(),
            T::Item::default(),
            self.bounds.width,
            self.bounds.height,
        )
    }
    pub fn ranges_owned<T: CreateRange>(
        self,
    ) -> SortedRangesIter<std::vec::IntoIter<TIncluded>, std::vec::IntoIter<TExcluded>, T>
    where
        TIncluded: UncheckedCast<T::Item>,
        TExcluded: UncheckedCast<T::Item>,
        T::Item: Default + Copy + SignedNonZeroable + std::ops::Add<Output = T::Item>,
    {
        SortedRangesIter::new(
            self.included.into_iter(),
            self.excluded.into_iter(),
            T::Item::default(),
            self.bounds.width,
            self.bounds.height,
        )
    }

    pub fn spans<T>(
        &self,
    ) -> SortedRangesSpanIter<
        SortedRangesIter<
            std::iter::Copied<std::slice::Iter<'_, TIncluded>>,
            std::iter::Copied<std::slice::Iter<'_, TExcluded>>,
            NonZeroRange<T>,
        >,
    >
    where
        NonZeroRange<T>: CreateRange<Item = T>,
        TIncluded: UncheckedCast<T>,
        TExcluded: UncheckedCast<T>,
        T: Default + Copy + SignedNonZeroable + std::ops::Add<Output = T>,
    {
        SortedRangesSpanIter::new(self.ranges::<NonZeroRange<T>>())
    }

    pub fn spans_owned<T>(
        self,
    ) -> SortedRangesSpanIter<
        SortedRangesIter<
            std::vec::IntoIter<TIncluded>,
            std::vec::IntoIter<TExcluded>,
            NonZeroRange<T>,
        >,
    >
    where
        NonZeroRange<T>: CreateRange<Item = T>,
        TIncluded: UncheckedCast<T>,
        TExcluded: UncheckedCast<T>,
        T: Default + Copy + SignedNonZeroable + std::ops::Add<Output = T>,
    {
        SortedRangesSpanIter::new(self.ranges_owned::<NonZeroRange<T>>())
    }
}

impl<TIncluded, TExcluded, TMeta> ImageDimension for SortedRangesMap<TIncluded, TExcluded, TMeta> {
    fn width(&self) -> NonZero<u32> {
        self.bounds.width
    }

    fn bounds(&self) -> Rect<u32> {
        self.bounds
    }
}

impl<TIncluded: UncheckedCast<u64>, TExcluded: UncheckedCast<u64>, TMeta> IntoIterator
    for SortedRangesMap<TIncluded, TExcluded, Vec<TMeta>>
{
    type Item = MetaRange<NonZeroRange<u64>, TMeta>;
    type IntoIter = SortedRangesMapIter<
        std::vec::IntoIter<TIncluded>,
        std::vec::IntoIter<TExcluded>,
        std::vec::IntoIter<TMeta>,
        NonZeroRange<u64>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        SortedRangesMapIter::new(
            self.included.into_iter(),
            self.excluded.into_iter(),
            self.meta.into_iter(),
            0,
        )
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub struct MetaRange<TRange, TMeta> {
    pub range: TRange,
    pub meta: TMeta,
}

impl<TRange, TMeta> From<(TRange, TMeta)> for MetaRange<TRange, TMeta> {
    fn from((range, meta): (TRange, TMeta)) -> Self {
        Self { range, meta }
    }
}

impl<TMeta> MetaRange<NonZeroRange<u64>, TMeta> {
    pub fn copy_with_offset(&self, offset: i64) -> Self
    where
        TMeta: Copy,
    {
        Self {
            range: self.range.with_offset(offset),
            meta: self.meta,
        }
    }

    pub fn clone_with_offset(&self, offset: i64) -> Self
    where
        TMeta: Clone,
    {
        Self {
            range: self.range.with_offset(offset),
            meta: self.meta.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZero, ops::RangeInclusive};

    use crate::ImaskSet;

    use super::*;

    fn test_bounds() -> Rect<u32> {
        Rect::new(
            0,
            0,
            NonZero::new(1000u32).unwrap(),
            NonZero::new(1000u32).unwrap(),
        )
    }

    #[cfg(feature = "range-set-blaze-0_5")]
    mod blaze {
        use super::*;
        use range_set_blaze_0_5::{SortedDisjointMap, ValueRef};
        #[derive(PartialEq, Eq, Clone, Debug)]
        struct TestMetaItem(&'static str);

        impl From<&'static str> for TestMetaItem {
            fn from(value: &'static str) -> Self {
                Self(value)
            }
        }

        impl ValueRef for TestMetaItem {
            type Target = TestMetaItem;

            fn into_value(self) -> Self::Target {
                self
            }
        }
        #[test]
        fn combine_owned() {
            let a = SortedRangesMap::<u8, u8, Vec<TestMetaItem>>::try_from_ordered_iter(
                [(10u32..30, "a_first".into()), (42..50, "a_second".into())]
                    .with_roi(test_bounds()),
            )
            .unwrap();
            let b = SortedRangesMap::<u8, u8, Vec<TestMetaItem>>::try_from_ordered_iter(
                [(20u32..30, "b_first".into()), (41..45, "b_second".into())]
                    .with_roi(test_bounds()),
            )
            .unwrap();

            let a_iter = a.iter_owned::<RangeInclusive<usize>>();
            let b_iter = b.iter_owned::<RangeInclusive<usize>>();
            let result = range_set_blaze_0_5::SortedDisjointMap::union(b_iter, a_iter)
                .map(|(r, m)| (*r.start()..(*r.end() + 1), m))
                .collect::<Vec<_>>();

            assert_eq!(
                vec![
                    (10usize..30, TestMetaItem::from("a_first")),
                    (41..42, TestMetaItem::from("b_second")),
                    (42..50, TestMetaItem::from("a_second"))
                ],
                result
            );
        }
        #[test]
        fn combine_inline() {
            use range_set_blaze_0_5::SortedDisjointMap;

            let a = SortedRangesMap::<u8, u8, Vec<TestMetaItem>>::try_from_ordered_iter(
                [(10u32..30, "a_first".into()), (42..50, "a_second".into())]
                    .with_roi(test_bounds()),
            )
            .unwrap();
            let b = SortedRangesMap::<u8, u8, Vec<TestMetaItem>>::try_from_ordered_iter(
                [(20u32..30, "b_first".into()), (41..45, "b_second".into())]
                    .with_roi(test_bounds()),
            )
            .unwrap();

            let b_iter = b.iter_owned::<RangeInclusive<u64>>();
            let a = a
                .map_inplace(|a_iter| {
                    range_set_blaze_0_5::SortedDisjointMap::union(b_iter, a_iter)
                        .map(|(r, m)| (*r.start()..=(*r.end()), m))
                })
                .unwrap();

            assert_eq!(
                vec![
                    (10u64..30, TestMetaItem::from("a_first")),
                    (41..42, TestMetaItem::from("b_second")),
                    (42..50, TestMetaItem::from("a_second"))
                ],
                a.iter_owned::<Range<u64>>().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn ranges_starting_at_zero() {
        let map = SortedRangesMap::<u32, u32, Vec<&str>>::try_from_ordered_iter(
            [(0u64..1, "first"), (5u64..6, "second")].with_roi(test_bounds()),
        );

        let map = map.unwrap();
        let collected: Vec<_> = map.iter::<std::ops::Range<u64>>().map(|x| x.0).collect();
        assert_eq!(vec![0u64..1, 5u64..6], collected);
    }

    #[test]
    fn range_with_initial_offset() {
        let encoded = SortedRangesMap::<u8, u8, _>::try_from_ordered_iter(
            [(10u32..20, "first"), (255..257, "second")].with_roi(test_bounds()),
        )
        .unwrap();
        assert_eq!(
            vec![(10u64..=19, &"first"), (255u64..=256, &"second")],
            encoded.iter::<RangeInclusive<u64>>().collect::<Vec<_>>()
        );
    }

    #[test]
    fn owned_iterator_inclusive() {
        let encoded = SortedRangesMap::<u8, u8, _>::try_from_ordered_iter(
            [
                (10u32..20, "first".to_string()),
                (255..257, "second".to_string()),
            ]
            .with_roi(test_bounds()),
        )
        .unwrap();
        let collected: Vec<_> = encoded.iter_owned::<RangeInclusive<u64>>().collect();
        assert_eq!(10u64..=19, collected[0].0);
        assert_eq!("first", collected[0].1);
        assert_eq!(255u64..=256, collected[1].0);
        assert_eq!("second", collected[1].1);
        assert_eq!(2, collected.len());
    }
    #[test]
    fn owned_iterator() {
        let encoded = SortedRangesMap::<u8, u8, _>::try_from_ordered_iter(
            [
                (10u32..20, "first".to_string()),
                (255..257, "second".to_string()),
            ]
            .with_roi(test_bounds()),
        )
        .unwrap();
        let collected: Vec<_> = encoded.into_iter().collect();
        assert_eq!(2, collected.len());
        assert_eq!(
            NonZeroRange::from_span(10, const { NonZero::new(10).unwrap() }),
            collected[0].range
        );
        assert_eq!("first", collected[0].meta);
        assert_eq!(
            NonZeroRange::from_span(255, const { NonZero::new(2).unwrap() },),
            collected[1].range
        );
        assert_eq!("second", collected[1].meta);
    }

    #[test]
    fn assert_big_gap_causes_error() {
        let error = SortedRangesMap::<u16, u8, _>::try_from_ordered_iter(
            [(10u32..20, "first"), (276..280, "second")].with_roi(test_bounds()),
        )
        .unwrap_err();
        assert!(error.contains("out of range"), "{error}");
    }

    #[test]
    fn assert_big_ranges_cause_error() {
        let error = SortedRangesMap::<u8, u16, _>::try_from_ordered_iter(
            [(10u32..280, "first")].with_roi(test_bounds()),
        )
        .unwrap_err();
        assert!(error.contains("out of range"), "{error}");
    }
    #[test]
    fn zero_ranges_cause_error() {
        let error = SortedRangesMap::<u8, u8, _>::try_from_ordered_iter(
            [(10u32..10, "first")].with_roi(test_bounds()),
        )
        .unwrap_err();
        assert!(error.contains("> 10"), "{error}");
    }

    #[test]
    fn overlapping_cause_error() {
        let error = SortedRangesMap::<u8, u8, _>::try_from_ordered_iter(
            [(10u32..12, "first"), (11..12, "second")].with_roi(test_bounds()),
        )
        .unwrap_err();
        assert!(error.contains("> 12"), "{error}");
    }

    #[test]
    fn split_combine() {
        let a = SortedRangesMap::<u8, u8, Vec<String>>::try_from_ordered_iter(
            [(10u32..15, "a1".to_string()), (30..35, "a2".to_string())].with_roi(test_bounds()),
        )
        .unwrap();

        let a = a
            .map_inplace(|iter| {
                iter.map(|(x, m)| {
                    let (start, end) = x.into_inner();
                    ((start + 5)..=(end + 5), m)
                })
            })
            .unwrap();

        assert_eq!(
            vec![(15u64..=19, "a1"), (35..=39, "a2")],
            a.iter::<RangeInclusive<u64>>()
                .map(|(r, m)| (r, m.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn split_when_collection_becomes_bigger() {
        let a = SortedRangesMap::<u8, u8, Vec<String>>::try_from_ordered_iter(
            [
                (10u32..15, "first".to_string()),
                (30..35, "second".to_string()),
            ]
            .with_roi(test_bounds()),
        )
        .unwrap();

        let a = a
            .map_inplace(|iter| {
                iter.flat_map(|(x, m)| {
                    let with_offset = (*x.start() + 10)..=(*x.end() + 10);
                    [(x, m.clone()), (with_offset, format!("{}_offset", m))]
                })
            })
            .unwrap();

        assert_eq!(
            vec![
                (10u64..=14, "first"),
                (20..=24, "first_offset"),
                (30..=34, "second"),
                (40..=44, "second_offset")
            ],
            a.iter::<RangeInclusive<u64>>()
                .map(|(r, m)| (r, m.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn split_returns_none_when_empty() {
        let a = SortedRangesMap::<u8, u8, Vec<String>>::try_from_ordered_iter(
            [(10u32..15, "test".to_string())].with_roi(test_bounds()),
        )
        .unwrap();

        let result = a.map_inplace(|_| std::iter::empty());

        assert!(result.is_none());
    }
}
