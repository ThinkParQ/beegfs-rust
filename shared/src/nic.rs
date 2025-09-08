use crate::types::NicType;
use anyhow::{Result, anyhow};
use serde::Deserializer;
use serde::de::{Unexpected, Visitor};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::str::FromStr;

/// Network protocol
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// A filter entry for matching nics
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NicFilter {
    pub invert: bool,
    pub name: Option<String>,
    pub address: Option<IpAddr>,
    pub protocol: Option<Protocol>,
    pub nic_type: Option<NicType>,
}

impl NicFilter {
    const EXPECT_STR: &str =
        "a nic filter in the form \"[!] [<name>|*] [<addr>|*] [4|6|*] [tcp|rdma|*]\"";

    /// Parses a string in the form `[!] [name] [addr] [protocol] [type]` into a [NicFilter]
    #[rustfmt::skip] // opt out because if let chaings are misformatted
    pub fn parse_optional(input: &str) -> Option<Self> {
        let mut split = input.split_whitespace().peekable();
        let mut res = Self::default();

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
    pub fn parse(input: &str) -> Result<Self> {
        Self::parse_optional(input).ok_or_else(|| anyhow!(Self::EXPECT_STR))
    }
}

#[derive(Debug, Default)]
struct NicFilterVisitor;

impl Visitor<'_> for NicFilterVisitor {
    type Value = NicFilter;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(NicFilter::EXPECT_STR)
    }

    fn visit_str<E>(self, input: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        NicFilter::parse_optional(input)
            .ok_or_else(|| E::invalid_value(Unexpected::Str(input), &self))
    }
}

impl<'de> serde::Deserialize<'de> for NicFilter {
    fn deserialize<D>(de: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        de.deserialize_str(NicFilterVisitor)
    }
}

// NIC FILTERING AND QUERYING

/// Returns a priority for a given nic info based on the filter list. Returns `None` if there is
/// no match or the nic is matched on a `!` entry.
fn nic_priority(filter: &[NicFilter], name: &str, ip: &IpAddr) -> Option<usize> {
    // Always ignore link local addresses
    if match ip {
        IpAddr::V4(a) => a.is_link_local(),
        IpAddr::V6(a) => a.is_unicast_link_local(),
    } {
        return None;
    }

    if filter.is_empty() {
        return Some(0);
    }

    for (i, fil) in filter.iter().enumerate() {
        if fil.name.as_ref().is_some_and(|e| e != name) {
            continue;
        }
        if fil.address.as_ref().is_some_and(|e| e != ip) {
            continue;
        }
        if fil.protocol.as_ref().is_some_and(|e| match e {
            Protocol::IPv4 => ip.is_ipv6(),
            Protocol::IPv6 => ip.is_ipv4(),
        }) {
            continue;
        }
        // We don't detect rdma interfaces yet
        if fil.nic_type.as_ref().is_some_and(|e| match e {
            NicType::Ethernet => false,
            NicType::Rdma => true,
        }) {
            continue;
        }

        if fil.invert {
            return None;
        } else {
            return Some(i);
        }
    }

    None
}

/// A local interfaces address (a "Nic" in BeeGFS terms) including a priority for manual sorting
/// priority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nic {
    pub address: IpAddr,
    pub nic_type: NicType,
    pub name: String,
    pub priority: usize,
}
impl PartialOrd for Nic {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nic {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            // Put loopbacks last ( = non-loopbacks first)
            .then_with(|| self.address.is_loopback().cmp(&other.address.is_loopback()))
            // Then prioritize ipv4
            .then_with(|| other.address.is_ipv4().cmp(&self.address.is_ipv4()))
            // Then rdma interfaces
            .then_with(|| self.nic_type.cmp(&other.nic_type))
            .then_with(|| self.address.cmp(&other.address))
            .then_with(|| self.name.cmp(&other.name))
    }
}

/// Retrieve the systems available network interfaces with their addresses
///
/// Only interfaces matching one of the given names in `filter` will be returned, unless the list
/// is empty.
pub fn query_nics(filter: &[NicFilter]) -> Result<Vec<Nic>> {
    let mut filtered_nics = vec![];

    for interface in pnet_datalink::interfaces() {
        for ip in interface.ips {
            if let Some(priority) = nic_priority(filter, &interface.name, &ip.ip()) {
                filtered_nics.push(Nic {
                    name: interface.name.clone(),
                    address: ip.ip(),
                    nic_type: NicType::Ethernet,
                    priority,
                });
            }
        }
    }

    filtered_nics.sort();

    Ok(filtered_nics)
}

/// Selects address to bind to for listening: Checks if IPv6 sockets are available on this host
/// according to our rules: IPv6 must be enabled during boot and at runtime, and IPv6 sockets must
/// be dual stack. Then it returns `::` (IPv6), otherwise `0.0.0.0` (IPv4).
pub fn select_bind_addr(port: u16) -> SocketAddr {
    // SAFETY: Any data used in the libc calls is local only
    unsafe {
        // Check if IPv6 socket can be created
        let sock = libc::socket(libc::AF_INET6, libc::SOCK_STREAM, 0);
        if sock < 0 {
            log::info!("IPv6 is unavailable on this host, falling back to IPv4 sockets");
            return SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port);
        }
        // Make sure the socket is closed on drop
        let sock = OwnedFd::from_raw_fd(sock);

        // Check if we can connect the socket to ipv6. We are not interested in an actual connection
        // here but rather if it fails with EADDRNOTAVAIL, which indicates ipv6 is loaded in
        // kernel but disabled at runtime
        libc::fcntl(sock.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
        let addr_in6 = libc::sockaddr_in6 {
            sin6_family: libc::AF_INET6 as u16,
            sin6_port: libc::htons(port),
            sin6_flowinfo: 0,
            sin6_addr: libc::in6_addr {
                s6_addr: Ipv6Addr::LOCALHOST.octets(),
            },
            sin6_scope_id: 0,
        };
        let res = libc::connect(
            sock.as_raw_fd(),
            &addr_in6 as *const _ as *const _,
            size_of::<libc::sockaddr_in6>() as u32,
        );

        if res < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EADDRNOTAVAIL) {
            log::info!("IPv6 is disabled on this host, falling back to IPv4 sockets");
            return SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port);
        }

        // Check if dual stack sockets are enabled by querying the socket option
        let mut ipv6_only: std::ffi::c_int = 0;
        let mut ipv6_only_size = size_of::<std::ffi::c_int>();

        let res = libc::getsockopt(
            sock.as_raw_fd(),
            libc::IPPROTO_IPV6,
            libc::IPV6_V6ONLY,
            &mut ipv6_only as *mut _ as *mut libc::c_void,
            &mut ipv6_only_size as *mut _ as *mut libc::socklen_t,
        );

        if res < 0 || ipv6_only == 1 {
            log::info!(
                "IPv6 dual stack sockets are unavailable on this host, falling back to IPv4 sockets"
            );
            return SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port);
        }
    }

    SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), port)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_nic_filter() {
        let any = NicFilter {
            invert: false,
            name: None,
            address: None,
            protocol: None,
            nic_type: None,
        };

        assert_eq!(NicFilter::parse_optional("").unwrap(), any);
        assert_eq!(NicFilter::parse_optional("*").unwrap(), any);
        assert_eq!(NicFilter::parse_optional("* *").unwrap(), any);
        assert_eq!(NicFilter::parse_optional("* * *").unwrap(), any);
        assert_eq!(NicFilter::parse_optional("* * * *").unwrap(), any);
        assert_eq!(NicFilter::parse_optional("* * * * *").unwrap(), any);
        assert_eq!(NicFilter::parse_optional("* * * * * *").unwrap(), any);

        // Single field
        assert_eq!(
            NicFilter::parse_optional("eth0").unwrap(),
            NicFilter {
                name: Some("eth0".into()),
                ..Default::default()
            }
        );
        assert_eq!(
            NicFilter::parse_optional("* 127.0.0.1").unwrap(),
            NicFilter {
                address: Some("127.0.0.1".parse().unwrap()),
                ..Default::default()
            }
        );
        assert_eq!(
            NicFilter::parse_optional("* * 4").unwrap(),
            NicFilter {
                protocol: Some(Protocol::IPv4),
                ..Default::default()
            }
        );
        assert_eq!(
            NicFilter::parse_optional("* * * tcp").unwrap(),
            NicFilter {
                nic_type: Some(NicType::Ethernet),
                ..Default::default()
            }
        );

        // Additional *
        assert_eq!(
            NicFilter::parse_optional("* * * rdma * * * *").unwrap(),
            NicFilter {
                nic_type: Some(NicType::Rdma),
                ..Default::default()
            }
        );

        // Multiple fields
        assert_eq!(
            NicFilter::parse_optional("eth0 fd00::1 6 rdma").unwrap(),
            NicFilter {
                name: Some("eth0".into()),
                address: Some("fd00::1".parse().unwrap()),
                protocol: Some(Protocol::IPv6),
                nic_type: Some(NicType::Rdma),
                ..Default::default()
            }
        );

        // Inverted
        assert_eq!(
            NicFilter::parse_optional("! * fd00::1").unwrap(),
            NicFilter {
                invert: true,
                address: Some("fd00::1".parse().unwrap()),
                ..Default::default()
            }
        );
        assert_eq!(
            NicFilter::parse_optional("!eth0").unwrap(),
            NicFilter {
                name: Some("!eth0".into()),
                ..Default::default()
            }
        );
    }

    #[test]
    fn match_nic_filter() {
        use super::*;

        let f_prefer_ipv6 = &[
            NicFilter::parse("* * 6").unwrap(),
            NicFilter::parse("* * 4").unwrap(),
        ];
        assert_eq!(
            nic_priority(f_prefer_ipv6, "eth0", &"127.0.0.1".parse().unwrap()),
            Some(1)
        );
        assert_eq!(
            nic_priority(f_prefer_ipv6, "eth0", &"192.168.0.1".parse().unwrap()),
            Some(1)
        );
        assert_eq!(
            nic_priority(f_prefer_ipv6, "eth0", &"fd00::1".parse().unwrap()),
            Some(0)
        );

        let f_prefer_addr = &[
            NicFilter::parse("* fd00::1").unwrap(),
            NicFilter::parse("eth0 192.168.0.1 * *").unwrap(),
            NicFilter::parse("* 192.168.0.2 * *").unwrap(),
            NicFilter::parse("eth2").unwrap(),
        ];
        assert_eq!(
            nic_priority(f_prefer_addr, "eth0", &"192.168.0.2".parse().unwrap()),
            Some(2)
        );
        assert_eq!(
            nic_priority(f_prefer_addr, "eth0", &"192.168.0.1".parse().unwrap()),
            Some(1)
        );
        assert_eq!(
            nic_priority(f_prefer_addr, "eth0", &"fd00::1".parse().unwrap()),
            Some(0)
        );
        assert_eq!(
            nic_priority(f_prefer_addr, "eth1", &"192.168.0.1".parse().unwrap()),
            None
        );
        assert_eq!(
            nic_priority(f_prefer_addr, "eth2", &"fd00::123".parse().unwrap()),
            Some(3)
        );

        let f_invert = &[
            NicFilter::parse("! eth1 * 4").unwrap(),
            NicFilter::parse("! eth2 * 6").unwrap(),
            NicFilter::parse("! lo").unwrap(),
            NicFilter::parse("eth1").unwrap(),
            NicFilter::parse("*").unwrap(),
        ];
        assert_eq!(
            nic_priority(f_invert, "eth0", &"192.168.0.2".parse().unwrap()),
            Some(4)
        );
        assert_eq!(
            nic_priority(f_invert, "eth1", &"192.168.0.2".parse().unwrap()),
            None
        );
        assert_eq!(
            nic_priority(f_invert, "eth1", &"fd00::1".parse().unwrap()),
            Some(3)
        );
        assert_eq!(
            nic_priority(f_invert, "eth2", &"192.168.0.2".parse().unwrap()),
            Some(4)
        );
        assert_eq!(
            nic_priority(f_invert, "eth2", &"fd00::1".parse().unwrap()),
            None
        );
        assert_eq!(
            nic_priority(f_invert, "lo", &"fd00::1".parse().unwrap()),
            None
        );
    }

    #[test]
    fn sort_nics() {
        let mut nics = [
            Nic {
                address: IpAddr::from_str("127.0.0.1").unwrap(),
                nic_type: NicType::Ethernet,
                name: "a".into(),
                priority: 0,
            },
            Nic {
                address: IpAddr::from_str("192.168.0.2").unwrap(),
                nic_type: NicType::Ethernet,
                name: "b".into(),
                priority: 1,
            },
            Nic {
                address: IpAddr::from_str("192.168.0.1").unwrap(),
                nic_type: NicType::Ethernet,
                name: "a".into(),
                priority: 0,
            },
            Nic {
                address: IpAddr::from_str("192.168.0.3").unwrap(),
                nic_type: NicType::Rdma,
                name: "a".into(),
                priority: 0,
            },
            Nic {
                address: IpAddr::from_str("::1").unwrap(),
                nic_type: NicType::Ethernet,
                name: "a".into(),
                priority: 0,
            },
        ];

        nics.sort();

        assert_eq!(nics[0].address, IpAddr::from_str("192.168.0.3").unwrap());
        assert_eq!(nics[1].address, IpAddr::from_str("192.168.0.1").unwrap());
        assert_eq!(nics[2].address, IpAddr::from_str("127.0.0.1").unwrap());
        assert_eq!(nics[3].address, IpAddr::from_str("::1").unwrap());
        assert_eq!(nics[4].address, IpAddr::from_str("192.168.0.2").unwrap());
    }
}
