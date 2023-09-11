use regex::Regex;
use serde::de::{Unexpected, Visitor};
use serde::{Deserializer, Serializer};
use std::sync::OnceLock;
use std::time::Duration;

static REGEX: OnceLock<Regex> = OnceLock::new();

fn parse(input: &str) -> Result<Duration, ()> {
    let regex = REGEX
        .get_or_init(|| Regex::new(r"^(\d+) *(([num]?s|[mhd])?)$").expect("Regex must be valid"));
    let captures = regex.captures(input.trim()).ok_or(())?;
    let number = captures.get(1).ok_or(())?;
    let suffix = captures.get(2).ok_or(())?;

    let number: u64 = number.as_str().parse().map_err(|_| ())?;

    let duration = match suffix.as_str() {
        "ns" => Duration::from_nanos(number),
        "us" => Duration::from_micros(number),
        "ms" => Duration::from_millis(number),
        "s" => Duration::from_secs(number),
        "" => Duration::from_secs(number),
        "m" => Duration::from_secs(number.saturating_mul(60)),
        "h" => Duration::from_secs(number.saturating_mul(60 * 60)),
        "d" => Duration::from_secs(number.saturating_mul(24 * 60 * 60)),
        _ => return Err(()),
    };

    Ok(duration)
}

struct ValueVisitor {}

impl<'a> Visitor<'a> for ValueVisitor {
    type Value = Duration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            formatter,
            "a positive integer representing a time span in seconds or a string containing a \
             positive integer n with appended time unit in the form \"<n>[[n|u|m]s|m|h|d]\""
        )
    }

    fn visit_str<E>(self, input: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        parse(input).map_err(|_| E::invalid_value(Unexpected::Str(input), &self))
    }

    // Need to parse signed integer since  the TOML parser always parses as i64
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
    de.deserialize_any(ValueVisitor {})
}

pub fn serialize<S: Serializer>(input: &Duration, ser: S) -> Result<S::Ok, S::Error> {
    // TODO atm we only serialize to u64 containig seconds, but for user facing
    // output it might be nice to serialize to a prefix string instead
    ser.serialize_u64(input.as_secs())
}

/// (De)serialize an Option<Duration>
pub mod optional {
    use super::*;

    struct OptionalValueVisitor {}

    impl<'a> Visitor<'a> for OptionalValueVisitor {
        type Value = Option<Duration>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(
                formatter,
                "a positive integer representing a time span in seconds or a string containing a \
                 positive integer n with appended time unit in the form \"<n>[[n|u|m]s|m|h|d]\""
            )
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'a>,
        {
            Ok(Some(deserializer.deserialize_any(ValueVisitor {})?))
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Duration>, D::Error> {
        de.deserialize_option(OptionalValueVisitor {})
    }

    pub fn serialize<S: Serializer>(input: &Option<Duration>, ser: S) -> Result<S::Ok, S::Error> {
        match input {
            Some(input) => ser.serialize_some(input),
            None => ser.serialize_none(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parser() {
        assert_eq!(parse("100").unwrap(), Duration::from_secs(100));
        assert_eq!(parse(" 200  ").unwrap(), Duration::from_secs(200));
        assert_eq!(parse("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse("500 ns").unwrap(), Duration::from_nanos(500));
        assert_eq!(parse("3d").unwrap(), Duration::from_secs(3 * 86400));

        assert!(parse("-100").is_err());
        assert!(parse("-100ms").is_err());
        assert!(parse("100mh").is_err());
        assert!(parse("9999999999999999999999s").is_err());
        assert!(parse("garbage").is_err());
        assert!(parse("").is_err());
    }
}
