pub trait Squareable: TryFrom<Self::Square> {
    type Square: From<Self>;
}

impl Squareable for u8 {
    type Square = u16;
}

impl Squareable for u16 {
    type Square = u32;
}
impl Squareable for u32 {
    type Square = u64;
}
