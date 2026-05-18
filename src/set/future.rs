use std::{
    fmt::Display,
    io,
    task::{Poll::Ready, ready},
};

use futures_core::Stream;

use crate::{CreateRange, ImageDimension};

use super::{Builder, SortedRanges};

impl<TIncluded, TExcluded> SortedRanges<TIncluded, TExcluded> {
    pub fn try_from_ordered_stream<TStream, T>(
        stream: TStream,
    ) -> TryFromOrderedStreamFuture<TStream, TIncluded, TExcluded>
    where
        TStream: Stream<Item = io::Result<T>>,
        T: CreateRange<Item: TryInto<u64, Error: Display>>,
        TIncluded: TryFrom<u64, Error: Display>,
        TExcluded: TryFrom<u64, Error: Display>,
    {
        TryFromOrderedStreamFuture {
            stream,
            builder: None,
        }
    }
}
pin_project_lite::pin_project!(
    pub struct TryFromOrderedStreamFuture<S, TIncluded, TExcluded> {
        #[pin]
        stream: S,
        builder: Option<Builder<TIncluded, TExcluded>>,
    }
);
impl<S, T, TIncluded, TExcluded> std::future::Future
    for TryFromOrderedStreamFuture<S, TIncluded, TExcluded>
where
    S: Stream<Item = io::Result<T>> + ImageDimension,
    T: CreateRange<Item: TryInto<u64, Error: Display>>,
    TIncluded: TryFrom<u64, Error: Display>,
    TExcluded: TryFrom<u64, Error: Display>,
{
    type Output = std::io::Result<SortedRanges<TIncluded, TExcluded>>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        if this.builder.is_none() {
            let size_hint = this.stream.size_hint().0;
            match ready!(this.stream.as_mut().poll_next(cx)) {
                Some(Ok(first_range)) => match Builder::new(first_range, size_hint) {
                    Ok(x) => *this.builder = Some(x),
                    Err(e) => return Ready(Err(e)),
                },
                Some(Err(e)) => return Ready(Err(e)),
                None => {
                    return Ready(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Requires at least one item",
                    )));
                }
            }
        };
        loop {
            match ready!(this.stream.as_mut().poll_next(cx)) {
                Some(Ok(x)) => {
                    let builder = this
                        .builder
                        .as_mut()
                        .expect("Created if non existend... Lifetime issue");
                    if let Err(e) = builder.add(x) {
                        return Ready(Err(e));
                    }
                }
                Some(Err(e)) => return Ready(Err(e)),
                None => {
                    let width = this.stream.width();
                    return Ready(this.builder.take().unwrap().build_global(width));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use testresult::TestResult;

    use crate::{ImaskSet, WithBounds};

    use super::*;

    const NON_ZERO_1000: NonZero<u32> = NonZero::new(1000u32).unwrap();

    #[tokio::test]
    async fn try_from_stream() -> TestResult {
        let ranges_array = [0u16..10, 16..2020];
        let stream_ranges = SortedRanges::<u64, u64>::try_from_ordered_stream(WithBounds::new(
            futures_util::stream::iter(ranges_array.iter().map(|x| Ok(x.clone()))),
            NON_ZERO_1000,
            NON_ZERO_1000,
        ))
        .await?;
        let iter_ranges = SortedRanges::<u64, u64>::try_from_ordered_iter(
            ranges_array.with_bounds(NON_ZERO_1000, NON_ZERO_1000),
        )?;
        assert_eq!(stream_ranges, iter_ranges);
        Ok(())
    }
}
