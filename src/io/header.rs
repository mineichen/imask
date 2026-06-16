use std::io;

use super::{HEADER_SIZE, PROTOCOL_VERSION, roi::Roi};

#[derive(Debug, Clone)]
pub(super) struct Header {
    version: u8,
    included_type: DataType,
    excluded_type: DataType,
    pub roi: Roi,
}

impl Header {
    pub fn new(included_type: DataType, excluded_type: DataType, roi: Roi) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            excluded_type,
            included_type,
            roi,
        }
    }
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0] = self.version;
        buf[1] = self.included_type as u8;
        buf[2] = self.excluded_type as u8;
        buf[3..].copy_from_slice(&self.roi.to_bytes());
        buf
    }
    pub fn from_bytes(bytes: &[u8; HEADER_SIZE]) -> io::Result<Self> {
        if bytes[0] != PROTOCOL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unsupported protocol version: {:#x}", bytes[0]),
            ));
        }
        Ok(Self {
            version: bytes[0],
            included_type: DataType::try_from(bytes[1])?,
            excluded_type: DataType::try_from(bytes[2])?,
            roi: Roi::from_bytes(&bytes[3..])?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum DataType {
    U64 = 0,
}

impl TryFrom<u8> for DataType {
    type Error = io::Error;
    fn try_from(value: u8) -> io::Result<Self> {
        match value {
            0 => Ok(DataType::U64),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unsupported data type: {value}"),
            )),
        }
    }
}
