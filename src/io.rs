#[cfg(feature = "async-io")]
mod async_io;
mod header;
mod roi;
#[cfg(feature = "async-io")]
mod sync_io;

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
