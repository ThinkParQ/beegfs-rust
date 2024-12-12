//! Functions for node nic management.
use super::*;
use std::borrow::Cow;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

/// Retrieves all node addresses grouped by EntityUID.
///
/// # Return value
/// A Vec containing (EntityUID, Vec<SocketAddr>) entries.
pub(crate) fn get_all_addrs(tx: &Transaction) -> Result<Vec<(Uid, Vec<SocketAddr>)>> {
    let mut stmt = tx.prepare_cached(sql!(
        "SELECT nn.node_uid, nn.addr, n.port
        FROM node_nics AS nn
        INNER JOIN nodes AS n USING(node_uid)
        ORDER BY nn.node_uid ASC"
    ))?;

    let mut rows = stmt.query([])?;

    let mut res = vec![];
    let mut cur: Option<&mut (Uid, Vec<SocketAddr>)> = None;
    while let Some(row) = rows.next()? {
        let node_uid = row.get(0)?;
        let addr: IpAddr = row.get_ref(1)?.as_str()?.parse()?;
        let addr = SocketAddr::new(addr, row.get(2)?);

        if cur.is_some() && cur.as_ref().unwrap().0 == node_uid {
            #[allow(clippy::unnecessary_unwrap)]
            cur.as_mut().unwrap().1.push(addr);
        } else {
            res.push((node_uid, vec![addr]));
            cur = res.last_mut();
        }
    }

    Ok(res)
}

/// Represents a network interface entry
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct NodeNic {
    pub node_uid: Uid,
    pub addr: IpAddr,
    pub port: Port,
    pub nic_type: NicType,
    pub name: String,
}

impl NodeNic {
    fn from_row(row: &Row) -> Result<Self> {
        Ok(NodeNic {
            node_uid: row.get(0)?,
            addr: row.get_ref(1)?.as_str()?.parse::<IpAddr>()?,
            port: row.get(2)?,
            nic_type: NicType::from_row(row, 3)?,
            name: row.get(4)?,
        })
    }
}

/// Maps a list of NodeNic to an iterator of shared::bee_msg::node::Nic for use with BeeMsg
pub(crate) fn map_bee_msg_nics(
    nics: impl IntoIterator<Item = NodeNic>,
) -> impl Iterator<Item = shared::bee_msg::node::Nic> {
    nics.into_iter()
        // TODO Ipv6: Remove the Ipv4 filter when protocol changes (https://github.com/ThinkParQ/beegfs-rs/issues/145)
        .filter(|e| e.addr.is_ipv4())
        .map(|e| shared::bee_msg::node::Nic {
            addr: e.addr,
            name: e.name.into_bytes(),
            nic_type: e.nic_type,
        })
}

/// Retrieves all node nics for a specific node
pub(crate) fn get_with_node(tx: &Transaction, node_uid: Uid) -> Result<Vec<NodeNic>> {
    tx.prepare_cached(sql!(
        "SELECT nn.node_uid, nn.addr, n.port, nn.nic_type, nn.name
            FROM node_nics AS nn
            INNER JOIN nodes AS n USING(node_uid)
            WHERE nn.node_uid = ?1
            ORDER BY nn.node_uid ASC"
    ))?
    .query_and_then([node_uid], NodeNic::from_row)?
    .collect::<Result<Vec<_>>>()
}

/// Retrieves all node nics for the given node type.
pub(crate) fn get_with_type(tx: &Transaction, node_type: NodeType) -> Result<Arc<[NodeNic]>> {
    tx.prepare_cached(sql!(
        "SELECT nn.node_uid, nn.addr, n.port, nn.nic_type, nn.name
            FROM node_nics AS nn
            INNER JOIN nodes AS n USING(node_uid)
            WHERE n.node_type = ?1
            ORDER BY nn.node_uid ASC"
    ))?
    .query_and_then([node_type.sql_variant()], NodeNic::from_row)?
    .collect::<Result<Arc<_>>>()
}

#[derive(Debug)]
pub(crate) struct ReplaceNic<'a> {
    pub nic_type: NicType,
    pub addr: &'a IpAddr,
    pub name: Cow<'a, str>,
}

/// Replaces all node nics for the given node by UID.
pub(crate) fn replace<'a>(
    tx: &Transaction,
    node_uid: Uid,
    nics: impl IntoIterator<Item = ReplaceNic<'a>>,
) -> Result<()> {
    tx.execute_cached(
        sql!("DELETE FROM node_nics WHERE node_uid = ?1"),
        [node_uid],
    )?;

    let mut stmt = tx.prepare_cached(sql!(
        "INSERT INTO node_nics (node_uid, nic_type, addr, name) VALUES (?1, ?2, ?3, ?4)"
    ))?;

    for nic in nics {
        stmt.execute(params![
            node_uid,
            nic.nic_type.sql_variant(),
            nic.addr.to_string(),
            nic.name
        ])?;
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use shared::types::MGMTD_UID;
    use std::net::Ipv4Addr;

    #[test]
    fn get_all_addrs() {
        with_test_data(|tx| {
            let addrs = super::get_all_addrs(tx).unwrap();
            assert_eq!(12, addrs.len());
            assert_eq!(4, addrs[0].1.len());
        })
    }

    #[test]
    fn get_with_node() {
        with_test_data(|tx| {
            let addrs = super::get_with_node(tx, MGMTD_UID).unwrap();
            assert_eq!(0, addrs.len());

            let addrs = super::get_with_node(tx, 102001).unwrap();
            assert_eq!(4, addrs.len());
        })
    }

    #[test]
    fn get_update() {
        with_test_data(|tx| {
            let nics = super::get_with_type(tx, NodeType::Storage).unwrap();
            assert_eq!(4, nics.iter().filter(|e| e.node_uid == 102001).count());

            super::replace(tx, 102001i64, []).unwrap();

            let nics = super::get_with_type(tx, NodeType::Storage).unwrap();
            assert_eq!(0, nics.iter().filter(|e| e.node_uid == 102001).count());

            super::replace(
                tx,
                102001i64,
                [ReplaceNic {
                    addr: &Ipv4Addr::new(1, 2, 3, 4).into(),
                    name: "test".into(),
                    nic_type: NicType::Ethernet,
                }],
            )
            .unwrap();

            let nics = super::get_with_type(tx, NodeType::Storage).unwrap();
            assert_eq!(1, nics.iter().filter(|e| e.node_uid == 102001).count());
        })
    }
}
