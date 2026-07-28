use core::{fmt::Debug, marker::PhantomData, ops::RangeInclusive};

pub struct RangeToOffsetsIterMap<TIter, TIncluded, TExcluded, TMeta> {
    iter: TIter,
    prev_end: u64,
    _phantom: PhantomData<(TIncluded, TExcluded, TMeta)>,
}

impl<TIter, TIncluded, TExcluded, TMeta> RangeToOffsetsIterMap<TIter, TIncluded, TExcluded, TMeta> {
    pub fn new(iter: TIter) -> Self {
        Self {
            iter,
            prev_end: 0,
            _phantom: PhantomData,
        }
    }
}

impl<TIter, TIncluded, TExcluded, TMeta> Iterator
    for RangeToOffsetsIterMap<TIter, TIncluded, TExcluded, TMeta>
where
    TIter: Iterator<Item = (RangeInclusive<u64>, TMeta)>,
    TIncluded: TryFrom<u64, Error: Debug>,
    TExcluded: TryFrom<u64, Error: Debug>,
{
    type Item = (TExcluded, TIncluded, TMeta);

    fn next(&mut self) -> Option<Self::Item> {
        let (range, meta) = self.iter.next()?;
        let (start, end) = range.into_inner();
        let len = end - start + 1;
        let gap = start - self.prev_end;
        self.prev_end = start + len;
        let excluded = gap.try_into().unwrap();
        let included = len.try_into().unwrap();
        Some((excluded, included, meta))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}
