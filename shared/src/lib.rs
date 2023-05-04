//! # Overview
//! This library provides functionality to communicate with BeeGFS nodes.
//! It contains the network [message definitions](msg), [types] to build them
//! from, the [serializer](serialization) and [connection](conn) handling.

pub mod bee_serde;
pub mod config;
pub mod conn;
pub mod journald_logger;
pub mod msg;
pub mod parser;
pub mod shutdown;
pub mod types;

use anyhow::{bail, Result};
pub use conn::PeerID;
use std::net::IpAddr;
// Reexport for convenience
pub use types::*;

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
