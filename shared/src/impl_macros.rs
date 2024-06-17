//! Contains convenience macros for type and message definitions

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

macro_rules! impl_enum_user_str {
    ($type:ty, $($variant:path => $text:literal),+ $(,)?) => {
        impl $type {
            pub fn user_str(&self) -> &str {
                match self {
                    $(
                        $variant => $text,
                    )+
                }
            }
        }

        impl std::fmt::Display for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.user_str())
            }
        }
    };
}

#[cfg(feature = "protobuf")]
macro_rules! impl_enum_protobuf_traits {
    ($type:ty => $proto_type:ty, unspecified => $proto_unspec_variant:path, $($variant:path => $proto_variant:path),+ $(,)?) => {
        impl TryFrom<$proto_type> for $type {
            type Error = ::anyhow::Error;

            fn try_from(value: $proto_type) -> std::result::Result<Self, Self::Error> {
                let nt = match value {
                    $proto_unspec_variant => ::anyhow::bail!("$type is unspecified"),
                    $(
                        $proto_variant => $variant,
                    )+
                };

                Ok(nt)
            }
        }

        impl From<$type> for $proto_type {
            fn from(value: $type) -> Self {
                match value {
                    $(
                        $variant => $proto_variant,
                    )+
                }
            }
        }
    };
}
