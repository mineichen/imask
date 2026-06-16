use std::{io, num::NonZeroU32};

use super::{U32_SIZE, read_u32, write_u32};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Roi {
    pub offset_x: u32,
    pub offset_y: u32,
    pub width: NonZeroU32,
    pub height: NonZeroU32,
}

impl Roi {
    #[cfg(test)]
    pub(super) const fn new(
        offset_x: u32,
        offset_y: u32,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Self {
        Self {
            offset_x,
            offset_y,
            width,
            height,
        }
    }

    pub fn to_bytes(self) -> [u8; U32_SIZE * 4] {
        let mut buf = [0u8; U32_SIZE * 4];
        write_u32(&mut buf[..], self.offset_x);
        write_u32(&mut buf[U32_SIZE..], self.offset_y);
        write_u32(&mut buf[U32_SIZE * 2..], self.width.get());
        write_u32(&mut buf[U32_SIZE * 3..], self.height.get());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let width_pos = U32_SIZE * 2;
        let height_pos = U32_SIZE * 3;
        Ok(Self {
            offset_x: read_u32(bytes),
            offset_y: read_u32(&bytes[U32_SIZE..]),
            width: read_u32(&bytes[width_pos..]).try_into().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Unexpected zero for width")
            })?,
            height: read_u32(&bytes[height_pos..]).try_into().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Unexpected zero for height")
            })?,
        })
    }
}
