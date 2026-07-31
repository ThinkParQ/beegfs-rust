use super::*;
use itertools::Itertools;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::str::FromStr;

/// Delivers a list of nodes
pub(crate) async fn get_nodes(
    app: &impl App,
    req: pm::GetNodesRequest,
) -> Result<pm::GetNodesResponse> {
    let (mut nodes, nics, mut keys_by_node, meta_root_node, meta_root_buddy_group, fs_uuid) = app
        .read_tx(move |tx| {
            // Fetching the nic list is optional as it causes additional load
            let nics: Vec<(Uid, pm::get_nodes_response::node::Nic)> = if req.include_nics {
                tx.prepare_cached(sql!(
                    "SELECT nn.node_uid, nn.addr, n.port, nn.nic_type, nn.name
                    FROM node_nics AS nn
                    INNER JOIN nodes AS n USING(node_uid)
                    ORDER BY nn.node_uid ASC"
                ))?
                .query_and_then([], |row| {
                    let nic_type = NicType::from_row(row, 3)?.into_proto_i32();

                    Ok((
                        row.get(0)?,
                        pm::get_nodes_response::node::Nic {
                            addr: row.get(1)?,
                            name: row.get(4)?,
                            nic_type,
                        },
                    ))
                })?
                .collect::<Result<Vec<_>>>()?
            } else {
                vec![]
            };

            // Get all the public keys assigned to a node, grouped by node
            let keys_by_node: HashMap<(NodeId, i32), Vec<Vec<u8>>> = itertools::process_results(
                tx.prepare_cached(sql!(
                    "SELECT node_id, node_type, key
                    FROM keys
                    INNER JOIN identity_to_node AS idn USING(identity_id)"
                ))?
                .query_and_then(
                    [],
                    |row| -> Result<((NodeId, i32), Vec<u8>)> {
                        Ok((
                            (row.get(0)?, NodeType::from_row(row, 1)?.into_proto_i32()),
                            row.get(2)?,
                        ))
                    },
                )?,
                |iter| iter.into_group_map(),
            )?;

            // Fetch the node list
            let nodes: Vec<pm::get_nodes_response::Node> = tx.query_map_collect(
                sql!("SELECT node_uid, node_id, node_type, alias, port FROM nodes_ext"),
                [],
                |row| {
                    let node_type = NodeType::from_row(row, 2)?.into_proto_i32();

                    let node = pb::EntityIdSet {
                        uid: row.get(0)?,
                        legacy_id: Some(pb::LegacyId {
                            num_id: row.get(1)?,
                            node_type,
                        }),
                        alias: row.get(3)?,
                    };

                    Ok(pm::get_nodes_response::Node {
                        id: Some(node),
                        node_type,
                        port: row.get(4)?,
                        nics: vec![],
                        public_key: vec![],
                    })
                },
            )?;

            // Figure out the meta root node and buddy mirror information
            let maybe_row = tx
                .query_row_cached(
                    sql!(
                        "SELECT
                            COALESCE(mn.node_uid, mn2.node_uid),
                            COALESCE(e.alias, e2.alias),
                            COALESCE(mn.node_id, mn2.node_id),
                            mg.group_id,
                            mg.group_uid,
                            ge.alias
                        FROM root_inode as ri
                        LEFT JOIN targets AS mt USING(node_type, target_id)
                        LEFT JOIN nodes AS mn ON mn.node_id = mt.node_id
                            AND mn.node_type = mt.node_type
                        LEFT JOIN entities AS e ON e.uid = mn.node_uid
                        LEFT JOIN buddy_groups AS mg USING(node_type, group_id)
                        LEFT JOIN entities AS ge ON ge.uid = mg.group_uid
                        LEFT JOIN targets AS mt2 ON mt2.target_id = mg.p_target_id
                            AND mt2.node_type = mg.node_type
                        LEFT JOIN nodes AS mn2 ON mn2.node_id = mt2.node_id
                            AND mn2.node_type = mg.node_type
                        LEFT JOIN entities AS e2 ON e2.uid = mn2.node_uid"
                    ),
                    [],
                    |row| {
                        Ok((
                            row.get::<_, Uid>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, NodeId>(2)?,
                            row.get::<_, Option<NodeId>>(3)?,
                            row.get::<_, Option<Uid>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                        ))
                    },
                )
                .optional()?;

            let (meta_root_node, meta_root_buddy_group) =
                if let Some((uid, alias, num_id, bg_num_id, bg_uid, bg_alias)) = maybe_row {
                    let meta_root_node = Some(EntityIdSet {
                        uid,
                        alias: alias.try_into()?,
                        legacy_id: LegacyId {
                            node_type: NodeType::Meta,
                            num_id,
                        },
                    });

                    let meta_root_buddy_group = if let (Some(num_id), Some(uid), Some(alias)) =
                        (bg_num_id, bg_uid, bg_alias)
                    {
                        Some(EntityIdSet {
                            uid,
                            alias: alias.try_into()?,
                            legacy_id: LegacyId {
                                node_type: NodeType::Meta,
                                num_id,
                            },
                        })
                    } else {
                        None
                    };

                    (meta_root_node, meta_root_buddy_group)
                } else {
                    (None, None)
                };

            let fs_uuid = db::config::get(tx, db::config::Config::FsUuid)
                .context("Could not read file system UUID from database")?;

            Ok((
                nodes,
                nics,
                keys_by_node,
                meta_root_node,
                meta_root_buddy_group,
                fs_uuid,
            ))
        })
        .await?;

    if req.include_nics {
        for node in &mut nodes {
            node.nics = nics
                .iter()
                .filter(|(uid, _)| node.id.as_ref().is_some_and(|e| e.uid == Some(*uid)))
                .cloned()
                .map(|(_, mut nic)| {
                    nic.addr = SocketAddr::new(
                        IpAddr::from_str(&nic.addr).unwrap_or(Ipv6Addr::UNSPECIFIED.into()),
                        node.port as u16,
                    )
                    .to_string();
                    nic
                })
                .collect();
        }
    }

    // Insert public keys into the node list
    for node in &mut nodes {
        let Some(legacy_id) = node.id.as_ref().and_then(|e| e.legacy_id.as_ref()) else {
            continue;
        };

        let db_keys = keys_by_node.remove(&(legacy_id.num_id, legacy_id.node_type));

        // Management answers with the key from the keypair it actually loaded rather than from the
        // `keys` table. That keeps the key file the single source of truth: replacing it takes
        // effect at once, instead of leaving a stale row that peers would use to reject us. It also
        // means the management node does not have to be registered by hand for anything to be able
        // to connect to it.
        if node.id.as_ref().and_then(|e| e.uid) == Some(MGMTD_UID) {
            if db_keys.is_some() {
                log::warn!(
                    "The `keys` table contains an entry for the management node. It is ignored - \
                     management's own public key is taken from {:?}. Remove the entry to avoid \
                     confusion.",
                    app.static_info().user_config.key_file
                );
            }

            node.public_key = match &app.static_info().static_keypair {
                Some(keypair) => vec![keypair.public().as_bytes().to_vec()],
                // Authentication is disabled, so there is no key exchange to advertise a key for.
                None => vec![],
            };

            continue;
        }

        if let Some(node_keys) = db_keys {
            node.public_key = node_keys;
        }
    }
    Ok(pm::GetNodesResponse {
        nodes,
        meta_root_node: meta_root_node.map(|e| e.into()),
        meta_root_buddy_group: meta_root_buddy_group.map(|e| e.into()),
        fs_uuid,
    })
}
#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn get_nodes() {
        let app = TestApp::new().await;

        let res = super::get_nodes(
            &app,
            pm::GetNodesRequest {
                include_nics: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(res.nodes.len(), 14);
        assert!(res.nodes.iter().all(|e| e.nics.is_empty()));

        let res = super::get_nodes(&app, pm::GetNodesRequest { include_nics: true })
            .await
            .unwrap();

        assert_eq!(res.nodes.len(), 14);
        assert_eq!(
            res.nodes
                .iter()
                .find(|e| e.id.as_ref().unwrap().uid() == 101001)
                .unwrap()
                .nics
                .len(),
            4
        );
        assert_eq!(
            res.nodes
                .iter()
                .find(|e| e.id.as_ref().unwrap().uid() == 103004)
                .unwrap()
                .nics
                .len(),
            2
        );
        assert_eq!(res.meta_root_node.unwrap().uid.unwrap(), 101001);
    }

    /// Management must advertise the public key of the keypair it actually loaded. Peers need this
    /// to open a BeeMsg connection to management, and taking it from the keypair rather than the
    /// `keys` table means it can never disagree with the key file.
    #[tokio::test]
    async fn own_public_key_comes_from_the_loaded_keypair() {
        let app = TestApp::new().await;

        let expected = app
            .static_info()
            .static_keypair
            .as_ref()
            .expect("the test app configures a keypair")
            .public();

        let res = super::get_nodes(
            &app,
            pm::GetNodesRequest {
                include_nics: false,
            },
        )
        .await
        .unwrap();

        let mgmtd = res
            .nodes
            .iter()
            .find(|e| e.id.as_ref().unwrap().uid() == MGMTD_UID)
            .expect("the management node must be in the node list");

        assert_eq!(
            mgmtd.public_key,
            vec![expected.as_bytes().to_vec()],
            "management must advertise its loaded public key"
        );
    }

    /// A key registered for the management node in the database must not override the keypair - the
    /// key file is the single source of truth, so a stale row cannot lock peers out.
    #[tokio::test]
    async fn database_entry_does_not_override_own_public_key() {
        let app = TestApp::new().await;

        let expected = app.static_info().static_keypair.as_ref().unwrap().public();

        // Register a *different* key against the management node, the way an operator following
        // older setup instructions would.
        app.write_tx(|tx| {
            tx.execute("INSERT INTO identities (name) VALUES ('stale-mgmtd')", [])?;
            let identity_id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO identity_to_node (identity_id, node_type, node_id)
                SELECT ?1, node_type, node_id FROM nodes WHERE node_uid = ?2",
                rusqlite::params![identity_id, MGMTD_UID],
            )?;
            tx.execute(
                "INSERT INTO keys (key, identity_id) VALUES (?1, ?2)",
                rusqlite::params![[0xabu8; 32].as_slice(), identity_id],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        let res = super::get_nodes(
            &app,
            pm::GetNodesRequest {
                include_nics: false,
            },
        )
        .await
        .unwrap();

        let mgmtd = res
            .nodes
            .iter()
            .find(|e| e.id.as_ref().unwrap().uid() == MGMTD_UID)
            .unwrap();

        assert_eq!(
            mgmtd.public_key,
            vec![expected.as_bytes().to_vec()],
            "the stale database entry must be ignored"
        );
    }

    /// With authentication disabled there is no keypair and no key exchange, so management must
    /// advertise no key at all rather than falling back to whatever is in the database.
    #[tokio::test]
    async fn no_public_key_when_authentication_is_disabled() {
        let mut app = TestApp::new().await;
        Arc::get_mut(&mut app.info)
            .expect("no other handles to StaticInfo yet")
            .static_keypair = None;

        let res = super::get_nodes(
            &app,
            pm::GetNodesRequest {
                include_nics: false,
            },
        )
        .await
        .unwrap();

        let mgmtd = res
            .nodes
            .iter()
            .find(|e| e.id.as_ref().unwrap().uid() == MGMTD_UID)
            .unwrap();

        assert!(
            mgmtd.public_key.is_empty(),
            "no keypair means no advertised key, got {:?}",
            mgmtd.public_key
        );
    }
}
