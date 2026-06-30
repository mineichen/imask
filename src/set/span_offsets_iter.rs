use std::{any::type_name, fmt::Debug, num::NonZero};

use crate::{ImageDimension, Span};

pub struct SpanToOffsetsIter<TIter, TIncluded, TExcluded> {
    iter: TIter,
    prev_end: u64,
    width: u64,
    buffered: Option<(u64, u64)>,
    _phantom: std::marker::PhantomData<(TIncluded, TExcluded)>,
}

impl<TIter, TIncluded, TExcluded> SpanToOffsetsIter<TIter, TIncluded, TExcluded> {
    pub fn new(iter: TIter, width: u32) -> Self {
        Self {
            iter,
            prev_end: 0,
            width: width as u64,
            buffered: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<TIter, TIncluded, TExcluded> Iterator for SpanToOffsetsIter<TIter, TIncluded, TExcluded>
where
    TIter: Iterator<Item = Span<u64>>,
    TIncluded: TryFrom<u64, Error: Debug>,
    TExcluded: TryFrom<u64, Error: Debug>,
{
    type Item = (TExcluded, TIncluded);

    fn next(&mut self) -> Option<Self::Item> {
        let (start, end) = if let Some(b) = self.buffered.take() {
            b
        } else {
            let span = self.iter.next()?;
            let start = span.y * self.width + span.x.start;
            let end = span.y * self.width + span.x.end;
            debug_assert!(
                start >= self.prev_end,
                "Spans must be sorted and non-overlapping: got start={start}, but prev_end={}",
                self.prev_end
            );
            (start, end)
        };

        let gap = start - self.prev_end;
        self.prev_end = end;
        let mut total_len = end - start;

        loop {
            let Some(span) = self.iter.next() else {
                break;
            };
            let next_start = span.y * self.width + span.x.start;
            let next_end = span.y * self.width + span.x.end;
            debug_assert!(
                next_start >= self.prev_end,
                "Spans must be sorted and non-overlapping: got next_start={next_start}, but prev_end={}",
                self.prev_end
            );
            if next_start == self.prev_end {
                total_len += next_end - next_start;
                self.prev_end = next_end;
            } else {
                self.buffered = Some((next_start, next_end));
                break;
            }
        }

        let excluded = gap.try_into().unwrap_or_else(|_| {
            panic!(
                "Gap of {} is too large to fit into {}",
                gap,
                type_name::<TExcluded>()
            );
        });
        let included = total_len.try_into().unwrap_or_else(|_| {
            panic!(
                "Span length {} is too large to fit into {}",
                total_len,
                type_name::<TIncluded>()
            );
        });
        Some((excluded, included))
    }
}

impl<TIter, TIncluded, TExcluded> ImageDimension for SpanToOffsetsIter<TIter, TIncluded, TExcluded>
where
    TIter: ImageDimension,
{
    fn width(&self) -> NonZero<u32> {
        self.iter.width()
    }

    fn bounds(&self) -> crate::Rect<u32> {
        self.iter.bounds()
    }
}
