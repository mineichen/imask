use std::io;

use crate::{NonZeroRange, Roi};

use super::{U32_SIZE, read_u32, write_u32};

impl Roi<u32> {
    pub fn to_bytes(self) -> [u8; U32_SIZE * 4] {
        let mut buf = [0u8; U32_SIZE * 4];
        write_u32(&mut buf[..], self.x.start);
        write_u32(&mut buf[U32_SIZE..], self.y.start);
        write_u32(&mut buf[U32_SIZE * 2..], self.width().get());
        write_u32(&mut buf[U32_SIZE * 3..], self.height().get());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let width_pos = U32_SIZE * 2;
        let height_pos = U32_SIZE * 3;
        let offset_x = read_u32(bytes);
        let offset_y = read_u32(&bytes[U32_SIZE..]);
        let width = read_u32(&bytes[width_pos..]);
        let height = read_u32(&bytes[height_pos..]);

        if !(1..(u32::MAX - offset_x)).contains(&width) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid value for width",
            ));
        }

        if !(1..(u32::MAX - offset_y)).contains(&height) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid value for height",
            ));
        }

        Ok(Self {
            x: NonZeroRange::new_unchecked(offset_x..offset_x + width),
            y: NonZeroRange::new_unchecked(offset_y..offset_y + height),
        })
    }
}
