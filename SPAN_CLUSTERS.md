Plan a new Span-Combinator, which groups all neighbouring spans. Neighbouring is also satisfied, if Pixels touch diagonal, no direct top,bottom,right,left contact required. Consider the special case and write one test, where two not connected ranges suddenly get connected by a span which covers both. Don't just collect all Groups in a big Collection, but yield Groups of spans (Which themself have to implement ImageDimension) as soon as they cannot be connected to
anything anymore. Pay attention to efficiency (e.g. don't just delete things out of a array, but think, if swap_remove is also correct), use the best datastructures for the job.

Write just a few, but meaningful tests: One with all possible only diagonal connections, which should be merged. Another one for initiali disconnected clusters which later meet: Add three spans in the second line, repeat the first and third in the third line and connect the first and the third in the third line. 


SpanCluster must implement ImageDimension. What should width() report?
Track min/max x/y for each (even incomplete) Cluster and adjust it for each pushed new span.
May we assume within-row spans are already unioned (non-overlapping), or must we handle touching spans in the same row?
Assume pre-merged input only
Preferred names for the combinator type and ImaskSet method?
ClusterSpanIter + cluster() (Recommended)

Another note: Spans in SpanCluster also have to be sorted... You cannot simply push Elements of one SpanCluster at the end of the other, but you must Union them.. As this is a expensive operation, i want you to Merge the elements from the Smaller Cluster into the bigger one. Keep a SpanClusterIterator wide Cache (Vec?) to move big cluster elements after the first has to be inserted from SmallCluster, so you can efficiently merge into the original (bigger) cluster.

The cache should be reused for all merges(Don't just swap it with a empty one, but use iter (as span is copy => you should add a Copy bound to T), not just for two clusters, but for every pair of clusters to be merged... Everything clear?

Also keep the Pending Clusters sorted, or at least in a Heap if this is impractical, so you can efficiently handle thousands of pending groups (and thus thousands of spans in the next row, which should be attached to the pending clusters..



Assume, Clusters are NEVER empty (similar to SortedRanges)... Then, you don't have to use frontier_lo and hi, because this is just the last tem in spans... MaxRow is not necessary, as it's the same as max_y... 

use a vec-deques... Always take with pop_front (yield it, if next span from input is horizontally after its last x-end)... If you merged a span item to it, push_back to the others...

Add a test which verifies, that a span in line 1 and line 3 are not merged, if line 2 is empty, even if they horizontally overlap



DON't scan all frontier-row clusters... But correctness is very important.. Don't just look pending Clusters last span, but all spans on the previous line... If you found it overlapping in the e.g. second or third last item, don't use push_back, but push_front and swap Dequeue-elements, until it's sorted again... Also add a test case Spans which form a stroke square(not filled), which is not closed on the bottom-right


# Questions
Confirm the deque is sorted purely by spans.last().x.end (mixing max_y==Y-1 and matched max_y==Y clusters), so clusters with remaining frontier stay reachable.
Order by the first item of the current y... Otherwise, you might miss merges... Add a closed stroke square to the tests too to verify this works...
A done cluster may be yielded slightly late (blocked behind a not-done cluster with smaller last.x.end). Acceptable, or must yields be strictly immediate?
Accept bounded delay (Recommended)

Store a VecDeque<(usize, Cluster<T>)>, where usize points to the element, which has to be checked next... When it's not the last increment + push_front+ swap, if it was the last, use the faster push_back...
