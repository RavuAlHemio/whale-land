#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fixed {
    // 24.8
    inner_value: i32,
}
impl Fixed {
    pub const fn inner_value(&self) -> i32 { self.inner_value }
    pub const fn from_inner_value(inner_value: i32) -> Self {
        Self {
            inner_value,
        }
    }
}

macro_rules! impl_from_int {
    ($src_t:ty) => {
        impl From<$src_t> for Fixed {
            fn from(value: $src_t) -> Self {
                Self {
                    inner_value: (value as i32) << 8,
                }
            }
        }
    };
}
impl_from_int!(u8);
impl_from_int!(i8);
impl_from_int!(u16);
impl_from_int!(i16);

macro_rules! impl_try_from_int {
    ($src_t:ty) => {
        impl TryFrom<$src_t> for Fixed {
            type Error = ();
            fn try_from(value: $src_t) -> Result<Self, Self::Error> {
                let shifted = value.checked_shl(8)
                    .ok_or(())?;
                let converted = i32::try_from(shifted)
                    .map_err(|_| ())?;
                Ok(Self {
                    inner_value: converted,
                })
            }
        }
    };
}
impl_try_from_int!(u32);
impl_try_from_int!(i32);
impl_try_from_int!(u64);
impl_try_from_int!(i64);
impl_try_from_int!(u128);
impl_try_from_int!(i128);

macro_rules! impl_try_from_float {
    ($src_t:ty) => {
        impl TryFrom<$src_t> for Fixed {
            type Error = ();

            fn try_from(value: $src_t) -> Result<Self, Self::Error> {
                let one_shift_8 = <$src_t>::from(1u16 << 8);
                let multiplied = value * one_shift_8;
                if multiplied.fract() == 0.0 {
                    let max_int = i32::MAX as $src_t;
                    let min_int = i32::MIN as $src_t;
                    if multiplied > max_int || multiplied < min_int {
                        Err(())
                    } else {
                        Ok(Self {
                            inner_value: multiplied as i32,
                        })
                    }
                } else {
                    Err(())
                }
            }
        }
    };
}
impl_try_from_float!(f32);
impl_try_from_float!(f64);


macro_rules! impl_try_into_int {
    ($dest_t:ty) => {
        impl TryFrom<Fixed> for $dest_t {
            type Error = ();
            fn try_from(value: Fixed) -> Result<$dest_t, Self::Error> {
                const FRAC_BIT_MASK: i32 = (1 << 8) - 1;
                if value.inner_value & FRAC_BIT_MASK == 0 {
                    let int_value = value.inner_value >> 8;
                    int_value.try_into()
                        .map_err(|_| ())
                } else {
                    Err(())
                }
            }
        }
    };
}
impl_try_into_int!(u8);
impl_try_into_int!(i8);
impl_try_into_int!(u16);
impl_try_into_int!(i16);
impl_try_into_int!(u32);
impl_try_into_int!(i32);
impl_try_into_int!(u64);
impl_try_into_int!(i64);
impl_try_into_int!(u128);
impl_try_into_int!(i128);

macro_rules! impl_into_float {
    ($dest_t:ty) => {
        impl From<Fixed> for $dest_t {
            fn from(value: Fixed) -> $dest_t {
                let one_shift_8 = <$dest_t>::from(1u16 << 8);
                (value.inner_value as $dest_t) / one_shift_8
            }
        }
    };
}
impl_into_float!(f32);
impl_into_float!(f64);
