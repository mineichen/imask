/// Unchecked cast between unsigned integer types.
/// Cast fails during debug, but silently continues in release builds.
/// # Examples
/// ```
/// use imask::UncheckedCast;
///
/// let a: u8 = 255;
/// let b: u16 = a.cast_unchecked();
///
pub trait UncheckedCast<T>: Copy {
    fn cast_unchecked(self) -> T;
}

macro_rules! impl_debug_checked_cast {
    ($src:ty, $dst:ty) => {
        impl UncheckedCast<$dst> for $src {
            #[inline]
            fn cast_unchecked(self) -> $dst {
                if core::mem::size_of::<$src>() > core::mem::size_of::<$dst>() {
                    debug_assert!(
                        self <= (<$dst>::MAX as $src),
                        "Expected {}{} <= {}::MAX",
                        self,
                        core::any::type_name::<$src>(),
                        core::any::type_name::<$dst>()
                    );
                }
                self as _
            }
        }
    };
}

impl_debug_checked_cast!(u8, u8);
impl_debug_checked_cast!(u8, u16);
impl_debug_checked_cast!(u8, u32);
impl_debug_checked_cast!(u8, u64);
impl_debug_checked_cast!(u8, u128);
impl_debug_checked_cast!(u8, usize);

impl_debug_checked_cast!(u16, u8);
impl_debug_checked_cast!(u16, u16);
impl_debug_checked_cast!(u16, u32);
impl_debug_checked_cast!(u16, u64);
impl_debug_checked_cast!(u16, u128);
impl_debug_checked_cast!(u16, usize);

impl_debug_checked_cast!(u32, u8);
impl_debug_checked_cast!(u32, u16);
impl_debug_checked_cast!(u32, u32);
impl_debug_checked_cast!(u32, u64);
impl_debug_checked_cast!(u32, u128);
impl_debug_checked_cast!(u32, usize);

impl_debug_checked_cast!(u64, u8);
impl_debug_checked_cast!(u64, u16);
impl_debug_checked_cast!(u64, u32);
impl_debug_checked_cast!(u64, u64);
impl_debug_checked_cast!(u64, u128);
impl_debug_checked_cast!(u64, usize);

impl_debug_checked_cast!(u128, u8);
impl_debug_checked_cast!(u128, u16);
impl_debug_checked_cast!(u128, u32);
impl_debug_checked_cast!(u128, u64);
impl_debug_checked_cast!(u128, u128);
impl_debug_checked_cast!(u128, usize);

impl_debug_checked_cast!(usize, u8);
impl_debug_checked_cast!(usize, u16);
impl_debug_checked_cast!(usize, u32);
impl_debug_checked_cast!(usize, u64);
impl_debug_checked_cast!(usize, u128);
impl_debug_checked_cast!(usize, usize);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Expected 256u64 <= u8::MAX")]
    fn cast_invalid_u64_to_u8() {
        let _: u8 = 256u64.cast_unchecked();
    }

    #[test]
    fn cast_u64_to_u8() {
        let cast: u8 = 255u64.cast_unchecked();
        assert_eq!(255u8, cast);
    }
}
