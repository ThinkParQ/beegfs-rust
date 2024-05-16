//! Functions for node management

use super::*;
use rusqlite::OptionalExtension;
use std::borrow::Cow;
use std::time::Duration;

/// Represents a node entry.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub uid: EntityUID,
    pub id: NodeID,
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
pub(crate) fn get_with_type(tx: &mut Transaction, node_type: NodeType) -> Result<Vec<Node>> {
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

/// Retrieve the global UID for the given node ID and type.
///
/// # Return value
/// Returns `None` if the entry doesn't exist.
pub(crate) fn get_uid(
    tx: &mut Transaction,
    node_id: NodeID,
    node_type: NodeType,
) -> Result<Option<EntityUID>> {
    let res: Option<EntityUID> = tx
        .query_row_cached(
            sql!("SELECT node_uid FROM all_nodes_v WHERE node_id = ?1 AND node_type = ?2"),
            params![node_id, node_type.sql_str()],
            |row| row.get(0),
        )
        .optional()?;

    Ok(res)
}

/// Delete client nodes with a last contact time bigger than `timeout`.
///
/// # Return value
/// Returns the number of deleted clients.
pub(crate) fn delete_stale_clients(tx: &mut Transaction, timeout: Duration) -> Result<usize> {
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
    tx: &mut Transaction,
    node_id: NodeID,
    alias: Option<&str>,
    node_type: NodeType,
    port: Port,
) -> Result<(EntityUID, NodeID)> {
    let node_id = if node_id == 0 {
        misc::find_new_id(
            tx,
            &format!("{}_nodes", node_type.sql_str()),
            "node_id",
            1..=0xFFFF,
        )?
    } else if get_uid(tx, node_id, node_type)?.is_some() {
        bail!(TypedError::value_exists("node ID", node_id));
    } else {
        node_id
    };

    let alias = if let Some(alias) = alias {
        Cow::Borrowed(alias)
    } else {
        Cow::Owned(format!("node_{}_{}", node_type.sql_str(), node_id))
    };

    let uid = entity::insert(tx, EntityType::Node, alias.as_ref())?;

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
pub(crate) fn update(tx: &mut Transaction, node_uid: EntityUID, new_port: Port) -> Result<()> {
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
    tx: &mut Transaction,
    target_ids: &[TargetID],
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
pub(crate) fn delete(tx: &mut Transaction, node_uid: EntityUID) -> Result<()> {
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
            let (uid, _) = insert(tx, 1234, Some("new_node"), NodeType::Meta, 10000).unwrap();
            insert(tx, 1234, Some("new_node_2"), NodeType::Meta, 10000).unwrap_err();
            insert(tx, 1235, Some("new_node"), NodeType::Meta, 10000).unwrap_err();
            assert_eq!(6, get_with_type(tx, NodeType::Meta).unwrap().len());

            delete(tx, uid).unwrap();
            delete(tx, uid).unwrap_err();
            assert_eq!(5, get_with_type(tx, NodeType::Meta).unwrap().len());
        });
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
