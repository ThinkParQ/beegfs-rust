//! This library provides shared code for BeeGFS Rust projects.
//!
//! It contains the network [message definitions](msg), [types] to build them
//! from, the [serializer](bee_serde), [connection](conn) handling and other utilities and
//! definitions.

pub mod bee_serde;
pub mod conn;
pub mod journald_logger;
pub mod msg;
pub mod parser;
pub mod shutdown;
pub mod types;

use anyhow::{bail, Result};
use std::net::IpAddr;
// Reexport for convenience
pub use types::*;

/// Retrieve the systems available network interfaces with their addresses
pub fn network_interfaces(filter: &[impl AsRef<str>]) -> Result<Vec<Nic>> {
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
                filtered_nics.push(Nic {
                    addr: ipv4,
                    alias: interface.name.as_str().into(),
                    nic_type: NicType::Ethernet,
                });
            }
        }
    }

    Ok(filtered_nics)
}

/// Logs any error that implements `AsRef<&dyn std::error::Error>` with additional context and its
/// sources.
#[macro_export]
macro_rules! log_error_chain {
    ($err:expr, $fmt:expr $(,$arg:expr)* $(,)?) => {{
        use std::fmt::Write;

        let mut err_string = String::new();

        let mut current_source: Option<&dyn std::error::Error> = Some($err.as_ref());
        while let Some(source) = current_source {
            write!(err_string, ": {}", source).ok();
            current_source = source.source();
        }

        log::error!("{}{}", format_args!($fmt, $($arg,)*), err_string);
    }};
}
