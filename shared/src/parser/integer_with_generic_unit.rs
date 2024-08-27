//! Custom serde parser for integers with arbitrary units (like `"10kiB"`)
//!
//! Meant for command line argument and config file parsing.

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::de::{Unexpected, Visitor};
use serde::{Deserializer, Serializer};
use std::sync::LazyLock;

static REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d+) *([kMGTPE]?i?)[[:alpha:]]*$").expect("Regex must be valid")
});

const EXPECT_STR: &str =
    "a positive integer representing the base value or a string containing a positive \
     integer n with appended arbitrary unit with optional SI prefix in the form \
     \"<integer>[k|M|G|T|P|E][i][unit]\"";

/// Parses a string in the form `<int>[kMGTPE][i]<unit>` into an integer.
///
/// Takes the given integer and multiplies it according to the given SI suffix, using base 10
/// (`10k` becomes 10000). When the `[i]` is given, base 2 is used (`10kiB` becomes 10240).
///
/// The `<unit>` suffix is ignored and can be anything or be omitted.
pub fn parse_optional(input: &str) -> Option<u64> {
    let captures = REGEX.captures(input.trim())?;
    let number = captures.get(1)?;
    let suffix = captures.get(2)?;

    let number: u64 = number.as_str().parse().ok()?;

    let number = number.saturating_mul(match suffix.as_str() {
        "" => 1,
        "k" => 10u64.pow(3),
        "M" => 10u64.pow(6),
        "G" => 10u64.pow(9),
        "T" => 10u64.pow(12),
        "P" => 10u64.pow(15),
        "E" => 10u64.pow(18),

        "ki" => 2u64.pow(10), // 1024
        "Mi" => 2u64.pow(20), // 1024^2
        "Gi" => 2u64.pow(30),
        "Ti" => 2u64.pow(40),
        "Pi" => 2u64.pow(50),
        "Ei" => 2u64.pow(60),
        _ => return None,
    });

    Some(number)
}

pub fn parse(input: &str) -> Result<u64> {
    parse_optional(input).ok_or_else(|| anyhow!(EXPECT_STR))
}

struct CustomVisitor {}

impl<'a> Visitor<'a> for CustomVisitor {
    type Value = u64;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(EXPECT_STR)
    }

    fn visit_str<E>(self, input: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        parse_optional(input).ok_or_else(|| E::invalid_value(Unexpected::Str(input), &self))
    }

    fn visit_u64<E>(self, input: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(input)
    }

    // Need to parse signed integer since  the TOML parser always parses as i64
    fn visit_i64<E>(self, input: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let v = input
            .try_into()
            .map_err(|_| E::invalid_value(Unexpected::Signed(input), &self))?;

        self.visit_u64(v)
    }
}

pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<u64, D::Error> {
    de.deserialize_any(CustomVisitor {})
}

pub fn serialize<S: Serializer>(input: &u64, ser: S) -> Result<S::Ok, S::Error> {
    // TODO atm we only serialize to u64, but for user facing output it might be
    // nice to serialize to a prefix string instead
    ser.serialize_u64(*input)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn basic() {
        assert_eq!(parse_optional("100").unwrap(), 100);
        assert_eq!(parse_optional(" 200  ").unwrap(), 200);
        assert_eq!(parse_optional("100k").unwrap(), 100_000);
        assert_eq!(parse_optional("100 k").unwrap(), 100_000);
        assert_eq!(parse_optional("123 M").unwrap(), 123_000_000);
        assert_eq!(parse_optional("0 T").unwrap(), 0);
        assert_eq!(parse_optional("1ki").unwrap(), 1024);
        assert_eq!(parse_optional("2 ki ").unwrap(), 2048);
        assert_eq!(parse_optional("1000 Mi").unwrap(), 1000 * 1024 * 1024);

        assert!(parse_optional("Ti").is_none());
        assert!(parse_optional("100 i").is_none());
        assert!(parse_optional("-10 k").is_none());
        assert!(parse_optional("garbage").is_none());
        assert!(parse_optional("").is_none());
    }
}
