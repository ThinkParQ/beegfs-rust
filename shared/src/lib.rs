//! This library provides shared code for BeeGFS Rust projects.
//!
//! It contains the network [message definitions](msg), [types] to build them
//! from, the [serializer](bee_serde), [connection](conn) handling and other utilities and
//! definitions.

#[macro_use]
mod impl_macros;

pub mod bee_serde;
pub mod beemsg;
pub mod conn;
pub mod error;
pub mod journald_logger;
pub mod parser;
pub mod shutdown;
pub mod types;

use anyhow::{bail, Result};
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone)]
pub struct NetworkAddr {
    pub addr: Ipv4Addr,
    pub name: String,
}

/// Retrieve the systems available network interfaces with their addresses
///
/// Only interfaces matching one of the given names in `filter` will be returned, unless the list
/// is empty.
pub fn ethernet_interfaces(filter: &[impl AsRef<str>]) -> Result<Vec<NetworkAddr>> {
    let all_interfaces = pnet_datalink::interfaces();

    for f in filter {
        if !all_interfaces.iter().any(|g| g.name == f.as_ref()) {
            bail!("Network interface {} doesn't exist", f.as_ref());
        }
    }

    let mut filtered_nics = vec![];

    for interface in all_interfaces {
        // if a filter list is specified, filter interfaces by name
        if !filter.is_empty() && !filter.iter().any(|e| interface.name == e.as_ref()) {
            continue;
        }

        for ip in interface.ips {
            if let IpAddr::V4(ipv4) = ip.ip() {
                filtered_nics.push(NetworkAddr {
                    addr: ipv4,
                    name: interface.name.clone(),
                });
            }
        }
    }

    Ok(filtered_nics)
}

/// Stringifies any Error that implements `AsRef<&dyn std::error::Error>` with additional context
/// and its sources.
#[macro_export]
macro_rules! error_chain {
    ($err:expr, $fmt:expr $(,$arg:expr)* $(,)?) => {{
        use std::fmt::Write;

        let mut err_string = String::new();
        write!(err_string, "{}", format_args!($fmt, $($arg,)*)).ok();

        let mut current_source: Option<&dyn std::error::Error> = Some($err.as_ref());
        while let Some(source) = current_source {
            write!(err_string, ": {}", source).ok();
            current_source = source.source();
        }

        err_string
    }};
}

/// Logs any error that implements `AsRef<&dyn std::error::Error>` with additional context and its
/// sources.
#[macro_export]
macro_rules! log_error_chain {
    ($err:expr, $fmt:expr $(,$arg:expr)* $(,)?) => {
        log::error!("{}", $crate::error_chain!($err, $fmt, $($arg,)*));
    };
}
