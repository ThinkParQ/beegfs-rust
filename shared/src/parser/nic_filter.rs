use crate::types::NicType;
use anyhow::Result;
use anyhow::anyhow;
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq)]
pub enum Protocol {
    IPv4,
    IPv6,
}

impl FromStr for Protocol {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "4" => Ok(Self::IPv4),
            "6" => Ok(Self::IPv6),
            s => Err(anyhow!("{s} is not a valid protocol")),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Default)]
pub struct NicFilter {
    pub invert: bool,
    pub name: Option<String>,
    pub address: Option<IpAddr>,
    pub protocol: Option<Protocol>,
    pub nic_type: Option<NicType>,
}

const EXPECT_STR: &str = "a positive integer representing a time span in seconds or a string containing a \
     positive integer n with appended time unit in the form \"<n>[[n|u|m]s|m|h|d]\"";

/// Parses a string in the form `[!] [name] [addr] [protocol] [type]` into a [NicFilter]
#[rustfmt::skip] // opt out because if let chaings are misformatted
pub fn parse_optional(input: &str) -> Option<NicFilter> {
    let mut split = input.split_whitespace().peekable();
    let mut res = NicFilter::default();

    if let Some(field) = split.peek() {
        if *field == "!" {
            res.invert = true;
            split.next();
        }
    }

    if let Some(field) = split.next() && field != "*" {
        res.name = Some(field.to_string());
    }

    if let Some(field) = split.next() && field != "*" {
        res.address = Some(field.parse().ok()?);
    }

    if let Some(field) = split.next() && field != "*" {
        res.protocol = Some(field.parse().ok()?);
    }

    if let Some(field) = split.next() && field != "*" {
        res.nic_type = Some(field.parse().ok()?);
    }

    Some(res)
}

/// Parses a string in the form `[!] [name] [addr] [protocol] [type]` into a [NicFilter]
pub fn parse(input: &str) -> Result<NicFilter> {
    parse_optional(input).ok_or_else(|| anyhow!(EXPECT_STR))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parser() {
        let any = NicFilter {
            invert: false,
            name: None,
            address: None,
            protocol: None,
            nic_type: None,
        };

        // println!("{:?}", parse_optional("*"));
        assert_eq!(parse_optional("*").unwrap(), any);
        assert_eq!(parse_optional("* *").unwrap(), any);
        assert_eq!(parse_optional("* * *").unwrap(), any);
        assert_eq!(parse_optional("* * * *").unwrap(), any);
        assert_eq!(parse_optional("* * * * *").unwrap(), any);
        assert_eq!(parse_optional("* * * * * *").unwrap(), any);
    }
}
