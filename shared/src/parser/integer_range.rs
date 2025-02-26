//! Custom serde parser for integer ranges
//!
//! Meant for command line argument and config file parsing.

use anyhow::{Result, anyhow};
use regex::Regex;
use serde::Deserializer;
use serde::de::{Unexpected, Visitor as VisitorT};
use std::marker::PhantomData;
use std::ops::RangeInclusive;
use std::str::FromStr;
use std::sync::LazyLock;

static REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+) *(- *(\d+))?$").expect("Regex must be valid"));

const EXPECT_STR: &str = "A single positive integer or a range in the form \"<lower>-<upper>\"";

/// Parses a string in the form `<lower>-<upper>` into a `RangeInclusive<u64>`.
fn parse_optional<T: FromStr + Copy + Ord>(input: &str) -> Option<RangeInclusive<T>> {
    let captures = REGEX.captures(input.trim())?;
    let lower: T = captures.get(1)?.as_str().parse().ok()?;
    let upper = captures.get(3);

    if let Some(upper) = upper {
        let upper: T = upper.as_str().parse().ok()?;

        if upper < lower {
            return None;
        }

        Some(lower..=upper)
    } else {
        Some(lower..=lower)
    }
}

/// Parses a string in the form `<lower>-<upper>` into a `RangeInclusive<u64>`.
pub fn parse<T: FromStr + Copy + Ord>(input: &str) -> Result<RangeInclusive<T>> {
    parse_optional(input).ok_or_else(|| anyhow!(EXPECT_STR))
}

struct Visitor<T> {
    _pd: PhantomData<T>,
}

impl<T> Default for Visitor<T> {
    fn default() -> Self {
        Self { _pd: PhantomData }
    }
}

impl<T: FromStr + Copy + Ord> VisitorT<'_> for Visitor<T> {
    type Value = RangeInclusive<T>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(EXPECT_STR)
    }

    fn visit_str<E>(self, input: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        parse_optional(input).ok_or_else(|| E::invalid_value(Unexpected::Str(input), &self))
    }
}

pub fn deserialize<'de, D: Deserializer<'de>, T: FromStr + Ord + Copy>(
    de: D,
) -> Result<RangeInclusive<T>, D::Error> {
    de.deserialize_str(Visitor::default())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn basic() {
        assert_eq!(parse_optional("100").unwrap(), 100..=100);
        assert_eq!(parse_optional(" 200  ").unwrap(), 200..=200);
        assert_eq!(parse_optional("0-100").unwrap(), 0..=100);
        assert_eq!(parse_optional("0 - 100").unwrap(), 0..=100);

        assert!(parse_optional::<u64>("abc").is_none());
        assert!(parse_optional::<u64>("abc-").is_none());
        assert!(parse_optional::<u64>("1-").is_none());
        assert!(parse_optional::<u64>("-1-100").is_none());
        assert!(parse_optional::<u64>("100-1").is_none());
        assert!(parse_optional::<u64>("").is_none());
    }
}
