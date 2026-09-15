pub trait Squareable: TryFrom<Self::Square>
where
    Self::Square: Sqrtable<Sqrt = Self>,
{
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

impl Squareable for u64 {
    type Square = u128;
}

pub trait Sqrtable: Sized + From<Self::Sqrt>
where
    Self::Sqrt: Squareable<Square = Self>,
{
    type Sqrt: TryFrom<Self>;
}

impl Sqrtable for u16 {
    type Sqrt = u8;
}

impl Sqrtable for u32 {
    type Sqrt = u16;
}

impl Sqrtable for u64 {
    type Sqrt = u32;
}

impl Sqrtable for u128 {
    type Sqrt = u64;
}
