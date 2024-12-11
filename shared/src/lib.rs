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
    let mut filtered_nics = vec![];
    for interface in pnet_datalink::interfaces() {
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

    // Check all filters have been used
    if !filter
        .iter()
        .all(|e| filtered_nics.iter().any(|g| g.name == e.as_ref()))
    {
        bail!("At least one network interface doesn't exist");
    }

    // Sort
    filtered_nics.sort_unstable_by_key(|k| {
        if filter.is_empty() {
            // Move loopbacks to the back
            k.addr.is_loopback() as usize
        } else {
            // Sort by filter
            filter
                .iter()
                .enumerate()
                .find(|e| e.1.as_ref() == k.name)
                .map(|e| e.0)
                .unwrap_or(usize::MAX)
        }
    });

    Ok(filtered_nics)
}
