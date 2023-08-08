//! Functions for node nic management.
use super::*;
use rusqlite::ToSql;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

/// Represents a network interface entry
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct NodeNic {
    pub node_uid: EntityUID,
    pub addr: Ipv4Addr,
    pub port: Port,
    pub nic_type: NicType,
    pub alias: EntityAlias,
}

/// Retrieves all node addresses grouped by EntityUID.
///
/// # Return value
/// A Vec containing (EntityUID, Vec<SocketAddr>) entries.
pub fn get_all_addrs(tx: &mut Transaction) -> DbResult<Vec<(EntityUID, Vec<SocketAddr>)>> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT nn.node_uid, nn.addr, n.port
        FROM node_nics AS nn
        INNER JOIN nodes AS n USING(node_uid)
        ORDER BY nn.node_uid ASC
        "#,
    )?;

    let mut rows = stmt.query([])?;

    let mut res = vec![];
    let mut cur: Option<&mut (EntityUID, Vec<SocketAddr>)> = None;
    while let Some(row) = rows.next()? {
        let node_uid = row.get(0)?;
        let addr = SocketAddr::new(row.get::<_, [u8; 4]>(1)?.into(), row.get(2)?);

        if cur.is_some() && cur.as_ref().unwrap().0 == node_uid {
            cur.as_mut().unwrap().1.push(addr);
        } else {
            res.push((node_uid, vec![addr]));
            cur = res.last_mut();
        }
    }

    Ok(res)
}

/// Retrieves all node nics for the given node type.
///
/// # Return value
/// A Vec containing [NodeNic] entries.
pub fn get_with_type(tx: &mut Transaction, node_type: NodeType) -> DbResult<Arc<[NodeNic]>> {
    fetch(
        tx,
        r#"
        SELECT nn.node_uid, nn.addr, n.port, nn.nic_type, nn.name
        FROM node_nics AS nn
        INNER JOIN nodes AS n USING(node_uid)
        WHERE n.node_type = ?1
        ORDER BY nn.node_uid ASC
        "#,
        params![node_type],
    )
}

/// Retrieves all node nics for the given node by UID.
///
/// # Return value
/// A Vec containing [NodeNic] entries.
pub fn get_with_node_uid(tx: &mut Transaction, node_uid: EntityUID) -> DbResult<Arc<[NodeNic]>> {
    fetch(
        tx,
        r#"
        SELECT nn.node_uid, nn.addr, n.port, nn.nic_type, nn.name
        FROM node_nics AS nn
        INNER JOIN nodes AS n USING(node_uid)
        WHERE n.node_uid = ?1
        "#,
        params![node_uid],
    )
}

fn fetch(tx: &mut Transaction, stmt: &str, params: &[&dyn ToSql]) -> DbResult<Arc<[NodeNic]>> {
    let mut stmt = tx.prepare_cached(stmt)?;

    let nics = stmt
        .query_map(params, |row| {
            Ok(NodeNic {
                node_uid: row.get(0)?,
                addr: row.get::<_, [u8; 4]>(1)?.into(),
                port: row.get(2)?,
                nic_type: row.get(3)?,
                alias: row.get(4)?,
            })
        })?
        .try_collect()?;

    Ok(nics)
}

/// Replaces all node nics for the given node by UID.
pub fn replace(tx: &mut Transaction, node_uid: EntityUID, nics: &[Nic]) -> DbResult<()> {
    tx.execute_cached("DELETE FROM node_nics WHERE node_uid = ?1", [node_uid])?;

    let mut stmt = tx.prepare_cached(
        "INSERT INTO node_nics (node_uid, nic_type, addr, name)
        VALUES (?1, ?2, ?3, ?4)",
    )?;

    for nic in nics {
        stmt.execute(params![
            node_uid,
            nic.nic_type,
            nic.addr.octets(),
            nic.alias
        ])?;
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn get_all_addrs() {
        with_test_data(|tx| {
            let addrs = super::get_all_addrs(tx).unwrap();
            assert_eq!(12, addrs.len());
            assert_eq!(4, addrs[0].1.len());
        })
    }

    #[test]
    fn get_with_type() {
        with_test_data(|tx| {
            let nics = super::get_with_type(tx, NodeType::Meta).unwrap();
            assert_eq!(7, nics.len());
        })
    }

    #[test]
    fn update_get() {
        with_test_data(|tx| {
            let nics = super::get_with_node_uid(tx, 102001.into()).unwrap();
            assert_eq!(4, nics.len());

            super::replace(tx, 102001.into(), &[]).unwrap();

            let nics = super::get_with_node_uid(tx, 102001.into()).unwrap();
            assert_eq!(0, nics.len());

            super::replace(
                tx,
                102001.into(),
                &[Nic {
                    addr: Ipv4Addr::new(1, 2, 3, 4),
                    alias: "test".into(),
                    nic_type: NicType::Ethernet,
                }],
            )
            .unwrap();

            let nics = super::get_with_node_uid(tx, 102001.into()).unwrap();
            assert_eq!(1, nics.len());
        })
    }
}
