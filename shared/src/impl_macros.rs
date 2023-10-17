//! Contains convenience macros for type and message defininitions

/// Implements safe (e.g. no panic) conversion of an enum to all integer types and back
macro_rules! impl_enum_to_int {
    ($type:ty, $($variant:ident => $number:literal),+) => {
        impl_enum_to_int!(INT_VARIANT $type => u8, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => u16, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => i16, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => u32, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => i32, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => u64, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => i64, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => usize, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => isize, $($variant => $number),+);
    };

    (INT_VARIANT $type:ty => $int_type:ty, $($variant:ident => $number:literal),+) => {
        impl TryFrom<$int_type> for $type {
            type Error = ::anyhow::Error;
            fn try_from(value: $int_type) -> Result<Self, Self::Error> {
                match value {
                    $(
                        $number => Ok(Self::$variant),
                    )+
                    t => Err(anyhow::anyhow!("Invalid enum value {t} for conversion")),
                }
            }
        }

        impl From<$type> for $int_type {
            fn from(value: $type) -> $int_type {
                match value {
                    $(
                        <$type>::$variant => $number,
                    )+
                }
            }
        }
    };
}

/// Auto-implements From traits for two enums to convert back and forth
#[macro_export]
macro_rules! impl_from_and_into {
    ($type1:path, $type2:path, $($variant1:ident <=> $variant2:ident),+ $(,)?) => {
        impl From<$type1> for $type2 {
            fn from(value: $type1) -> Self {
                match value {
                    $(
                        <$type1>::$variant1 => Self::$variant2,
                     )+
                }
            }
        }

        impl From<$type2> for $type1 {
            fn from(value: $type2) -> Self {
                match value {
                    $(
                        <$type2>::$variant2 => Self::$variant1,
                     )+
                }
            }
        }
    };
}
