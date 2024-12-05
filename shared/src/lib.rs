//! This library provides shared code for BeeGFS Rust projects.
//!
//! It contains the network [message definitions](msg), [types] to build them
//! from, the [serializer](bee_serde), [connection](conn) handling and other utilities and
//! definitions.

#[macro_use]
mod impl_macros;

pub mod bee_msg;
pub mod bee_serde;
pub mod conn;
#[cfg(feature = "grpc")]
pub mod grpc;
pub mod journald_logger;
pub mod parser;
pub mod run_state;
pub mod types;

use anyhow::{bail, Result};
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct NetworkAddr {
    pub addr: IpAddr,
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
            // TODO Ipv6: Remove the Ipv4 filter when protocol changes (https://github.com/ThinkParQ/beegfs-rs/issues/145)
            if !ip.is_ipv4() {
                continue;
            }

            filtered_nics.push(NetworkAddr {
                addr: ip.ip(),
                name: interface.name.clone(),
            });
        }
    }

    Ok(filtered_nics)
}
