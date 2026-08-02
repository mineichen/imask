use std::{
    collections::VecDeque,
    fmt::Debug,
    iter::FusedIterator,
    num::NonZero,
    ops::{Add, Sub},
};

use num_traits::One;

use crate::{ImageDimension, Rect, Span, UncheckedCast};

/// Iterator-combinator that groups neighbouring [`Span`]s into [`SpanCluster`]s.
///
/// Two spans are considered neighbours when their pixels touch directly (top/
/// bottom/left/right) **or** diagonally (8-connectivity). Each [`SpanCluster`]
/// is yielded as soon as it can no longer be connected to by any future span.
///
/// The input is expected to be a sorted span iterator (sorted by `(y, x.start)`,
/// non-overlapping within each row — i.e. pre-merged).
///
/// `pending` holds the clusters that are still "open" (could be extended by a
/// later span). Each cluster carries a cursor (`check_idx`) into its `spans`,
/// pointing at the next previous-row ("frontier") span that still has to be
/// checked against the sweep. As the sweep advances, the cursor is moved past
/// spans that have either become stale (their row is more than one above the
/// current span) or have been passed (they lie completely to the left of the
/// current span and every future span). Once no frontier span remains, the
/// cluster is final and is emitted when it reaches the front of the queue.
pub struct ClusterSpanIter<I, T> {
    parent: I,
    pending: VecDeque<Cluster<T>>,
    /// Iterator-wide scratch buffer reused for every pairwise merge
    /// (`small`-into-`big`). Cleared, never replaced, so its allocation is
    /// amortised across all merges.
    merge_cache: Vec<Span<T>>,
    pending_item: Option<Span<T>>,
}

impl<I, T> ClusterSpanIter<I, T> {
    pub fn new(parent: I) -> Self {
        Self {
            parent,
            pending: VecDeque::new(),
            merge_cache: Vec::new(),
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
        let mut maybe_span = self.pending_item.take().or_else(|| self.parent.next());
        while let Some(span) = maybe_span {
            let connects = connects_to(&mut self.pending, span);
            #[cfg(debug_assertions)]
            {
                let mut iter = self.pending.iter().map(|x| x.spans[x.check_idx]);
                if let Some(x) = iter.next() {
                    iter.fold(x, |before, next| {
                        assert!(before < next);
                        next
                    });
                }
            }
            match connects {
                Ok(0) => {
                    self.pending.push_back(Cluster::from_span(span));
                }
                Ok(x) => {
                    let mut iter = self.pending.iter_mut().take(x).rev();
                    let remain = iter.next().expect("Has more than 0");
                    for c in iter {
                        Cluster::merge_into(remain, c.take(), &mut self.merge_cache);
                    }
                    for _ in 1..x {
                        self.pending.pop_front();
                    }
                }
                Err(x) => {
                    self.pending_item = Some(span);
                    return Some(x.into_group());
                }
            }

            maybe_span = self.parent.next();
        }

        Some(self.pending.pop_front()?.into_group())
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

/// Advances the cursor past spans that can no longer connect to `span` (or
/// to any later, even further-right input span), then reports whether the
/// cluster's current frontier span touches `span`.
///
/// A span can only connect to inputs exactly one row below it
/// (`f.y + 1 == span.y`); for those, 8-connectivity (including the diagonal
/// neighbours) reduces to an x-overlap of the inclusive expansion, i.e.
/// `f.x.start <= span.x.end && span.x.start <= f.x.end`.
fn connects_to<T>(clusters: &mut VecDeque<Cluster<T>>, span: Span<T>) -> Result<usize, Cluster<T>>
where
    T: Ord + Copy + Debug + Add<Output = T> + Sub<Output = T> + One + UncheckedCast<u32>,
{
    let mut i = 0;
    loop {
        let mut iter = clusters.iter_mut().skip(i);
        let Some(mut first) = iter.next() else {
            return Ok(i);
        };
        let mut count = 0;
        let first_not_out = first.spans[first.check_idx..]
            .iter()
            .take_while(|f| {
                f.y + T::one() < span.y || f.y + T::one() == span.y && f.x.end < span.x.start
            })
            .inspect(|_| count += 1)
            .last();
        if let Some(f) = first_not_out {
            first.check_idx += count;

            let inside_span =
                f.y + T::one() == span.y && f.x.start <= span.x.end && span.x.start <= f.x.end;
            let mut moved = false;
            while let Some(x) = iter.next()
                && x.spans[x.check_idx] > first.spans[first.check_idx]
            {
                std::mem::swap(x, first);
                first = x;
                moved = true;
            }
            if !inside_span {
                return Ok(i);
            } else if !moved {
                i += 1;
            }
        } else {
            return Err(clusters.pop_front().expect("Checked above"));
        }
    }
    // .next()
    // .map(|(i, f)| {
    //     first.check_idx += i;
    //     f.y + T::one() == span.y && f.x.start <= span.x.end && span.x.start <= f.x.end
    // })
    // .unwrap_or_default();
    // Ok(1)
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

    fn take(&mut self) -> Self {
        let mut spans = Vec::new();
        core::mem::swap(&mut spans, &mut self.spans);
        Self {
            spans,
            min_x: self.min_x,
            max_x: self.max_x,
            min_y: self.min_y,
            check_idx: self.check_idx,
        }
    }

    /// `span` is the most recently pulled input span, i.e. it is `>=` every span
    /// already present, so a plain push keeps `spans` sorted.
    fn add(&mut self, span: Span<T>) {
        self.min_x = self.min_x.min(span.x.start);
        self.max_x = self.max_x.max(span.x.end);
        self.spans.push(span);
    }

    /// Merge `small` into `big` keeping `big.spans` sorted, reusing `cache` as
    /// scratch (cleared before and after, never reallocated from empty).
    /// Only the tail of `big` from the first insertion point is moved to the
    /// cache, then the two sorted tails are merged back in place.
    fn merge_into(target: &mut Cluster<T>, mut src: Cluster<T>, cache: &mut Vec<Span<T>>) {
        if src.min_y < target.min_y {
            std::mem::swap(target, &mut src);
        }
        let new_check_idx = target.check_idx + src.check_idx;

        target.min_x = target.min_x.min(src.min_x);
        target.max_x = target.max_x.max(src.max_x);
        target.spans.reserve_exact(src.spans.len());

        let bi = target.spans.partition_point(|s| s <= &src.spans[0]);
        if bi >= target.spans.len() {
            target.spans.extend(src.spans.iter().copied());
            return;
        }

        cache.extend_from_slice(&target.spans[bi..]);
        target.spans.truncate(bi);

        let mut si = 0;
        let mut ci = 0;
        while si < src.spans.len() && ci < cache.len() {
            if src.spans[si] < cache[ci] {
                target.spans.push(src.spans[si]);
                si += 1;
            } else {
                target.spans.push(cache[ci]);
                ci += 1;
            }
        }
        target.spans.extend_from_slice(&src.spans[si..]);
        target.spans.extend_from_slice(&cache[ci..]);
        target.check_idx = new_check_idx;
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
    fn keep_separate() {
        let spans = vec![
            Span::new(0u32..2, 0),
            Span::new(8u32..10, 0),
            Span::new(1u32..2, 1),
            Span::new(8u32..10, 1),
        ];
        let groups = run(spans);
        assert_eq!(
            groups.len(),
            2,
            "expected a single merged cluster: {groups:?}"
        );
        assert_eq!(groups[0].len(), 2);
        assert_eq!(groups[1].len(), 2);
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
