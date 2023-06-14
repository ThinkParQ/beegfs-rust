//! Functions for node nic management.
use super::*;
use rusqlite::ToSql;
use std::net::Ipv4Addr;

/// Represents a network interface entry
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct NodeNic {
    pub uid: NodeUID,
    pub node_uid: NodeUID,
    pub addr: Ipv4Addr,
    pub port: Port,
    pub nic_type: NicType,
    pub alias: EntityAlias,
}

/// Retrieves all node nics for the given node type.
///
/// # Return value
/// A Vec containing [NodeNic] entries.
pub fn get_with_type(tx: &mut Transaction, node_type: NodeType) -> DbResult<Vec<NodeNic>> {
    fetch(
        tx,
        r#"
        SELECT nn.nic_uid, nn.node_uid, nn.addr, n.port, nn.nic_type, nn.name
        FROM node_nics AS nn
        INNER JOIN nodes AS n USING(node_uid)
        WHERE n.node_type = ?1
        "#,
        params![node_type],
    )
}

/// Retrieves all node nics for the given node by UID.
///
/// # Return value
/// A Vec containing [NodeNic] entries.
pub fn get_with_node_uid(tx: &mut Transaction, node_uid: NodeUID) -> DbResult<Vec<NodeNic>> {
    fetch(
        tx,
        r#"
        SELECT nn.nic_uid, nn.node_uid, nn.addr, n.port, nn.nic_type, nn.name
        FROM node_nics AS nn
        INNER JOIN nodes AS n USING(node_uid)
        WHERE n.node_uid = ?1
        "#,
        params![node_uid],
    )
}

fn fetch(tx: &mut Transaction, stmt: &str, params: &[&dyn ToSql]) -> DbResult<Vec<NodeNic>> {
    let mut stmt = tx.prepare_cached(stmt)?;

    let nics = stmt
        .query_map(params, |row| {
            Ok(NodeNic {
                uid: row.get(0)?,
                node_uid: row.get(1)?,
                addr: row.get::<_, [u8; 4]>(2)?.into(),
                port: row.get(3)?,
                nic_type: row.get(4)?,
                alias: row.get(5)?,
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
    fn with_type() {
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
