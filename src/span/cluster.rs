use std::{
    collections::VecDeque,
    fmt::Debug,
    iter::FusedIterator,
    num::NonZero,
    ops::{Add, Sub},
};

use num_traits::One;

use crate::{ImageDimension, Rect, Span, UncheckedCast, span};

/// Iterator-combinator that groups neighbouring [`Span`]s into [`SpanCluster`]s.
///
/// Two spans are considered neighbours when their pixels touch directly (top/
/// bottom/left/right) **or** diagonally (8-connectivity). Each [`SpanCluster`]
/// is yielded as soon as it can no longer be connected to by any future span.
///
/// The input is expected to be a sorted span iterator (sorted by `(y, x.start)`,
/// non-overlapping within each row — i.e. pre-merged).
///
/// `pending` is a `VecDeque` of `(cursor, Cluster)` pairs. The `cursor` is an
/// index into the cluster's `spans` pointing at the next previous-row
/// ("frontier") span that must be checked against the sweep. The deque is kept
/// sorted by that frontier span's `x.start`, so each input span only touches
/// the relevant front clusters. When a frontier span has been passed it is
/// consumed by advancing the cursor; if more frontier remains the cluster is
/// reinserted at its sorted position (`push_front` + adjacent swaps), otherwise
/// it is retired to the back (`push_back`).
pub struct ClusterSpanIter<I, T> {
    parent: I,
    pending: VecDeque<Cluster<T>>,
    closed: VecDeque<Cluster<T>>,
    /// Iterator-wide scratch buffer reused for every pairwise merge
    /// (`small`-into-`big`). Cleared, never replaced, so its allocation is
    /// amortised across all merges.
    merge_cache: Vec<Span<T>>,
    current_row: Option<T>,
    pending_item: Option<Span<T>>,
}

impl<I, T> ClusterSpanIter<I, T> {
    pub fn new(parent: I) -> Self {
        Self {
            parent,
            pending: VecDeque::new(),
            closed: VecDeque::new(),
            merge_cache: Vec::new(),
            current_row: None,
            pending_item: None,
        }
    }
}

impl<I, T> ImageDimension for ClusterSpanIter<I, T>
where
    I: ImageDimension,
{
    fn bounds(&self) -> Rect<u32> {
        self.parent.bounds()
    }
    fn width(&self) -> NonZero<u32> {
        self.parent.width()
    }
}

impl<I, T> Iterator for ClusterSpanIter<I, T>
where
    I: Iterator<Item = Span<T>> + FusedIterator,
    T: Ord + Copy + Debug + Add<Output = T> + Sub<Output = T> + One + UncheckedCast<u32>,
{
    type Item = SpanCluster<T>;

    fn next(&mut self) -> Option<SpanCluster<T>> {
        loop {
            let Some(span) = self.pending_item.take().or_else(|| self.parent.next()) else {
                return Some(self.pending.pop_front()?.into_group());
            };

            // let mut maybe_unfinished_idx = 0;
            match next_mergable_if_no_empty(&mut self.pending, 0, span) {
                Ok(mut first) => {
                    first.add(span);
                    let mut i = 0;
                    loop {
                        match next_mergable_if_no_empty(&mut self.pending, i, span) {
                            Ok(mut x) => {
                                if x.spans.len() > first.spans.len() {
                                    std::mem::swap(&mut first, &mut x);
                                }
                                Cluster::merge_into(&mut first, x, &mut self.merge_cache);
                            }
                            Err(Some(x)) => {
                                // Not optimal, but should be very rare in practice (interwoven but separated regions)
                                self.pending.push_front(x);
                                i += 1;
                            }
                            Err(None) => break,
                        }
                    }
                    return Some(first.into_group());
                }
                Err(None) => self.pending.push_back(Cluster::from_span(span)),
                // All span's of r are before span
                Err(Some(r)) => {
                    self.pending_item = Some(span);
                    return Some(r.into_group());
                }
            }
        }
    }
}
// Gets the first cluster, which might be merged with span
/// To do so, it can increase Cluster::check_idx if there are more and items and fix the ordering within data
/// Errors if
/// - there are no items in data (Err(None))
/// - a cluster is entirely before span
fn next_mergable_if_no_empty<'a, T>(
    data: &'a mut VecDeque<Cluster<T>>,
    start: usize,
    span: Span<T>,
) -> Result<Cluster<T>, Option<Cluster<T>>>
where
    T: Ord + Copy + Debug + Add<Output = T> + Sub<Output = T> + One + UncheckedCast<u32>,
{
    let Some(mut first) = data.remove(start) else {
        return Err(None);
    };
    let mut iter = data.iter_mut().skip(start);

    loop {
        let s = first.check_span();
        if s.y == span.y + T::one() && s.x.end + T::one() > span.x.start {}
        if let Some(rest) = first.spans.get_mut(first.check_idx + 1..)
            && !rest.iter_mut().find(|s| true).is_some()
        {
            return Err(Some(first));
        }
    }
}

impl<I, T> FusedIterator for ClusterSpanIter<I, T>
where
    I: Iterator<Item = Span<T>> + FusedIterator,
    T: Ord + Copy + Debug + Add<Output = T> + Sub<Output = T> + One + UncheckedCast<u32>,
{
}

/// A group of neighbouring spans. Implements [`ImageDimension`] (tight bounding
/// box of its spans; `width() == bounds().width`) and drains its spans via
/// [`Iterator`]. Iterating it consumes the group.
pub struct SpanCluster<T> {
    spans: std::vec::IntoIter<Span<T>>,
    bounds: Rect<u32>,
}

impl<T> Iterator for SpanCluster<T> {
    type Item = Span<T>;
    fn next(&mut self) -> Option<Span<T>> {
        self.spans.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.spans.size_hint()
    }
}

impl<T> ImageDimension for SpanCluster<T> {
    fn bounds(&self) -> Rect<u32> {
        self.bounds
    }
    fn width(&self) -> NonZero<u32> {
        self.bounds.width
    }
}

struct Cluster<T> {
    /// Sorted by `(y, x.start)`; never empty.
    spans: Vec<Span<T>>,
    min_x: T,
    max_x: T,
    min_y: T,
    check_idx: usize,
}

impl<T> Cluster<T>
where
    T: Ord + Copy + Debug + Add<Output = T> + Sub<Output = T> + One + UncheckedCast<u32>,
{
    fn from_span(span: Span<T>) -> Self {
        Self {
            spans: vec![span],
            min_x: span.x.start,
            max_x: span.x.end,
            min_y: span.y,
            check_idx: 0,
        }
    }

    fn check_span(&self) -> Span<T> {
        self.spans[self.check_idx]
    }

    // It's not certain that it does, but it's possible
    fn might_current_be_merged(&self, next: Span<T>) -> bool {
        let prev = self.check_span();
        next.y == prev.y + T::one() && next.x.start > prev.x.start
    }

    /// `span` is the most recently pulled input span, i.e. it is `>=` every span
    /// already present, so a plain push keeps `spans` sorted.
    fn add(&mut self, span: Span<T>) {
        if self.min_x > span.x.start {
            self.min_x = span.x.start;
        }
        if self.max_x < span.x.end {
            self.max_x = span.x.end;
        }
        if self.min_y > span.y {
            self.min_y = span.y;
        }
        self.spans.push(span);
    }

    /// Merge `small` into `big` keeping `big.spans` sorted, reusing `cache` as
    /// scratch (cleared before and after, never reallocated from empty).
    /// Only the tail of `big` from the first insertion point is moved to the
    /// cache, then the two sorted tails are merged back in place.
    fn merge_into(big: &mut Cluster<T>, small: Cluster<T>, cache: &mut Vec<Span<T>>) {
        if big.min_x > small.min_x {
            big.min_x = small.min_x;
        }
        if big.max_x < small.max_x {
            big.max_x = small.max_x;
        }
        if big.min_y > small.min_y {
            big.min_y = small.min_y;
        }

        let bi = big.spans.partition_point(|s| s <= &small.spans[0]);
        if bi >= big.spans.len() {
            big.spans.extend(small.spans.iter().copied());
            return;
        }

        cache.clear();
        cache.extend(big.spans[bi..].iter().copied());
        big.spans.truncate(bi);

        let mut si = 0;
        let mut ci = 0;
        while si < small.spans.len() && ci < cache.len() {
            if small.spans[si] < cache[ci] {
                big.spans.push(small.spans[si]);
                si += 1;
            } else {
                big.spans.push(cache[ci]);
                ci += 1;
            }
        }
        big.spans.extend(small.spans[si..].iter().copied());
        big.spans.extend(cache[ci..].iter().copied());
        cache.clear();
    }

    fn into_group(self) -> SpanCluster<T> {
        let max_y = self.spans.last().expect("never empty").y;
        let x = self.min_x.cast_unchecked();
        let y = self.min_y.cast_unchecked();
        let width = NonZero::new(self.max_x.cast_unchecked() - x)
            .expect("non-empty cluster has positive width");
        let height = NonZero::new(max_y.cast_unchecked() + 1 - y)
            .expect("non-empty cluster has positive height");
        SpanCluster {
            spans: self.spans.into_iter(),
            bounds: Rect::new(x, y, width, height),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use crate::{ImageDimension, ImaskSet, Rect, Span};

    use super::*;

    const W: NonZero<u32> = NonZero::new(100).unwrap();
    const H: NonZero<u32> = NonZero::new(100).unwrap();

    /// Collect all clusters, each as a sorted `Vec<Span<u32>>`, sorted across
    /// clusters by (min_y, min_x) so assertions are order-independent.
    fn collect_sorted(
        iter: ClusterSpanIter<impl std::iter::FusedIterator<Item = Span<u32>>, u32>,
    ) -> Vec<Vec<Span<u32>>> {
        let mut groups: Vec<Vec<Span<u32>>> = Vec::new();
        for cluster in iter {
            let _ = cluster.bounds();
            groups.push(cluster.collect());
        }
        groups.sort_by(|a, b| {
            a.first()
                .unwrap()
                .y
                .cmp(&b.first().unwrap().y)
                .then_with(|| a.first().unwrap().x.start.cmp(&b.first().unwrap().x.start))
        });
        groups
    }

    fn run(spans: Vec<Span<u32>>) -> Vec<Vec<Span<u32>>> {
        collect_sorted(spans.into_iter().with_bounds(W, H).cluster())
    }

    #[test]
    fn covering_span() {
        // row 0: two disconnected spans; row 1: a single span overlapping both.
        let spans = vec![
            Span::new(0u32..2, 0),
            Span::new(8u32..10, 0),
            Span::new(1u32..9, 1),
        ];
        let groups = run(spans);
        assert_eq!(
            groups.len(),
            1,
            "expected a single merged cluster: {groups:?}"
        );
        assert_eq!(groups[0].len(), 3);
    }

    #[test]
    fn diagonal_only_merged() {
        // Single-pixel staircase: each pair touches only diagonally.
        let spans = vec![
            Span::new(0u32..1, 0),
            Span::new(1u32..2, 1),
            Span::new(2u32..3, 2),
            Span::new(3u32..4, 3),
        ];
        let groups = run(spans);
        assert_eq!(groups.len(), 1, "diagonal chain must merge: {groups:?}");
        assert_eq!(groups[0].len(), 4);
    }

    #[test]
    fn row_gap_not_merged() {
        // A span in line 0 and line 2 (line 1 empty) must stay separate even
        // though their x-ranges are identical.
        let spans = vec![Span::new(0u32..5, 0), Span::new(0u32..5, 2)];
        let groups = run(spans);
        assert_eq!(groups.len(), 2, "row gap (>=2) must split: {groups:?}");
        assert_eq!(groups[0], vec![Span::new(0u32..5, 0)]);
        assert_eq!(groups[1], vec![Span::new(0u32..5, 2)]);
    }

    #[test]
    fn disconnected_then_meet() {
        // row 1: three separate spans; row 2: one pre-merged span overlapping
        // all three → they merge into a single cluster.
        let spans = vec![
            Span::new(0u32..2, 1),
            Span::new(4u32..6, 1),
            Span::new(8u32..10, 1),
            Span::new(0u32..10, 2),
        ];
        let groups = run(spans);
        assert_eq!(groups.len(), 1, "three clusters should meet: {groups:?}");
        assert_eq!(groups[0].len(), 4);
    }

    #[test]
    fn stroke_square_closed() {
        // Closed hollow square outline (perimeter) → one cluster.
        let spans = vec![
            Span::new(0u32..4, 0),
            Span::new(0u32..1, 1),
            Span::new(3u32..4, 1),
            Span::new(0u32..1, 2),
            Span::new(3u32..4, 2),
            Span::new(0u32..4, 3),
        ];
        let groups = run(spans);
        assert_eq!(
            groups.len(),
            1,
            "closed stroke square is one cluster: {groups:?}"
        );
        assert_eq!(groups[0].len(), 6);
    }

    #[test]
    fn stroke_square_open_bottom_right() {
        // Same outline but the bottom-right is open (row 3 misses the right
        // part). The right column is still connected through the top → one
        // cluster. This exercises the multi-span-frontier / cursor matching:
        // the right column must not be missed.
        let spans = vec![
            Span::new(0u32..4, 0),
            Span::new(0u32..1, 1),
            Span::new(3u32..4, 1),
            Span::new(0u32..1, 2),
            Span::new(3u32..4, 2),
            Span::new(0u32..2, 3),
        ];
        let groups = run(spans);
        assert_eq!(
            groups.len(),
            1,
            "open-bottom-right stroke square is still one cluster: {groups:?}"
        );
        assert_eq!(groups[0].len(), 6);
    }

    #[test]
    fn cluster_has_image_dimension() {
        let spans = vec![Span::new(1u32..4, 2u32), Span::new(1u32..4, 3u32)];
        let mut iter = spans.into_iter().with_bounds(W, H).cluster();
        let cluster = iter.next().unwrap();
        let bounds = ImageDimension::bounds(&cluster);
        assert_eq!(
            bounds,
            Rect::new(1, 2, NonZero::new(3).unwrap(), NonZero::new(2).unwrap())
        );
        assert_eq!(ImageDimension::width(&cluster), NonZero::new(3).unwrap());
    }

    #[test]
    fn retired_cluster_is_not_lost() {
        // Two clusters in row 0. In row 1: the left cluster is matched then its
        // frontier is passed by the sweep (retired to the back); a new cluster
        // appears in the middle; the right cluster must still match its row-1
        // span. Verifies that retiring a passed cluster to the back (no `held`)
        // does not lose or skip the cluster behind it.
        let spans = vec![
            Span::new(0u32..2, 0),
            Span::new(10u32..12, 0),
            Span::new(0u32..2, 1),
            Span::new(5u32..6, 1),
            Span::new(10u32..12, 1),
        ];
        let groups = run(spans);
        assert_eq!(groups.len(), 3, "three clusters: {groups:?}");
        assert_eq!(
            groups[0],
            vec![Span::new(0u32..2, 0), Span::new(0u32..2, 1)]
        );
        assert_eq!(
            groups[1],
            vec![Span::new(10u32..12, 0), Span::new(10u32..12, 1)]
        );
        assert_eq!(groups[2], vec![Span::new(5u32..6, 1)]);
    }
}
