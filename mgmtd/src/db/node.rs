//! Functions for node management

use super::*;
use std::time::Duration;

/// Represents a node entry.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub uid: Uid,
    pub id: NodeId,
    pub node_type: NodeType,
    pub alias: String,
    pub port: Port,
}

impl Node {
    fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Node {
            uid: row.get(0)?,
            id: row.get(1)?,
            node_type: NodeType::from_row(row, 2)?,
            alias: row.get(3)?,
            port: row.get(4)?,
        })
    }
}

/// Retrieve a list of nodes filtered by node type.
pub(crate) fn get_with_type(tx: &Transaction, node_type: NodeType) -> Result<Vec<Node>> {
    Ok(tx.query_map_collect(
        sql!(
            "SELECT node_uid, node_id, node_type, alias, port
            FROM all_nodes_v
            WHERE node_type = ?1"
        ),
        [node_type.sql_str()],
        Node::from_row,
    )?)
}

/// Retrieve a node by its alias.
pub(crate) fn get_by_alias(tx: &Transaction, alias: &str) -> Result<Node> {
    Ok(tx.query_row(
        sql!(
            "SELECT node_uid, node_id, node_type, alias, port
            FROM all_nodes_v
            WHERE alias = ?1"
        ),
        [alias],
        Node::from_row,
    )?)
}

/// Delete client nodes with a last contact time bigger than `timeout`.
///
/// # Return value
/// Returns the number of deleted clients.
pub(crate) fn delete_stale_clients(tx: &Transaction, timeout: Duration) -> Result<usize> {
    let affected = {
        let mut stmt = tx.prepare_cached(sql!(
            "DELETE FROM nodes
            WHERE DATETIME(last_contact) < DATETIME('now', '-' || ?1 || ' seconds')
                AND node_uid IN (SELECT node_uid FROM client_nodes)"
        ))?;
        stmt.execute(params![timeout.as_secs()])?
    };

    Ok(affected)
}

/// Inserts a node into the database. If node_id is 0, a new ID is chosen automatically.
pub(crate) fn insert(
    tx: &Transaction,
    node_id: NodeId,
    alias: Option<Alias>,
    node_type: NodeType,
    port: Port,
) -> Result<(Uid, NodeId)> {
    let node_id = if node_id == 0 {
        if node_type == NodeType::Client {
            // Immediately reusing client IDs is not possible because nodes only learn about removed
            // clients periodically when they download the node lists via the InternodeSyncer. At
            // most this could take up to 10 minutes. The old mgmtd ensured client IDs were
            // generated sequentially, and only once the u32 wrapped would it allow IDs to be reused
            // from the bottom part of the range. This implements the same behavior.
            let last_id: u32 = config::get(tx, config::Config::CounterLastClientID)?.unwrap_or(0);
            let min_id = if last_id == u32::MAX { 1 } else { last_id + 1 };
            // Generally the new_id will always be last_id+1. However after the u32 wraps it is
            // theoretically possible (though highly unlikely) we encounter an ID that is already in
            // use. The use of find_lowest_unused_id avoids ever assigning an already in use ID to a
            // new client, though doesn't completely guarantee we don't reuse a client ID that was
            // just released but still associated on the meta and storage nodes with the alias of an
            // client that no longer exists. This is HIGHLY unlikely as it would mean a client had
            // BeeGFS mounted for a REALLY long time and that client just happened to be unmounted
            // right before another client mount happened.
            let new_id = misc::find_new_id(tx, "client_nodes", "node_id", min_id..=u32::MAX)?;
            config::set(tx, config::Config::CounterLastClientID, new_id)?;
            new_id
        } else {
            // All other node types:
            misc::find_new_id(
                tx,
                &format!("{}_nodes", node_type.sql_str()),
                "node_id",
                1..=0xFFFF,
            )?
        }
    } else {
        if let Some(node) = try_resolve_num_id(tx, EntityType::Node, node_type, node_id)? {
            bail!("{node} already exists");
        }

        node_id
    };

    let alias = if let Some(alias) = alias {
        alias
    } else {
        format!("node_{}_{}", node_type.sql_str(), node_id).try_into()?
    };

    let uid = entity::insert(tx, EntityType::Node, &alias)?;

    tx.execute_cached(
        sql!(
            "INSERT INTO nodes (node_uid, node_type, port, last_contact)
            VALUES (?1, ?2, ?3, DATETIME('now'))"
        ),
        params![uid, node_type.sql_str(), port],
    )?;

    tx.execute_cached(
        &format!(
            "INSERT INTO {}_nodes (node_id, node_uid) VALUES (?1, ?2)",
            node_type.sql_str()
        ),
        params![node_id, uid],
    )?;

    Ok((uid, node_id))
}

/// Updates a node in the database.
pub(crate) fn update(tx: &Transaction, node_uid: Uid, new_port: Port) -> Result<()> {
    let affected = tx.execute_cached(
        sql!("UPDATE nodes SET port = ?1, last_contact = DATETIME('now') WHERE node_uid = ?2"),
        params![new_port, node_uid],
    )?;

    check_affected_rows(affected, [1])
}

/// Updates the `last_contact` time for all the nodes belonging to the passed targets.
///
/// BeeGFS considers contact times belonging to targets and only provides target ids in the messages
/// that are used to update these. This doesn't make sense (a node is the entity that can be
/// unreachable, not a target), but since there is currently no way to know from which node these
/// messages come, the nodes to update are determined using target IDs.
pub(crate) fn update_last_contact_for_targets(
    tx: &Transaction,
    target_ids: &[TargetId],
    node_type: NodeTypeServer,
) -> Result<usize> {
    Ok(tx.execute_cached(
        sql!(
            "UPDATE nodes AS n SET last_contact = DATETIME('now')
            WHERE n.node_uid IN (
            SELECT DISTINCT node_uid FROM all_targets_v
            WHERE target_id IN rarray(?1) AND node_type = ?2)"
        ),
        params![
            &rarray_param(target_ids.iter().copied()),
            node_type.sql_str()
        ],
    )?)
}

/// Delete a node from the database.
pub(crate) fn delete(tx: &Transaction, node_uid: Uid) -> Result<()> {
    let affected = tx.execute_cached(
        sql!("DELETE FROM nodes WHERE node_uid = ?1"),
        params![node_uid],
    )?;

    check_affected_rows(affected, [1])
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn insert_get_delete() {
        with_test_data(|tx| {
            assert_eq!(5, get_with_type(tx, NodeType::Meta).unwrap().len());
            let (uid, _) = insert(
                tx,
                1234,
                Some("new_node".try_into().unwrap()),
                NodeType::Meta,
                10000,
            )
            .unwrap();
            insert(
                tx,
                1234,
                Some("new_node_2".try_into().unwrap()),
                NodeType::Meta,
                10000,
            )
            .unwrap_err();
            insert(
                tx,
                1235,
                Some("new_node".try_into().unwrap()),
                NodeType::Meta,
                10000,
            )
            .unwrap_err();
            assert_eq!(6, get_with_type(tx, NodeType::Meta).unwrap().len());

            delete(tx, uid).unwrap();
            delete(tx, uid).unwrap_err();
            assert_eq!(5, get_with_type(tx, NodeType::Meta).unwrap().len());
        });
    }

    #[test]
    fn query_by_alias() {
        with_test_data(|tx| {
            insert(
                tx,
                11,
                Some("node_1".try_into().unwrap()),
                NodeType::Meta,
                10000,
            )
            .unwrap();
            insert(
                tx,
                12,
                Some("node_2".try_into().unwrap()),
                NodeType::Storage,
                10000,
            )
            .unwrap();
            assert_eq!(11, get_by_alias(tx, "node_1").unwrap().id);
        })
    }

    #[test]
    fn delete_stale_clients() {
        with_test_data(|tx| {
            let deleted = super::delete_stale_clients(tx, Duration::from_secs(99999)).unwrap();
            assert_eq!(0, deleted);

            tx.execute(
                r#"
                UPDATE nodes
                SET last_contact = DATETIME("now", "-1 hour")
                WHERE node_uid IN (103001, 103002)
                "#,
                [],
            )
            .unwrap();

            let deleted = super::delete_stale_clients(tx, Duration::from_secs(100)).unwrap();
            assert_eq!(2, deleted);

            let clients = node::get_with_type(tx, NodeType::Client).unwrap();
            assert_eq!(2, clients.len());
        })
    }

    #[test]
    fn update_last_contact_for_targets() {
        with_test_data(|tx| {
            super::update_last_contact_for_targets(tx, &[1, 2], NodeTypeServer::Meta).unwrap();
        })
    }
}
