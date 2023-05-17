use super::*;
use rusqlite::ToSql;
use std::net::Ipv4Addr;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct NodeNic {
    pub uid: NodeUID,
    pub node_uid: NodeUID,
    pub addr: Ipv4Addr,
    pub port: Port,
    pub nic_type: NicType,
    pub alias: EntityAlias,
}

pub(crate) fn with_type(tx: &mut Transaction, node_type: NodeType) -> Result<Vec<NodeNic>> {
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

pub(crate) fn with_node_uid(tx: &mut Transaction, node_uid: NodeUID) -> Result<Vec<NodeNic>> {
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

fn fetch(tx: &mut Transaction, stmt: &str, params: &[&dyn ToSql]) -> Result<Vec<NodeNic>> {
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

#[cfg(test)]
mod test {
    use super::*;
    use tests::with_test_data;

    #[test]
    fn with_type() {
        with_test_data(|tx| {
            let nics = super::with_type(tx, NodeType::Meta).unwrap();
            assert_eq!(7, nics.len());
        })
    }

    #[test]
    fn with_node_uid() {
        with_test_data(|tx| {
            let nics = super::with_node_uid(tx, 102001.into()).unwrap();
            assert_eq!(4, nics.len());
        })
    }
}
