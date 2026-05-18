use std::ops::Neg;

pub trait CheckedAddSigned: Sized {
    type Signed: Neg<Output = Self::Signed> + Copy;
    fn checked_add_signed(&self, rhs: Self::Signed) -> Option<Self>;
    fn into_signed(v: Self) -> Self::Signed;
}

impl CheckedAddSigned for u8 {
    type Signed = i8;
    fn checked_add_signed(&self, rhs: i8) -> Option<Self> {
        u8::checked_add_signed(*self, rhs)
    }
    fn into_signed(v: Self) -> i8 {
        v as i8
    }
}
impl CheckedAddSigned for u16 {
    type Signed = i16;
    fn checked_add_signed(&self, rhs: i16) -> Option<Self> {
        u16::checked_add_signed(*self, rhs)
    }
    fn into_signed(v: Self) -> i16 {
        v as i16
    }
}
impl CheckedAddSigned for u32 {
    type Signed = i32;
    fn checked_add_signed(&self, rhs: i32) -> Option<Self> {
        u32::checked_add_signed(*self, rhs)
    }
    fn into_signed(v: Self) -> i32 {
        v as i32
    }
}
impl CheckedAddSigned for u64 {
    type Signed = i64;
    fn checked_add_signed(&self, rhs: i64) -> Option<Self> {
        u64::checked_add_signed(*self, rhs)
    }
    fn into_signed(v: Self) -> i64 {
        v as i64
    }
}
impl CheckedAddSigned for usize {
    type Signed = isize;
    fn checked_add_signed(&self, rhs: isize) -> Option<Self> {
        usize::checked_add_signed(*self, rhs)
    }
    fn into_signed(v: Self) -> isize {
        v as isize
    }
}
