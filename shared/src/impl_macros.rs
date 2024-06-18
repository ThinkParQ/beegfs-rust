//! Contains convenience macros for type and message definitions

/// Implements safe (e.g. no panic) conversion of an enum to all integer types and back
macro_rules! impl_enum_bee_msg_traits {
    ($type:ty, $($variant:ident => $number:literal),+) => {
        impl_enum_bee_msg_traits!(INT_VARIANT $type => u8, $($variant => $number),+);
        impl_enum_bee_msg_traits!(INT_VARIANT $type => u16, $($variant => $number),+);
        impl_enum_bee_msg_traits!(INT_VARIANT $type => i16, $($variant => $number),+);
        impl_enum_bee_msg_traits!(INT_VARIANT $type => u32, $($variant => $number),+);
        impl_enum_bee_msg_traits!(INT_VARIANT $type => i32, $($variant => $number),+);
        impl_enum_bee_msg_traits!(INT_VARIANT $type => u64, $($variant => $number),+);
        impl_enum_bee_msg_traits!(INT_VARIANT $type => i64, $($variant => $number),+);
        impl_enum_bee_msg_traits!(INT_VARIANT $type => usize, $($variant => $number),+);
        impl_enum_bee_msg_traits!(INT_VARIANT $type => isize, $($variant => $number),+);
    };

    (INT_VARIANT $type:ty => $int_type:ty, $($variant:ident => $number:literal),+) => {
        impl crate::bee_serde::BeeSerdeConversion<$int_type> for $type {
            fn try_from_bee_serde(value: $int_type) -> ::anyhow::Result<Self> {
                match value {
                    $(
                        $number => Ok(Self::$variant),
                    )+
                    t => Err(anyhow::anyhow!("Invalid value {t} for conversion into enum {}", stringify!($type))),
                }
            }

            fn into_bee_serde(self) -> $int_type {
                let res = match self {
                    $(
                        <$type>::$variant => $number,
                    )+
                };

                res
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
