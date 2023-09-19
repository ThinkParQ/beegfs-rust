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

/// Implements SQLite support for an enum (without data) by converting its variants into strings.
///
/// The enum can then be used as parameter for a TEXT column.
macro_rules! impl_enum_to_sql_str {
    ($type:ty, $($variant:ident => $text:literal),+ $(,)?) => {

        #[cfg(feature = "rusqlite")]
        impl $type {
            pub fn as_sql_str(&self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $text,
                    )+
                }
            }
        }

        #[cfg(feature = "rusqlite")]
        impl ::rusqlite::types::ToSql for $type {
            fn to_sql(&self) -> ::rusqlite::Result<::rusqlite::types::ToSqlOutput> {
                Ok(::rusqlite::types::ToSqlOutput::Borrowed(
                        ::rusqlite::types::ValueRef::Text(match self {
                            $(
                                Self::$variant => $text.as_bytes(),
                            )+
                        }
                    ),
                ))
            }
        }

        #[cfg(feature = "rusqlite")]
        impl ::rusqlite::types::FromSql for $type {
            fn column_result(
                value: ::rusqlite::types::ValueRef,
            ) -> ::rusqlite::types::FromSqlResult<Self> {
                let raw = String::column_result(value)?;

                match raw.as_str() {
                    $(
                        $text => Ok(Self::$variant),
                    )+
                    _ => Err(::rusqlite::types::FromSqlError::InvalidType),
                }
            }
        }
    };
}
