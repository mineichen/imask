use std::{
    future::Future,
    io::{self, ErrorKind},
    pin::Pin,
    task::{Context, Poll, ready},
};

use futures_io::{AsyncRead, AsyncWrite};
use pin_project_lite::pin_project;

use super::{HEADER_SIZE, U64_SIZE, header::Header, roi::Roi};
use crate::{CreateRange, ImageDimension, NonZeroRange, io::header::DataType};

fn write_u64(buf: &mut [u8], val: u64) {
    buf[..8].copy_from_slice(&val.to_le_bytes());
}
fn read_u64(buf: &[u8]) -> u64 {
    u64::from_le_bytes(buf[..8].try_into().unwrap())
}

fn unexpected_eof() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "Unexpected Eof")
}

fn poll_write_all<W: AsyncWrite + ?Sized>(
    mut writer: Pin<&mut W>,
    cx: &mut Context<'_>,
    buf: &[u8],
    offset: &mut usize,
) -> Poll<io::Result<()>> {
    while *offset < buf.len() {
        match ready!(writer.as_mut().poll_write(cx, &buf[*offset..]))? {
            0 => {
                return Poll::Ready(Err(unexpected_eof()));
            }
            n => *offset += n,
        }
    }
    Poll::Ready(Ok(()))
}

fn poll_read_exact<R: AsyncRead + ?Sized>(
    mut reader: Pin<&mut R>,
    cx: &mut Context<'_>,
    buf: &mut [u8],
    offset: &mut usize,
) -> Poll<io::Result<()>> {
    while *offset < buf.len() {
        match ready!(reader.as_mut().poll_read(cx, &mut buf[*offset..]))? {
            0 => return Poll::Ready(Err(unexpected_eof())),
            n => *offset += n,
        }
    }
    Poll::Ready(Ok(()))
}

trait IntoRangeResult {
    type Range: CreateRange<Item: Into<u64>>;
    fn into_range_result(self) -> io::Result<Self::Range>;
}

impl<T> IntoRangeResult for T
where
    T: CreateRange<Item: Into<u64>>,
{
    type Range = T;
    fn into_range_result(self) -> io::Result<Self::Range> {
        Ok(self)
    }
}

impl<T, E> IntoRangeResult for Result<T, E>
where
    T: CreateRange<Item: Into<u64>>,
    E: Into<io::Error>,
{
    type Range = T;
    fn into_range_result(self) -> io::Result<Self::Range> {
        self.map_err(Into::into)
    }
}

enum WriterState {
    Header,
    ReadRange,
    WriteBuf,
    Closing,
    Done,
}

pin_project! {
    pub struct AsyncRangeWriter<W, S> {
        #[pin] writer: W,
        #[pin] stream: S,
        state: WriterState,
        buf: [u8; HEADER_SIZE],
        pos: usize,
        len: usize,
        last_end: u64,
    }
}

impl<W, S> AsyncRangeWriter<W, S> {
    pub fn new(writer: W, stream: S) -> Self {
        Self {
            writer,
            stream,
            state: WriterState::Header,
            buf: [0; HEADER_SIZE],
            pos: 0,
            len: 0,
            last_end: 0,
        }
    }
}

impl<W, S> Future for AsyncRangeWriter<W, S>
where
    W: AsyncWrite,
    S: futures_core::Stream + ImageDimension,
    S::Item: IntoRangeResult,
{
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let roi = this.stream.bounds();
        let roi = Roi {
            offset_x: roi.x,
            offset_y: roi.y,
            width: roi.width,
            height: roi.height,
        };
        loop {
            match &mut this.state {
                WriterState::Header => {
                    let header = Header::new(DataType::U64, DataType::U64, roi);
                    this.buf[..HEADER_SIZE].copy_from_slice(&header.to_bytes());
                    *this.len = HEADER_SIZE;
                    *this.state = WriterState::WriteBuf;
                }
                WriterState::ReadRange => match ready!(this.stream.as_mut().poll_next(cx)) {
                    Some(item) => {
                        let r = item.into_range_result()?;
                        let start: u64 = r.start().into();
                        let end: u64 = r.end().into();
                        if *this.last_end > 0 && start <= *this.last_end {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "Range start {start} must be > previous end {}",
                                    *this.last_end
                                ),
                            )));
                        }
                        let gap = start - *this.last_end;
                        let len = end - start;
                        write_u64(&mut this.buf[..], gap);
                        write_u64(&mut this.buf[U64_SIZE..], len);
                        *this.last_end = end;
                        *this.len = U64_SIZE * 2;
                        *this.state = WriterState::WriteBuf;
                    }
                    None => {
                        if *this.last_end == 0 {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "Expected at least 1 range",
                            )));
                        }
                        *this.state = WriterState::Closing;
                    }
                },
                WriterState::WriteBuf => {
                    ready!(poll_write_all(
                        this.writer.as_mut(),
                        cx,
                        &this.buf[..*this.len],
                        this.pos
                    ))?;
                    *this.pos = 0;
                    *this.state = WriterState::ReadRange;
                }
                WriterState::Closing => {
                    ready!(this.writer.as_mut().poll_close(cx))?;
                    *this.state = WriterState::Done;
                }
                WriterState::Done => {
                    return Poll::Ready(Ok(()));
                }
            }
        }
    }
}

pin_project_lite::pin_project! {
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    struct HeaderReader<R> {
        #[pin] reader: R,
        buf: [u8; HEADER_SIZE],
        pos: usize,
    }
}

impl<R> HeaderReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buf: [0; HEADER_SIZE],
            pos: 0,
        }
    }
}

impl<R: AsyncRead + Unpin> Future for HeaderReader<R> {
    type Output = io::Result<Header>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        ready!(poll_read_exact(
            this.reader,
            cx,
            &mut this.buf[..],
            this.pos,
        ))?;
        Poll::Ready(Header::from_bytes(this.buf))
    }
}

pin_project! {
    pub struct AsyncRangeStream<R> {
        #[pin] reader: R,
        roi: Roi,
        buf: [u8; U64_SIZE * 2],
        pos: usize,
        last_end: u64,
    }
}

impl<R> ImageDimension for AsyncRangeStream<R> {
    fn bounds(&self) -> crate::Rect<u32> {
        crate::Rect {
            x: self.roi.offset_x,
            y: self.roi.offset_y,
            width: self.roi.width,
            height: self.roi.height,
        }
    }

    fn width(&self) -> std::num::NonZero<u32> {
        self.roi.width
    }
}

impl<R: AsyncRead + Unpin> AsyncRangeStream<R> {
    pub async fn new(mut reader: R) -> io::Result<Self> {
        let header = HeaderReader::new(&mut reader).await?;
        let roi = header.roi;
        Ok(AsyncRangeStream {
            reader,
            roi,
            buf: [0; U64_SIZE * 2],
            pos: 0,
            last_end: 0,
        })
    }
}

impl<R: AsyncRead> futures_core::Stream for AsyncRangeStream<R> {
    type Item = io::Result<NonZeroRange<u64>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        match ready!(poll_read_exact(
            this.reader.as_mut(),
            cx,
            &mut this.buf[..],
            this.pos
        )) {
            Ok(()) => {
                let gap = read_u64(&this.buf[..]);
                let len = read_u64(&this.buf[U64_SIZE..]);
                if len == 0 {
                    return Poll::Ready(None);
                }
                let start = *this.last_end + gap;
                let end = start + len;
                *this.last_end = end;
                *this.pos = 0;
                Poll::Ready(Some(Ok(NonZeroRange::new(start..end))))
            }
            Err(e) if *this.pos == 0 && e.kind() == ErrorKind::UnexpectedEof => Poll::Ready(None),
            Err(e) => Poll::Ready(Some(Err(e))),
        }
    }
}

#[cfg(test)]
mod tests {

    use std::ops::RangeInclusive;
    use std::{io::ErrorKind, num::NonZeroU32};

    use futures_util::TryStreamExt;
    use testresult::TestResult;

    use crate::io::PROTOCOL_VERSION;
    use crate::{Rect, WithRoi};

    use super::*;

    const NONZERO_1000: NonZeroU32 = NonZeroU32::new(1000).unwrap();
    const ROI: Rect<u32> = Rect::new(0, 0, NONZERO_1000, NONZERO_1000);
    fn with_1000_roi<I: IntoIterator>(
        inner: I,
    ) -> WithRoi<futures_util::stream::Iter<I::IntoIter>> {
        WithRoi::new(futures_util::stream::iter(inner), ROI)
    }

    fn make_header_bytes(
        offset_x: u32,
        offset_y: u32,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Vec<u8> {
        Header::new(
            DataType::U64,
            DataType::U64,
            Roi::new(offset_x, offset_y, width, height),
        )
        .to_bytes()
        .to_vec()
    }

    fn make_range_bytes(gap: u64, len: u64) -> [u8; 16] {
        let mut buf = [0u8; 16];
        write_u64(&mut buf[..8], gap);
        write_u64(&mut buf[8..], len);
        buf
    }

    fn expect_unexpected_eof<T>(result: io::Result<T>) {
        let Err(e) = result else {
            panic!("Expected io Error")
        };
        assert_eq!(e.kind(), ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn roundtrip() {
        let ranges = (0..100).map(|i| {
            let s = (i as u64) * 20;
            s..=s + 5 + (i as u64 % 10)
        });
        let expected: Vec<_> = ranges
            .clone()
            .map(|r| NonZeroRange::new(*r.start()..*r.end() + 1))
            .collect();
        let mut buf = Vec::new();
        let stream = with_1000_roi(ranges.map(Ok::<_, io::Error>));
        let writer = AsyncRangeWriter::new(&mut buf, stream);
        writer.await.unwrap();
        let reader = AsyncRangeStream::new(&buf[..]).await.unwrap();
        let result: Vec<_> = reader.try_collect().await.unwrap();
        assert_eq!(expected, result);
    }

    #[tokio::test]
    async fn read_empty_error() {
        let result = AsyncRangeStream::new(&[][..]).await;
        expect_unexpected_eof(result);
    }

    #[tokio::test]
    async fn write_empty_error() {
        let input: [RangeInclusive<u64>; 0] = [];
        let writer = Vec::new();
        let _err = AsyncRangeWriter::new(writer, with_1000_roi(input))
            .await
            .unwrap_err();
    }

    #[tokio::test]
    async fn header_only_is_empty() {
        let buf = make_header_bytes(0, 0, NonZeroU32::MIN, NonZeroU32::MIN);
        let reader = AsyncRangeStream::new(&buf[..]).await.unwrap();
        let result: Vec<_> = reader.try_collect().await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn overlapping_ranges_error() {
        let mut buf = Vec::new();
        let ranges = vec![10..=20u64, 15..=25];
        let writer = AsyncRangeWriter::new(&mut buf, with_1000_roi(ranges));
        let result = writer.await;
        assert!(matches!(result, Err(e) if e.kind() == ErrorKind::InvalidData));
    }

    #[tokio::test]
    async fn out_of_order_ranges_error() {
        let mut buf = Vec::new();
        let ranges = vec![50..=60u64, 10..=20];
        let writer = AsyncRangeWriter::new(&mut buf, with_1000_roi(ranges));
        let result = writer.await;
        assert!(matches!(result, Err(e) if e.kind() == ErrorKind::InvalidData));
    }

    #[tokio::test]
    async fn adjacent_ranges_error() {
        let mut buf = Vec::new();
        let ranges = vec![10..=20u64, 21..=30];
        let writer = AsyncRangeWriter::new(&mut buf, with_1000_roi(ranges));
        let result = writer.await;
        assert!(matches!(result, Err(e) if e.kind() == ErrorKind::InvalidData));
    }

    #[tokio::test]
    async fn invalid_protocol_version() {
        let mut buf = vec![0x99, 0x00, 0x00];
        buf.resize(HEADER_SIZE, 0);
        let result = AsyncRangeStream::new(&buf[..]).await;
        let Err(e) = result else {
            panic!("Expected error")
        };
        assert_eq!(e.kind(), ErrorKind::InvalidData);
        assert!(e.to_string().contains("Unsupported protocol version: 0x99"));
    }

    #[tokio::test]
    async fn invalid_included_data_type() {
        let mut buf = vec![PROTOCOL_VERSION, 0x05, 0x00];
        buf.resize(HEADER_SIZE, 0);
        let result = AsyncRangeStream::new(&buf[..]).await;
        let Err(e) = result else {
            panic!("Expected error")
        };
        assert_eq!(e.kind(), ErrorKind::InvalidData);
        assert!(e.to_string().contains("Unsupported data type: 5"));
    }

    #[tokio::test]
    async fn invalid_excluded_data_type() {
        let mut buf = vec![PROTOCOL_VERSION, 0x00, 0xFF];
        buf.resize(HEADER_SIZE, 0);
        let result = AsyncRangeStream::new(&buf[..]).await;
        let Err(e) = result else {
            panic!("Expected error")
        };
        assert_eq!(e.kind(), ErrorKind::InvalidData);
        assert!(e.to_string().contains("Unsupported data type: 255"));
    }

    #[tokio::test]
    async fn truncated_header_one_byte() {
        let buf = vec![PROTOCOL_VERSION];
        let result = AsyncRangeStream::new(&buf[..]).await;
        expect_unexpected_eof(result);
    }

    #[tokio::test]
    async fn truncated_header_two_bytes() {
        let buf = vec![PROTOCOL_VERSION, 0x00];
        let result = AsyncRangeStream::new(&buf[..]).await;
        expect_unexpected_eof(result);
    }

    #[tokio::test]
    async fn truncated_range_partial_gap() {
        let mut buf = make_header_bytes(0, 0, NonZeroU32::MIN, NonZeroU32::MIN);
        buf.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        let reader = AsyncRangeStream::new(&buf[..]).await.unwrap();
        let result = reader.try_collect::<Vec<_>>().await;
        expect_unexpected_eof(result);
    }

    #[tokio::test]
    async fn truncated_range_gap_complete_len_partial() {
        let mut buf = make_header_bytes(0, 0, NonZeroU32::MIN, NonZeroU32::MIN);
        buf.extend_from_slice(&make_range_bytes(10, 100)[..12]);
        let reader = AsyncRangeStream::new(&buf[..]).await.unwrap();
        let result = reader.try_collect::<Vec<_>>().await;
        expect_unexpected_eof(result);
    }

    #[tokio::test]
    async fn single_range_roundtrip() {
        let ranges = vec![100..=200u64];
        let mut buf = Vec::new();
        let writer = AsyncRangeWriter::new(&mut buf, with_1000_roi(ranges));
        writer.await.unwrap();
        let reader = AsyncRangeStream::new(&buf[..]).await.unwrap();
        let result: Vec<_> = reader.try_collect().await.unwrap();
        assert_eq!(result, vec![NonZeroRange::new(100u64..201)]);
    }

    #[tokio::test]
    async fn len_zero_terminates_stream() {
        let mut buf = make_header_bytes(0, 0, NONZERO_1000, NONZERO_1000);
        buf.extend_from_slice(&make_range_bytes(10, 100));
        buf.extend_from_slice(&make_range_bytes(5, 0));
        let reader = AsyncRangeStream::new(&buf[..]).await.unwrap();
        let result: Vec<_> = reader.try_collect().await.unwrap();
        assert_eq!(result, vec![NonZeroRange::new(10u64..110)]);
    }

    #[tokio::test]
    async fn io_error_on_read() {
        use std::io::{Error, ErrorKind};
        use std::pin::Pin;
        use std::task::{Context, Poll};

        struct FailingReader;

        impl AsyncRead for FailingReader {
            fn poll_read(
                self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                _buf: &mut [u8],
            ) -> Poll<io::Result<usize>> {
                Poll::Ready(Err(Error::new(ErrorKind::Other, "test error")))
            }
        }

        let result = AsyncRangeStream::new(FailingReader).await;
        assert!(matches!(result, Err(e) if e.kind() == ErrorKind::Other));
    }

    #[tokio::test]
    async fn writer_io_error() {
        use std::io::{Error, ErrorKind};
        use std::pin::Pin;
        use std::task::{Context, Poll};

        struct FailingWriter;

        impl AsyncWrite for FailingWriter {
            fn poll_write(
                self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                _buf: &[u8],
            ) -> Poll<io::Result<usize>> {
                Poll::Ready(Err(Error::new(ErrorKind::Other, "write error")))
            }

            fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
                Poll::Ready(Ok(()))
            }

            fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
                Poll::Ready(Ok(()))
            }
        }

        let writer = FailingWriter;
        let ranges = vec![10..=20u64];
        let async_writer = AsyncRangeWriter::new(writer, with_1000_roi(ranges));
        let result = async_writer.await;
        let Err(e) = result else {
            panic!("Expected Custom io error");
        };
        assert_eq!(e.kind(), ErrorKind::Other);
    }

    #[tokio::test]
    async fn rectangle_roundtrip_local() {
        let offset_x = 3u32;
        let offset_y = 5u32;
        let width = NonZeroU32::new(20).unwrap();
        let height = NonZeroU32::new(3).unwrap();
        let content_width = width.get() - offset_x;

        let roi = Rect::new(offset_x, offset_y, width, height);

        let local_ranges: Vec<RangeInclusive<u64>> = (0u64..height.get() as u64)
            .map(|i| {
                let s = i * width.get() as u64;
                let e = s + content_width as u64 - 1;
                s..=e
            })
            .collect();

        assert_eq!(local_ranges, vec![0..=16, 20..=36, 40..=56]);

        let mut buf = Vec::new();
        let writer = AsyncRangeWriter::new(
            &mut buf,
            WithRoi::new(
                futures_util::stream::iter(
                    local_ranges
                        .iter()
                        .into_iter()
                        .map(|x| std::io::Result::Ok(x.clone())),
                ),
                roi,
            ),
        );
        writer.await.unwrap();

        let reader = AsyncRangeStream::new(&buf[..]).await.unwrap();
        let reader_roi = reader.bounds();
        assert_eq!(roi, reader_roi);

        let result: Vec<_> = reader.try_collect().await.unwrap();
        let expected: Vec<_> = local_ranges
            .iter()
            .map(|r| NonZeroRange::new(*r.start()..*r.end() + 1))
            .collect();
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn reader_roi_forwarded_to_writer() -> TestResult {
        use crate::{Rect, SortedRanges};

        let bounds = Rect::new(0u32, 0, NONZERO_1000, NONZERO_1000);
        let sorted = SortedRanges::<u64, u64>::try_from_ordered_iter_roi(
            [10u64..20, 30..40, 1050..1060],
            bounds,
        )
        .unwrap();
        let expected: Vec<_> = sorted.iter_roi::<NonZeroRange<u64>>().collect();

        let roi = Rect::new(0, 0, NONZERO_1000, NONZERO_1000);
        let phase1_buf = {
            let mut buf = Vec::new();
            AsyncRangeWriter::new(
                &mut buf,
                with_1000_roi(sorted.iter_roi::<NonZeroRange<u64>>()),
            )
            .await?;
            buf
        };

        let reader = AsyncRangeStream::new(&phase1_buf[..]).await.unwrap();
        let reader_roi = reader.bounds();
        assert_eq!(roi, reader_roi);

        let mut phase2_buf = Vec::new();
        AsyncRangeWriter::new(&mut phase2_buf, reader)
            .await
            .unwrap();

        let verify_reader = AsyncRangeStream::new(&phase2_buf[..]).await.unwrap();
        let verified = verify_reader.try_collect::<Vec<_>>().await.unwrap();
        assert_eq!(expected, verified);
        assert_eq!(phase1_buf, phase2_buf);

        Ok(())
    }

    #[tokio::test]
    async fn stream_io_result_error_propagates() {
        let ranges: Vec<io::Result<RangeInclusive<u64>>> = vec![
            Ok(10..=20),
            Err(io::Error::new(io::ErrorKind::Other, "source error")),
        ];
        let mut buf = Vec::new();
        let result = AsyncRangeWriter::new(&mut buf, with_1000_roi(ranges)).await;
        assert!(matches!(result, Err(e) if e.kind() == ErrorKind::Other));
    }
}
