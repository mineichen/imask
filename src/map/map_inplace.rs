use std::{
    cell::RefCell, collections::VecDeque, fmt::Debug, iter::FusedIterator, num::NonZero,
    ops::RangeInclusive, rc::Rc,
};

use crate::{CreateRange, RangeToOffsetsIterMap, SortedRangesMap, UncheckedCast};
impl<TIncluded: UncheckedCast<u64>, TExcluded: UncheckedCast<u64>, TMeta: Debug>
    SortedRangesMap<TIncluded, TExcluded, Vec<TMeta>>
{
    pub fn map_inplace<TIter, TFun>(self, f: TFun) -> Option<Self>
    where
        TIter: Iterator<Item = (RangeInclusive<u64>, TMeta)>,
        TFun: FnOnce(SourceIteratorMap<TIncluded, TExcluded, TMeta>) -> TIter,
        TIncluded: TryFrom<u64, Error: Debug> + Clone,
        TExcluded: TryFrom<u64, Error: Debug> + Clone,
        TMeta: Clone,
    {
        let original_len = self.included.len();
        let cell = Rc::new(RefCell::new((self, 0usize)));

        let source = SourceIteratorMap {
            cell: cell.clone(),
            offset: 0,
            original_len,
        };

        let items = f(source);
        let offsets_iter = RangeToOffsetsIterMap::<_, TIncluded, TExcluded, TMeta>::new(items);
        let mut cache: VecDeque<(TExcluded, TIncluded, TMeta)> = VecDeque::new();
        let mut write_pos = 0;

        let write_tuple =
            |col: &mut SortedRangesMap<_, _, Vec<_>>, (excl, incl, meta), write_pos: &mut usize| {
                if *write_pos < col.included.len() {
                    col.excluded[*write_pos] = excl;
                    col.included[*write_pos] = incl;
                    col.meta[*write_pos] = meta;
                } else {
                    col.excluded.push(excl);
                    col.included.push(incl);
                    col.meta.push(meta);
                }
                *write_pos += 1;
            };

        for tuple in offsets_iter {
            let mut x = cell.borrow_mut();
            let (read_pos, col) = (x.1, &mut x.0);
            if (write_pos < read_pos || read_pos >= original_len) && cache.is_empty() {
                write_tuple(col, tuple, &mut write_pos);
            } else {
                cache.push_back(tuple);
                while (write_pos < read_pos || read_pos >= original_len)
                    && let Some(t) = cache.pop_front()
                {
                    write_tuple(col, t, &mut write_pos)
                }
            }
        }

        let not_empty = {
            let mut x = cell.borrow_mut();
            let col = &mut x.0;
            while let Some(tuple) = cache.pop_front() {
                write_tuple(col, tuple, &mut write_pos);
            }

            col.included.truncate(write_pos);
            col.excluded.truncate(write_pos);
            col.meta.truncate(write_pos);
            !x.0.included.is_empty()
        };
        not_empty.then(move || {
            Rc::try_unwrap(cell)
                .expect("You are not allowed to move SourceIter outside the lambda")
                .into_inner()
                .0
        })
    }
}

pub struct SourceIteratorMap<TIncluded, TExcluded, TMeta> {
    #[allow(clippy::type_complexity)]
    cell: Rc<RefCell<(SortedRangesMap<TIncluded, TExcluded, Vec<TMeta>>, usize)>>,
    offset: u64,
    original_len: usize,
}

unsafe impl<TIncluded: Send, TExcluded: Send, TMeta: Send> Send
    for SourceIteratorMap<TIncluded, TExcluded, TMeta>
{
}

impl<TIncluded, TExcluded, TMeta> FusedIterator for SourceIteratorMap<TIncluded, TExcluded, TMeta> where
    Self: Iterator
{
}

impl<TIncluded, TExcluded, TMeta> Iterator for SourceIteratorMap<TIncluded, TExcluded, TMeta>
where
    TIncluded: UncheckedCast<u64>,
    TExcluded: UncheckedCast<u64>,
    TMeta: Clone,
{
    type Item = (RangeInclusive<u64>, TMeta);

    fn next(&mut self) -> Option<Self::Item> {
        let mut x = self.cell.borrow_mut();
        let (col, read_pos) = &mut *x;
        if *read_pos >= self.original_len {
            return None;
        }
        let exclude = (*col.excluded.get(*read_pos)?).cast_unchecked();
        self.offset += exclude;

        let include = (*col.included.get(*read_pos)?).cast_unchecked();
        let out_range =
            RangeInclusive::new_debug_checked(self.offset, NonZero::new(include).unwrap());
        self.offset += include;

        // Todo: Unsafe MaybeInit code allows reuse of this...
        // We should be able to reduce cloning to a minimum
        let meta = col.meta[*read_pos].clone();
        *read_pos += 1;

        Some((out_range, meta))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let read_pos = self.cell.borrow().1;
        let remaining = self.original_len.saturating_sub(read_pos);
        (remaining, Some(remaining))
    }
}

#[cfg(feature = "range-set-blaze-0_5")]
mod range_set_blaze_0_5_interop {

    use super::*;
    use range_set_blaze_0_5::{SortedDisjointMap, SortedStartsMap, ValueRef};

    impl<TIncluded, TExcluded, TMeta> SortedStartsMap<u64, TMeta>
        for SourceIteratorMap<TIncluded, TExcluded, TMeta>
    where
        TIncluded: UncheckedCast<u64>,
        TExcluded: UncheckedCast<u64>,
        TMeta: ValueRef,
    {
    }

    impl<TIncluded, TExcluded, TMeta> SortedDisjointMap<u64, TMeta>
        for SourceIteratorMap<TIncluded, TExcluded, TMeta>
    where
        TIncluded: UncheckedCast<u64>,
        TExcluded: UncheckedCast<u64>,
        TMeta: ValueRef,
    {
    }
}
