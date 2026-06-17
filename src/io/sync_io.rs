use std::io::{self, Read, Write};
use std::num::NonZero;

use super::{HEADER_SIZE, IntoRangeResult, U64_SIZE, read_u64, unexpected_eof, write_u64};
use crate::{CreateRange, ImageDimension, SortedRanges};

// reader.read_exact doesn't provide info, if anything was read
fn read_exact_or_nothing<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<bool> {
    let mut offset = 0;
    while offset < buf.len() {
        match reader.read(&mut buf[offset..])? {
            0 => {
                return if offset == 0 {
                    Ok(false)
                } else {
                    Err(unexpected_eof())
                };
            }
            n => offset += n,
        }
    }
    Ok(true)
}

pub struct SyncRangeWriter<W, I> {
    writer: W,
    iter: I,
}

impl<W, I> SyncRangeWriter<W, I> {
    pub fn new(writer: W, iter: I) -> Self {
        Self { writer, iter }
    }
}

#[allow(private_bounds)]
impl<W: Write, I: Iterator + ImageDimension> SyncRangeWriter<W, I>
where
    I::Item: IntoRangeResult,
{
    pub fn write(mut self) -> io::Result<()> {
        let roi = self.iter.bounds();
        let roi = super::roi::Roi {
            offset_x: roi.x,
            offset_y: roi.y,
            width: roi.width,
            height: roi.height,
        };
        let header = super::header::Header::new(
            super::header::DataType::U64,
            super::header::DataType::U64,
            roi,
        );
        self.writer.write_all(&header.to_bytes())?;

        let mut last_end = 0u64;
        let mut first = true;
        for item in &mut self.iter {
            let r = item.into_range_result()?;
            let start: u64 = r.start().into();
            let end: u64 = r.end().into();
            if !first && start <= last_end {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Range start {start} must be > previous end {last_end}"),
                ));
            }
            let gap = start - last_end;
            let len = end - start;
            let mut buf = [0u8; U64_SIZE * 2];
            write_u64(&mut buf[..], gap);
            write_u64(&mut buf[U64_SIZE..], len);
            self.writer.write_all(&buf)?;
            last_end = end;
            first = false;
        }
        if first {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Expected at least 1 range",
            ));
        }
        let buf = [0u8; U64_SIZE * 2];
        self.writer.write_all(&buf)?;
        Ok(())
    }
}

pub struct ReaderRangeIterator<R, TRange> {
    reader: R,
    roi: super::roi::Roi,
    last_end: u64,
    _phantom: std::marker::PhantomData<TRange>,
}

impl<R, TRange> ImageDimension for ReaderRangeIterator<R, TRange> {
    fn bounds(&self) -> crate::Rect<u32> {
        crate::Rect {
            x: self.roi.offset_x,
            y: self.roi.offset_y,
            width: self.roi.width,
            height: self.roi.height,
        }
    }

    fn width(&self) -> NonZero<u32> {
        self.roi.width
    }
}

impl<R: Read, TRange> ReaderRangeIterator<R, TRange> {
    pub fn try_new(mut reader: R) -> io::Result<Self> {
        let mut buf = [0u8; HEADER_SIZE];

        reader.read_exact(&mut buf)?;
        let header = super::header::Header::from_bytes(&buf)?;
        Ok(Self {
            reader,
            roi: header.roi,
            last_end: 0,
            _phantom: std::marker::PhantomData,
        })
    }
}

impl<R: Read, TRange: CreateRange<Item = u64>> Iterator for ReaderRangeIterator<R, TRange> {
    type Item = io::Result<TRange>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = [0u8; U64_SIZE * 2];
        match read_exact_or_nothing(&mut self.reader, &mut buf) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(e) => return Some(Err(e)),
        }
        let gap = read_u64(&buf[..]);
        let len = read_u64(&buf[U64_SIZE..]);
        if len == 0 {
            return None;
        }
        let start = self.last_end + gap;
        let end = start + len;
        self.last_end = end;
        Some(Ok(TRange::new_debug_checked_zeroable(start, end)))
    }
}

impl<TIncluded, TExcluded> SortedRanges<TIncluded, TExcluded> {
    pub fn from_serialized(input: &[u8]) -> io::Result<Self>
    where
        TIncluded: TryFrom<u64>,
        TExcluded: TryFrom<u64>,
        <TIncluded as TryFrom<u64>>::Error: std::fmt::Display,
        <TExcluded as TryFrom<u64>>::Error: std::fmt::Display,
    {
        let Some(header_bytes) = input.first_chunk() else {
            return Err(unexpected_eof());
        };
        let header = super::header::Header::from_bytes(header_bytes)?;
        let bounds = crate::Rect {
            x: header.roi.offset_x,
            y: header.roi.offset_y,
            width: header.roi.width,
            height: header.roi.height,
        };

        let mut all_included = Vec::new();
        let all_excluded = input[HEADER_SIZE..]
            .chunks_exact(U64_SIZE * 2)
            .map(|chunk| (read_u64(chunk), read_u64(&chunk[U64_SIZE..])))
            .take_while(|&(_, len)| len != 0)
            .map(|(gap, len)| {
                let included = TIncluded::try_from(len)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                let excluded = TExcluded::try_from(gap)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()));
                all_included.push(included);
                excluded
            })
            .collect::<Result<_, _>>()?;
        if all_included.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Expected at least 1 range",
            ));
        }

        Ok(Self::from_parts(all_included, all_excluded, bounds))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::ops::{Range, RangeInclusive};

    use crate::io::PROTOCOL_VERSION;
    use crate::set::ImaskSet;
    use crate::{NonZeroRange, Rect, WithRoi};

    use super::*;

    const NONZERO_1000: NonZeroU32 = NonZeroU32::new(1000).unwrap();
    const ROI: Rect<u32> = Rect::new(0, 0, NONZERO_1000, NONZERO_1000);

    fn with_roi<I: IntoIterator>(inner: I) -> WithRoi<I::IntoIter> {
        WithRoi::new(inner.into_iter(), ROI)
    }

    fn make_header_bytes(
        offset_x: u32,
        offset_y: u32,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Vec<u8> {
        super::super::header::Header::new(
            super::super::header::DataType::U64,
            super::super::header::DataType::U64,
            super::super::roi::Roi::new(offset_x, offset_y, width, height),
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
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn roundtrip() {
        let ranges = (0..100).map(|i| {
            let s = (i as u64) * 20;
            s..=s + 5 + (i as u64 % 10)
        });
        let expected: Vec<_> = ranges
            .clone()
            .map(|r| NonZeroRange::new(*r.start()..*r.end() + 1))
            .collect();
        let mut buf = Vec::new();
        let writer = SyncRangeWriter::new(&mut buf, with_roi(ranges.map(Ok::<_, io::Error>)));
        writer.write().unwrap();
        let reader = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]).unwrap();
        let result: Vec<_> = reader.collect::<io::Result<Vec<_>>>().unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    fn read_empty_error() {
        let result = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&[][..]);
        expect_unexpected_eof(result);
    }

    #[test]
    fn write_empty_error() {
        let input: [RangeInclusive<u64>; 0] = [];
        let writer = SyncRangeWriter::new(Vec::new(), with_roi(input));
        let _err = writer.write().unwrap_err();
    }

    #[test]
    fn header_only_is_empty() {
        let buf = make_header_bytes(0, 0, NonZeroU32::MIN, NonZeroU32::MIN);
        let reader = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]).unwrap();
        let result: Vec<_> = reader.collect::<io::Result<Vec<_>>>().unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn overlapping_ranges_error() {
        let ranges = vec![10..=20u64, 15..=25];
        let writer = SyncRangeWriter::new(Vec::new(), with_roi(ranges));
        let result = writer.write();
        assert!(matches!(result, Err(e) if e.kind() == io::ErrorKind::InvalidData));
    }

    #[test]
    fn out_of_order_ranges_error() {
        let ranges = vec![50..=60u64, 10..=20];
        let writer = SyncRangeWriter::new(Vec::new(), with_roi(ranges));
        let result = writer.write();
        assert!(matches!(result, Err(e) if e.kind() == io::ErrorKind::InvalidData));
    }

    #[test]
    fn adjacent_ranges_error() {
        let ranges = vec![10..=20u64, 21..=30];
        let writer = SyncRangeWriter::new(Vec::new(), with_roi(ranges));
        let result = writer.write();
        assert!(matches!(result, Err(e) if e.kind() == io::ErrorKind::InvalidData));
    }

    #[test]
    fn invalid_protocol_version() {
        let mut buf = vec![0x99, 0x00, 0x00];
        buf.resize(HEADER_SIZE, 0);
        let result = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]);
        let Err(e) = result else {
            panic!("Expected error")
        };
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("Unsupported protocol version: 0x99"));
    }

    #[test]
    fn invalid_included_data_type() {
        let mut buf = vec![PROTOCOL_VERSION, 0x05, 0x00];
        buf.resize(HEADER_SIZE, 0);
        let result = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]);
        let Err(e) = result else {
            panic!("Expected error")
        };
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("Unsupported data type: 5"));
    }

    #[test]
    fn invalid_excluded_data_type() {
        let mut buf = vec![PROTOCOL_VERSION, 0x00, 0xFF];
        buf.resize(HEADER_SIZE, 0);
        let result = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]);
        let Err(e) = result else {
            panic!("Expected error")
        };
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("Unsupported data type: 255"));
    }

    #[test]
    fn truncated_header_one_byte() {
        let buf = vec![PROTOCOL_VERSION];
        let result = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]);
        expect_unexpected_eof(result);
    }

    #[test]
    fn truncated_header_two_bytes() {
        let buf = vec![PROTOCOL_VERSION, 0x00];
        let result = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]);
        expect_unexpected_eof(result);
    }

    #[test]
    fn truncated_range_partial_gap() {
        let mut buf = make_header_bytes(0, 0, NonZeroU32::MIN, NonZeroU32::MIN);
        buf.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        let reader = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]).unwrap();
        let result = reader.collect::<io::Result<Vec<_>>>();
        expect_unexpected_eof(result);
    }

    #[test]
    fn truncated_range_gap_complete_len_partial() {
        let mut buf = make_header_bytes(0, 0, NonZeroU32::MIN, NonZeroU32::MIN);
        buf.extend_from_slice(&make_range_bytes(10, 100)[..12]);
        let reader = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]).unwrap();
        let result = reader.collect::<io::Result<Vec<_>>>();
        expect_unexpected_eof(result);
    }

    #[test]
    fn single_range_roundtrip() {
        let ranges = vec![100..=200u64];
        let mut buf = Vec::new();
        let writer = SyncRangeWriter::new(&mut buf, with_roi(ranges));
        writer.write().unwrap();
        let reader = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]).unwrap();
        let result: Vec<_> = reader.collect::<io::Result<Vec<_>>>().unwrap();
        assert_eq!(result, vec![NonZeroRange::new(100u64..201)]);
    }

    #[test]
    fn len_zero_terminates_stream() {
        let mut buf = make_header_bytes(0, 0, NONZERO_1000, NONZERO_1000);
        buf.extend_from_slice(&make_range_bytes(10, 100));
        buf.extend_from_slice(&make_range_bytes(5, 0));
        let reader = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]).unwrap();
        let result: Vec<_> = reader.collect::<io::Result<Vec<_>>>().unwrap();
        assert_eq!(result, vec![NonZeroRange::new(10u64..110)]);
    }

    #[test]
    fn io_error_on_read() {
        struct FailingReader;

        impl Read for FailingReader {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::Other, "test error"))
            }
        }

        let result = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(FailingReader);
        assert!(matches!(result, Err(e) if e.kind() == io::ErrorKind::Other));
    }

    #[test]
    fn writer_io_error() {
        struct FailingWriter;

        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::Other, "write error"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let ranges = vec![10..=20u64];
        let writer = SyncRangeWriter::new(FailingWriter, with_roi(ranges));
        let result = writer.write();
        let Err(e) = result else {
            panic!("Expected Custom io error");
        };
        assert_eq!(e.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn rectangle_roundtrip_local() {
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
        let writer = SyncRangeWriter::new(
            &mut buf,
            WithRoi::new(
                local_ranges.iter().map(|x| std::io::Result::Ok(x.clone())),
                roi,
            ),
        );
        writer.write().unwrap();

        let reader = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&buf[..]).unwrap();
        let reader_roi = reader.bounds();
        assert_eq!(roi, reader_roi);

        let result: Vec<_> = reader.collect::<io::Result<Vec<_>>>().unwrap();
        let expected: Vec<_> = local_ranges
            .iter()
            .map(|r| NonZeroRange::new(*r.start()..*r.end() + 1))
            .collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn reader_roi_forwarded_to_writer() {
        use crate::SortedRanges;

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
            SyncRangeWriter::new(&mut buf, with_roi(sorted.iter_roi::<NonZeroRange<u64>>()))
                .write()
                .unwrap();
            buf
        };

        let reader = ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&phase1_buf[..]).unwrap();
        let reader_roi = reader.bounds();
        assert_eq!(roi, reader_roi);

        let mut phase2_buf = Vec::new();
        SyncRangeWriter::new(&mut phase2_buf, reader)
            .write()
            .unwrap();

        let verify_reader =
            ReaderRangeIterator::<_, NonZeroRange<u64>>::try_new(&phase2_buf[..]).unwrap();
        let verified = verify_reader.collect::<io::Result<Vec<_>>>().unwrap();
        assert_eq!(expected, verified);
        assert_eq!(phase1_buf, phase2_buf);
    }

    #[test]
    fn stream_io_result_error_propagates() {
        let ranges: Vec<io::Result<RangeInclusive<u64>>> = vec![
            Ok(10..=20),
            Err(io::Error::new(io::ErrorKind::Other, "source error")),
        ];
        let mut buf = Vec::new();
        let result = SyncRangeWriter::new(&mut buf, with_roi(ranges)).write();
        assert!(matches!(result, Err(e) if e.kind() == io::ErrorKind::Other));
    }

    #[test]
    fn from_serialized_roundtrip_with_offset() {
        let offset_x = 1u32;
        let offset_y = 2u32;
        let width = NonZeroU32::new(100).unwrap();
        let height = NonZeroU32::new(200).unwrap();
        let roi = Rect::new(offset_x, offset_y, width, height);

        let local_ranges: Vec<Range<u64>> = vec![10u64..30, 45..50, 205..210];
        let original = crate::SortedRanges::<u64, u64>::try_from_ordered_iter_roi(
            local_ranges.clone().with_roi(roi),
            roi,
        )
        .unwrap();

        let mut buf = Vec::new();
        SyncRangeWriter::new(&mut buf, original.iter_roi::<Range<u64>>().with_roi(roi))
            .write()
            .unwrap();

        let from_serialized = crate::SortedRanges::<u64, u64>::from_serialized(&buf).unwrap();
        assert_eq!(from_serialized, original);
    }

    #[test]
    fn reader_to_sorted_ranges_roundtrip_with_offset() {
        let offset_x = 1u32;
        let offset_y = 2u32;
        let width = NonZeroU32::new(100).unwrap();
        let height = NonZeroU32::new(200).unwrap();
        let roi = Rect::new(offset_x, offset_y, width, height);

        let local_ranges: Vec<Range<u64>> = vec![10u64..30, 45..50, 205..210];
        let original = crate::SortedRanges::<u64, u64>::try_from_ordered_iter_roi(
            local_ranges.clone().with_roi(roi),
            roi,
        )
        .unwrap();

        let mut buf = Vec::new();
        SyncRangeWriter::new(&mut buf, original.iter_roi::<Range<u64>>().with_roi(roi))
            .write()
            .unwrap();

        let reader = ReaderRangeIterator::<_, Range<u64>>::try_new(&buf[..]).unwrap();
        assert_eq!(reader.bounds(), roi);
        let reader_ranges: Vec<_> = reader.collect::<io::Result<Vec<_>>>().unwrap();
        let via_reader = crate::SortedRanges::<u64, u64>::try_from_ordered_iter_roi(
            reader_ranges.with_roi(roi),
            roi,
        )
        .unwrap();
        assert_eq!(via_reader, original);
    }
}
