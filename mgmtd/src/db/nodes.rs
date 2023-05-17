use super::*;
use itertools::Itertools;
use std::time::Duration;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct Node {
    pub uid: NodeUID,
    pub id: NodeID,
    pub node_type: NodeType,
    pub alias: EntityAlias,
    pub port: Port,
}

pub(crate) fn with_type(tx: &mut Transaction, node_type: NodeType) -> Result<Vec<Node>> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT node_uid, node_id, node_type, alias, port
        FROM all_nodes_v
        WHERE node_type = ?1
        "#,
    )?;

    let res = stmt
        .query_map([node_type], |row| {
            Ok(Node {
                uid: row.get(0)?,
                id: row.get(1)?,
                node_type: row.get(2)?,
                alias: row.get(3)?,
                port: row.get(4)?,
            })
        })?
        .try_collect()?;

    Ok(res)
}

fn get_uid(tx: &mut Transaction, id: NodeID, node_type: NodeType) -> Result<NodeUID> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT node_uid FROM all_nodes_v WHERE node_id = ?1 AND node_type = ?2
        "#,
    )?;
    let id = stmt.query_row(params![id, node_type], |row| row.get(0))?;

    Ok(id)
}

pub(crate) fn set(
    tx: &mut Transaction,
    enable_registration: bool,
    node_id: NodeID,
    node_type: NodeType,
    new_alias: EntityAlias,
    new_port: Port,
    new_nic_list: Vec<Nic>,
) -> Result<NodeID> {
    let node_id = if node_id == NodeID::ZERO {
        misc::find_new_id(
            tx,
            &format!("{}_nodes", node_type.as_sql_str()),
            "node_id",
            1..=0xFFFF,
        )?
        .into()
    } else {
        node_id
    };

    let updated = {
        // Try to update existing node
        let mut stmt = tx.prepare_cached(
            r#"
            UPDATE nodes SET alias = ?1, port = ?2, last_contact = DATETIME('now')
            WHERE node_uid = (
                SELECT node_uid FROM all_nodes_v WHERE node_id = ?3 AND node_type = ?4
            )
            "#,
        )?;
        stmt.execute(params![new_alias, new_port, node_id, node_type])?
    };

    // Doesn't exist (yet)
    if 0 == updated {
        {
            if !enable_registration {
                bail!("Registration of new nodes is not allowed");
            }

            // try to insert
            let mut stmt = tx.prepare_cached(
                r#"
                INSERT INTO entities (entity_type) VALUES ("node")
                "#,
            )?;

            stmt.execute(params![])?;

            let new_uid: NodeUID = tx.last_insert_rowid().into();

            let mut stmt = tx.prepare_cached(
                r#"
            INSERT INTO nodes (node_uid, node_type, alias, port, last_contact)
            VALUES (?1, ?2, ?3, ?4, DATETIME('now'))
            "#,
            )?;

            stmt.execute(params![new_uid, node_type, new_alias, new_port])?;

            let mut stmt = tx.prepare_cached(&format!(
                r#"
            INSERT INTO {}_nodes (node_id, node_uid)
            VALUES (?1, ?2)
            "#,
                node_type.as_sql_str()
            ))?;

            stmt.execute(params![node_id, new_uid])?;
        }

        if node_type == NodeType::Meta {
            targets::insert_meta(tx, node_id, &new_alias)?;
        }
    }

    let uid = get_uid(tx, node_id, node_type)?;

    // Delete old nics
    let mut stmt = tx.prepare_cached(
        r#"
        DELETE FROM node_nics WHERE node_uid = ?1
        "#,
    )?;
    stmt.execute([uid])?;

    // Insert new nics
    let mut stmt = tx.prepare_cached(
        r#"
        INSERT INTO node_nics (node_uid, nic_type, addr, name)
        VALUES (?1, ?2, ?3, ?4)
        "#,
    )?;
    for nic in new_nic_list {
        stmt.execute(params![uid, nic.nic_type, nic.addr.octets(), nic.alias])?;
    }

    Ok(node_id)
}

pub(crate) fn update_last_contact_for_targets(
    tx: &mut Transaction,
    target_ids: impl IntoIterator<Item = TargetID>,
    node_type: NodeType,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(&format!(
        r#"
        UPDATE nodes AS n SET last_contact = DATETIME('now')
        WHERE n.node_uid IN (
            SELECT DISTINCT node_uid FROM all_targets_v WHERE target_id IN ({}) AND node_type = ?1
        )
        "#,
        target_ids.into_iter().join(",")
    ))?;

    stmt.execute(params![node_type])?;

    Ok(())
}

pub(crate) fn delete(tx: &mut Transaction, node_id: NodeID, node_type: NodeType) -> Result<()> {
    let node_uid: NodeUID = tx.query_row(
        r#"
        SELECT node_uid FROM all_nodes_v WHERE node_id = ?1 AND node_type = ?2
        "#,
        params![node_id, node_type],
        |row| row.get(0),
    )?;

    let affected = tx.execute(
        r#"
        DELETE FROM nodes WHERE node_uid = ?1
        "#,
        params![node_uid],
    )?;
    ensure_rows_modified!(affected, node_id, node_type);

    Ok(())
}

pub(crate) fn delete_stale_clients(tx: &mut Transaction, timeout: Duration) -> Result<usize> {
    let affected = {
        let mut stmt = tx.prepare_cached(
            r#"
            DELETE FROM nodes
            WHERE
                DATETIME(last_contact) < DATETIME('now', '-' || ?1 || ' seconds')
                AND node_uid IN (SELECT node_uid FROM client_nodes)
            "#,
        )?;
        stmt.execute(params![timeout.as_secs()])?
    };

    Ok(affected)
}

#[cfg(test)]
mod test {
    use super::*;
    use tests::with_test_data;

    #[test]
    fn set_get() {
        let sn =
            move |tx: &mut Transaction, id: u16, alias: &'static str, enable_registration: bool| {
                set(
                    tx,
                    enable_registration,
                    id.into(),
                    NodeType::Meta,
                    alias.into(),
                    Port::from(8000),
                    vec![],
                )
            };

        with_test_data(|tx| {
            // Existing node
            sn(tx, 1, "existing_node", false).unwrap();
            sn(tx, 1, "existing_node", true).unwrap();
            // New node, auto ID
            sn(tx, 0, "new_node_1", true).unwrap();
            // New node, manual ID
            sn(tx, 1234, "new_node_2", true).unwrap();
            // New node not allowed
            sn(tx, 1235, "new_node_3", false).unwrap_err();
            // Non unique alias
            sn(tx, 1235, "existing_node", true).unwrap_err();

            let nodes = with_type(tx, NodeType::Meta).unwrap();

            // 2 new nodes added to test data
            assert_eq!(nodes.len(), 4 + 2);
            assert!(nodes.iter().any(|n| n.id == 1234.into()));
            assert!(nodes.iter().any(|n| n.alias == "new_node_1".into()));
            assert!(nodes.iter().any(|n| n.alias == "new_node_2".into()));
        });
    }
}
