use crate::{CreateRange, SortedRanges};
use std::{io, marker::PhantomData};

pub struct ReaderRangeIterator<T, TRange> {
    reader: T,
    _phantom: PhantomData<TRange>,
}

impl<TRead, TRange> ReaderRangeIterator<TRead, TRange> {
    pub fn try_new(read: TRead) -> io::Result<Self> {
        todo!("Read the Header here")
    }
}

impl<TRead, TRange: CreateRange> Iterator for ReaderRangeIterator<TRead, TRange> {
    type Item = TRange;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

impl<TIncluded, TExcluded> SortedRanges<TIncluded, TExcluded> {
    pub fn from_serialized(input: &[u8]) -> std::io::Result<Self> {
        // let ch = std::pin::pin!(futures::executor::block_on(AsyncRangeStream::new(
        //     futures::io::Cursor::new(&f)
        // ))?);
        // futures::executor::block_on_stream(ch).process_results(|x| {
        //     for range in x.with_roi(roi).try_clip_2d(bounds)? {
        //         let range = range.start as usize..range.end as usize;
        //         let dst = mask_slice
        //             .get_mut(range.clone())
        //             .ok_or_else(|| anyhow!("Mask-Range out of bound: {range:?}"))?;
        //         dst.fill(255);
        //     }
        //     anyhow::Ok(())
        // })??;
        todo!(
            "Not implemented... Should avoid the roundtrip to Ranges, as the binary format and SortedRanges both store offsets (paris of offset, len)"
        );
    }
}
