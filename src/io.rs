#[cfg(feature = "async-io")]
mod async_io;
mod header;
mod roi;
#[cfg(feature = "async-io")]
mod sync_io;

use crate::CreateRange;

#[cfg(feature = "async-io")]
pub use async_io::*;
// #[cfg(feature = "async-io")]
pub use sync_io::*;

const U32_SIZE: usize = std::mem::size_of::<u32>();
const U64_SIZE: usize = std::mem::size_of::<u64>();
const PROTOCOL_VERSION: u8 = 1;
const HEADER_SIZE: usize = 3 + U32_SIZE * 4;

fn write_u32(buf: &mut [u8], val: u32) {
    buf[..U32_SIZE].copy_from_slice(&val.to_le_bytes());
}

fn read_u32(buf: &[u8]) -> u32 {
    u32::from_le_bytes(buf[..U32_SIZE].try_into().unwrap())
}

fn write_u64(buf: &mut [u8], val: u64) {
    buf[..8].copy_from_slice(&val.to_le_bytes());
}
fn read_u64(buf: &[u8]) -> u64 {
    u64::from_le_bytes(buf[..8].try_into().unwrap())
}
fn unexpected_eof() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Unexpected Eof")
}

pub(super) trait IntoRangeResult {
    type Range: CreateRange<Item: Into<u64>>;
    fn into_range_result(self) -> std::io::Result<Self::Range>;
}

impl<T> IntoRangeResult for T
where
    T: CreateRange<Item: Into<u64>>,
{
    type Range = T;
    fn into_range_result(self) -> std::io::Result<Self::Range> {
        Ok(self)
    }
}

impl<T, E> IntoRangeResult for Result<T, E>
where
    T: CreateRange<Item: Into<u64>>,
    E: Into<std::io::Error>,
{
    type Range = T;
    fn into_range_result(self) -> std::io::Result<Self::Range> {
        self.map_err(Into::into)
    }
}
