//! Custom serde parser for integers with time (like `"10s"`)
//!
//! Meant for command line argument and config file parsing.

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::de::{Unexpected, Visitor as VisitorT};
use serde::Deserializer;
use std::sync::LazyLock;
use std::time::Duration;

static REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+) *(([num]?s|[mhd])?)$").expect("Regex must be valid"));

const EXPECT_STR: &str =
    "a positive integer representing a time span in seconds or a string containing a \
     positive integer n with appended time unit in the form \"<n>[[n|u|m]s|m|h|d]\"";

/// Parses a time string in the form `<int>[ns|us|ms|s|m|h|d]` into a [Duration]
pub fn parse_optional(input: &str) -> Option<Duration> {
    let captures = REGEX.captures(input.trim())?;
    let number = captures.get(1)?;
    let suffix = captures.get(2)?;

    let number: u64 = number.as_str().parse().ok()?;

    let duration = match suffix.as_str() {
        "ns" => Duration::from_nanos(number),
        "us" => Duration::from_micros(number),
        "ms" => Duration::from_millis(number),
        "s" => Duration::from_secs(number),
        "" => Duration::from_secs(number),
        "m" => Duration::from_secs(number.saturating_mul(60)),
        "h" => Duration::from_secs(number.saturating_mul(60 * 60)),
        "d" => Duration::from_secs(number.saturating_mul(24 * 60 * 60)),
        _ => return None,
    };

    Some(duration)
}

/// Parses a time string in the form `<int>[ns|us|ms|s|m|h|d]` into a [Duration]
pub fn parse(input: &str) -> Result<Duration> {
    parse_optional(input).ok_or_else(|| anyhow!(EXPECT_STR))
}

#[derive(Debug, Default)]
struct Visitor {}

impl VisitorT<'_> for Visitor {
    type Value = Duration;

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
        Ok(Duration::from_secs(input))
    }

    fn visit_i64<E>(self, input: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let secs: u64 = input
            .try_into()
            .map_err(|_| E::invalid_value(Unexpected::Signed(input), &self))?;

        Ok(Duration::from_secs(secs))
    }
}

pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Duration, D::Error> {
    de.deserialize_str(Visitor::default())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parser() {
        assert_eq!(parse_optional("100").unwrap(), Duration::from_secs(100));
        assert_eq!(parse_optional(" 200  ").unwrap(), Duration::from_secs(200));
        assert_eq!(parse_optional("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_optional("500 ns").unwrap(), Duration::from_nanos(500));
        assert_eq!(
            parse_optional("3d").unwrap(),
            Duration::from_secs(3 * 86400)
        );

        assert!(parse_optional("-100").is_none());
        assert!(parse_optional("-100ms").is_none());
        assert!(parse_optional("100mh").is_none());
        assert!(parse_optional("9999999999999999999999s").is_none());
        assert!(parse_optional("garbage").is_none());
        assert!(parse_optional("").is_none());
    }
}
