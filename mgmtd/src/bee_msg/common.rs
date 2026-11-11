use super::*;
use crate::StaticInfo;
use crate::config::RegisterServerMode;
use crate::db::node_nic::ReplaceNic;
use db::misc::MetaRoot;
use protobuf::license::VerifyResult;
use rusqlite::{OptionalExtension, Transaction, params};
use shared::bee_msg::node::*;
use shared::bee_msg::target::*;
use shared::conn::IdentityId;
use shared::conn::beecrypt::KeyPairId;
use shared::types::{NodeId, TargetId};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

// Maximum number of clients that can register if license verification fails or license is invalid
const MAX_NUM_CLIENTS: u32 = 5;

fn map_identity(
    tx: &Transaction,
    identity_id: IdentityId,
    node_type: NodeType,
    node_id: NodeId,
) -> Result<()> {
    let node_identity: Option<IdentityId> = tx.query_row(
        sql!("SELECT identity_id FROM nodes WHERE node_type = ?1 AND node_id = ?2"),
        params![node_type.sql_variant(), node_id],
        |row| row.get(0),
    )?;

    match node_identity {
        Some(id) if id == identity_id => return Ok(()),
        Some(id) => bail!(
            "{node_type} node {node_id} is already mapped to identity {id}, not to {identity_id}. \
If the node's key pair changed, revoke the old identity with \
\"beegfs auth id revoke id:{id}\""
        ),
        None => {}
    }

    let identity_node: Option<(NodeType, NodeId)> = tx
        .query_row(
            sql!("SELECT node_type, node_id FROM nodes WHERE identity_id = ?1"),
            [identity_id],
            |row| Ok((NodeType::from_row(row, 0)?, row.get(1)?)),
        )
        .optional()?;

    if let Some((t, n)) = identity_node {
        bail!("Identity {identity_id} is already mapped to {t} node {n}");
    }

    tx.execute(
        sql!("UPDATE nodes SET identity_id = ?1 WHERE node_type = ?2 AND node_id = ?3"),
        params![identity_id, node_type.sql_variant(), node_id],
    )?;
    db::identity::bump_version(tx)?;

    log::info!("Mapped {node_type} node {node_id} to identity {identity_id}");

    Ok(())
}

fn delete_stale_client(tx: &Transaction, key_id: &KeyPairId) -> Result<()> {
    let stale: Option<NodeId> = tx
        .query_row(
            sql!(
                "SELECT n.node_id FROM keys AS k
                INNER JOIN nodes AS n USING (identity_id)
                WHERE k.key = ?1 AND n.node_type = ?2"
            ),
            params![key_id.to_string(), NodeType::Client.sql_variant()],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(num_id) = stale {
        let node = LegacyId {
            node_type: NodeType::Client,
            num_id,
        }
        .resolve(tx, EntityType::Node)?;

        db::node::delete(tx, node.uid)?;
        log::info!("Deleted stale client node {node} that used the same identity");
    }

    Ok(())
}

pub(super) struct NodeUpdate {
    pub node_id: NodeId,
    pub identity_id: Option<IdentityId>,
    pub identities_version: u64,
    pub identities_changed: bool,
}

pub(super) enum NodeRegistration {
    Updated(NodeUpdate),
    Pending,
    Rejected(String),
}

pub(super) struct PeerKey {
    pub key_id: KeyPairId,
    pub token: Option<String>,
    pub addr: String,
}

pub(super) fn new_servers_disabled(info: &StaticInfo) -> bool {
    match info.conn_crypto {
        Some(_) => info.user_config.register_server_mode == RegisterServerMode::Disabled,
        None => info.user_config.registration_disable,
    }
}

/// Processes incoming node information. Registers new nodes if config allows it
pub(super) async fn update_node(
    msg: RegisterNode,
    app: &impl App,
    reject: bool,
    peer_key: Option<PeerKey>,
) -> Result<NodeRegistration> {
    let nics = msg.nics.clone();
    let requested_node_id = msg.node_id;
    let registration_disable = new_servers_disabled(app.static_info());
    let modes = AdmissionModes::from_config(&app.static_info().user_config);
    let key_req = peer_key.map(|k| KeyRequest {
        key_id: k.key_id,
        category: KeyCategory::of_node_type(msg.node_type),
        token: k.token,
        node: Some((msg.node_type, msg.node_id)),
        name: None,
        addr: k.addr,
    });

    let licensed_clients: Option<u32> = if msg.node_type == NodeType::Client {
        match app.get_license_cert_data() {
            Ok(r) => match r.result() {
                // If license is valid, no limit to client count
                VerifyResult::VerifyValid => None,
                // no license file loaded, limit number of clients to NUM_CLIENTS
                VerifyResult::VerifyError => Some(MAX_NUM_CLIENTS),
                // license file was loaded and is outside validity period
                VerifyResult::VerifyInvalid => None,
                _ => {
                    log::debug!(
                        "Unexpected error during license verification, limiting number of clients to {MAX_NUM_CLIENTS}: {0}",
                        r.message
                    );
                    Some(MAX_NUM_CLIENTS)
                }
            },
            Err(e) => {
                log::debug!(
                    "Error during license verification, limiting number of clients to {MAX_NUM_CLIENTS}: {e:#}",
                );
                Some(MAX_NUM_CLIENTS)
            }
        }
    } else {
        // not a client registration, so not going to be used anyway. But let's be defensive
        Some(MAX_NUM_CLIENTS)
    };

    let licensed_machines = match app.get_licensed_machines() {
        Ok(n) => n,
        Err(err) => {
            log::debug!(
                "Could not obtain number of licensed machines, defaulting to unlimited: {err:#}"
            );
            u32::MAX
        }
    };

    let res = app
        .write_tx_no_sync(move |tx| {
            let version_before = db::identity::version(tx)?;

            let identity_id = match &key_req {
                Some(key_req) => match admit_key(tx, modes, key_req)? {
                    Admission::Known(identity_id) | Admission::Registered(identity_id) => {
                        Some(identity_id)
                    }
                    Admission::Pending => return Ok(Err(NodeRegistration::Pending)),
                    Admission::Rejected(reason) => {
                        return Ok(Err(NodeRegistration::Rejected(reason)));
                    }
                },
                None => None,
            };

            let node = if msg.node_id == 0 {
                // No node ID given => new node
                None
            } else {
                // If Some is returned, the node with node_id already exists
                try_resolve_num_id(tx, EntityType::Node, msg.node_type, msg.node_id)?
            };

            let machine_uuid = if matches!(msg.node_type, NodeType::Meta | NodeType::Storage)
                && !msg.machine_uuid.is_empty()
            {
                Some(std::str::from_utf8(&msg.machine_uuid)?)
            } else {
                None
            };

            if let Some(machine_uuid) = machine_uuid
                && db::node::count_machines(tx, machine_uuid, node.as_ref().map(|n| n.uid))?
                    >= licensed_machines
            {
                bail!("Licensed machine limit reached. Node registration denied.");
            }

            let new_alias_or_reg_token = String::from_utf8(msg.node_alias)?;

            if node.is_none()
                && msg.node_type == NodeType::Client
                && let Some(key_req) = &key_req
            {
                delete_stale_client(tx, &key_req.key_id)?;
            }

            let (node, is_new) = if let Some(node) = node {
                // Existing node, update data
                db::node::update(tx, node.uid, msg.port, machine_uuid)?;

                // If the updated node is a meta node, check if its corresponding target has a
                // registration token
                if msg.node_type == NodeType::Meta {
                    let stored_reg_token: Option<String> = tx.query_row(
                        sql!("SELECT reg_token FROM meta_targets WHERE node_id = ?1"),
                        [node.num_id()],
                        |row| row.get(0),
                    )?;

                    if let Some(ref t) = stored_reg_token
                        && t != &new_alias_or_reg_token
                    {
                        bail!(
                            "Meta node {node} has already been registered and its \
registration token ({new_alias_or_reg_token}) does not match the stored token ({t})",
                        );
                    } else if stored_reg_token.is_none() && !new_alias_or_reg_token.is_empty() {
                        tx.execute(
                            sql!(
                                "UPDATE targets SET reg_token = ?1
                                WHERE node_id = ?2 AND node_type = ?3"
                            ),
                            rusqlite::params![
                                new_alias_or_reg_token,
                                node.num_id(),
                                NodeType::Meta.sql_variant()
                            ],
                        )?;
                    }
                }

                (node, false)
            } else {
                // New node, do additional checks and insert data

                // Check node registration is allowed. This should ignore registering client
                // nodes.
                if msg.node_type != NodeType::Client && registration_disable {
                    bail!("Registration of new nodes is not allowed");
                }

                let num_reg_clients: u32 = tx.query_row(
                    sql!("SELECT COUNT(DISTINCT node_uid) FROM nodes WHERE node_type = ?1"),
                    params![NodeType::Client.sql_variant()],
                    |row| row.get(0),
                )?;

                if msg.node_type == NodeType::Client
                    && let Some(cs) = licensed_clients
                    && num_reg_clients >= cs {
                        if reject {
                            bail!("Number of licensed clients ({MAX_NUM_CLIENTS}) exhausted. Client registration denied.");
                        } else {
                            log::warn!("Number of licensed clients ({MAX_NUM_CLIENTS}) exhausted but client doesn't support rejection.");
                        }
                    }

                let new_alias = if msg.node_type == NodeType::Client {
                    // In versions prior to 8.0 the string node ID generated by the client
                    // started with a number which is not allowed by the new alias schema.
                    // As part of BeeGFS 8 the nodeID generated for each client mount was
                    // updated to no longer start with a number, thus it is unlikely this
                    // would happen unless BeeGFS 8 was mounted by a BeeGFS 7 client.

                    let new_alias = Alias::try_from(new_alias_or_reg_token.clone()).ok();

                    if new_alias.is_none() {
                        log::warn!(
                            "Unable to use alias requested by client (possibly the\
client version < 8.0)"
                        );
                    }
                    new_alias
                } else {
                    None
                };

                // Insert new node entry
                let node = db::node::insert(tx, msg.node_id, new_alias, msg.node_type, msg.port)?;

                // if this is a meta node, auto-add a corresponding meta target after the node.
                if msg.node_type == NodeType::Meta {
                    // Either create a new TargetID/NodeID or use the given NodeID. In the latter
                    // case, convert it to a TargetID. Due to the difference in bitsize, meta node
                    // IDs are not allowed to be bigger than u16.
                    let target_id = match node.num_id() {
                        0 => db::misc::find_new_id(
                            tx,
                            "targets",
                            "target_id",
                            NodeType::Meta,
                            1..=0xFFFF,
                        )?,
                        n => TargetId::try_from(n).map_err(|_| anyhow!("{n} is not a valid numeric meta node id (must be between 1 and 65535)"))?,
                    };

                    // Do not set a registration token if the provided string is empty. This makes
                    // sure we don't set it to an empty string as provided by older meta nodes to
                    // prevent a mismatch after they are updated.
                    let tk = if !new_alias_or_reg_token.is_empty() {
                        Some(new_alias_or_reg_token.as_str())
                    } else {
                        None
                    };

                    db::target::insert(
                        tx,
                        target_id,
                        tk,
                        NodeTypeServer::Meta,
                        Some(target_id.into()),
                    )?;

                    // If this is the first meta target, set it as meta root
                    tx.execute(
                        sql!("INSERT OR IGNORE INTO root_inode (target_id) VALUES (?1)"),
                        [target_id],
                    )?;
                }

                (node, true)
            };

            if let Some(identity_id) = identity_id {
                map_identity(tx, identity_id, node.node_type(), node.num_id())?;
            }

            // Update the corresponding nic lists
            db::node_nic::replace(
                tx,
                node.uid,
                msg.nics.iter().map(|e| ReplaceNic {
                    nic_type: e.nic_type,
                    addr: &e.addr,
                    name: String::from_utf8_lossy(&e.name),
                }),
            )?;

            let meta_root = match node.node_type() {
                // In case this is a meta node, the requester expects info about the meta
                // root
                NodeType::Meta => db::misc::get_meta_root(tx)?,
                _ => MetaRoot::Unknown,
            };

            let version = db::identity::version(tx)?;

            Ok(Ok((
                node,
                meta_root,
                is_new,
                identity_id,
                version_before,
                version,
            )))
        })
        .await?;

    let (node, meta_root, is_new, identity_id, version_before, version) = match res {
        Ok(registered) => registered,
        Err(not_registered) => return Ok(not_registered),
    };

    app.replace_node_addrs(
        node.uid,
        nics.clone()
            .into_iter()
            .map(|e| SocketAddr::new(e.addr, msg.port))
            .collect::<Arc<_>>(),
    );

    if is_new {
        log::info!("Registered new node {node} (Requested Numeric Id: {requested_node_id})",);
    } else {
        log::debug!("Updated node {node} node",);
    }

    let node_num_id = node.num_id();

    // Don't wait for notifications to go out because server nodes only wait for a short time for
    // the reponse. On some systems broadcasting the notifications seems to take longer and
    // registration fails.
    let app = app.clone();
    tokio::spawn(async move {
        app.send_notifications(
            match node.node_type() {
                NodeType::Meta => &[NodeType::Meta, NodeType::Client],
                NodeType::Storage => &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                NodeType::Client => &[NodeType::Meta],
                _ => &[],
            },
            &Heartbeat {
                instance_version: 0,
                nic_list_version: 0,
                node_type: node.node_type(),
                node_alias: String::from(node.alias).into_bytes(),
                ack_id: "".into(),
                node_num_id,
                root_num_id: match meta_root {
                    MetaRoot::Unknown => 0,
                    MetaRoot::Normal(node_id, _) => node_id,
                    MetaRoot::Mirrored(group_id) => group_id.into(),
                },
                is_root_mirrored: match meta_root {
                    MetaRoot::Unknown | MetaRoot::Normal(_, _) => 0,
                    MetaRoot::Mirrored(_) => 1,
                },
                port: msg.port,
                port_tcp_unused: msg.port,
                nic_list: nics,
                machine_uuid: vec![], // No need for the other nodes to know machine UUIDs
            },
        )
        .await;
    });

    Ok(NodeRegistration::Updated(NodeUpdate {
        node_id: node_num_id,
        identity_id,
        identities_version: version,
        identities_changed: version != version_before,
    }))
}

pub(super) fn get_targets_with_states(
    tx: &Transaction,
    pre_shutdown: bool,
    node_type: NodeTypeServer,
    node_offline_timeout: Duration,
) -> Result<Vec<(TargetId, TargetConsistencyState, TargetReachabilityState)>> {
    let targets = tx.query_map_collect(
        sql!(
            "SELECT t.target_id, t.consistency,
                (UNIXEPOCH('now') - UNIXEPOCH(t.last_update)), gp.p_target_id, gs.s_target_id
            FROM targets AS t
            INNER JOIN nodes AS n USING(node_type, node_id)
            LEFT JOIN buddy_groups AS gp ON gp.p_target_id = t.target_id AND gp.node_type = t.node_type
            LEFT JOIN buddy_groups AS gs ON gs.s_target_id = t.target_id AND gs.node_type = t.node_type
            WHERE t.node_type = ?1"
        ),
        [node_type.sql_variant()],
        |row| {
            let is_primary = row.get::<_, Option<TargetId>>(3)?.is_some();
            let is_secondary = row.get::<_, Option<TargetId>>(4)?.is_some();

            Ok((
                row.get(0)?,
                TargetConsistencyState::from_row(row, 1)?,
                if !pre_shutdown || is_secondary {
                    let age = Duration::from_secs(row.get(2)?);

                    // We never want to report a primary node of a buddy group as offline since this
                    // is considered invalid. Instead we just report ProbablyOffline and wait for the switchover.
                    if !is_primary && age > node_offline_timeout {
                        TargetReachabilityState::Offline
                    } else if age > node_offline_timeout / 2 {
                        TargetReachabilityState::ProbablyOffline
                    } else {
                        TargetReachabilityState::Online
                    }
                } else {
                    TargetReachabilityState::ProbablyOffline
                },
            ))
        },
    )?;

    Ok(targets)
}

/// Updates the `last_contact` time for all the nodes belonging to the passed targets and the
/// targets `last_update` times themselves.
///
/// # Return Value
/// Returns the number of targets (not nodes) that were not considered "online" before. Meaning they
/// were at least "probably offline".
pub(super) fn update_last_contact_times(
    tx: &Transaction,
    target_ids: &[TargetId],
    node_type: NodeTypeServer,
    offline_timeout: Duration,
) -> Result<usize> {
    let target_ids_param = sqlite::rarray_param(target_ids.iter().copied());

    tx.execute_cached(
        sql!(
            "UPDATE nodes AS n SET last_contact = DATETIME('now')
            WHERE n.node_uid IN (
            SELECT DISTINCT node_uid FROM targets_ext
            WHERE target_id IN rarray(?1) AND node_type = ?2)"
        ),
        rusqlite::params![&target_ids_param, node_type.sql_variant()],
    )?;

    let reachabilities_changed: usize = tx.query_row_cached(
        sql!(
            "SELECT COUNT(*) FROM targets
            WHERE target_id IN rarray(?1) AND node_type = ?2
                AND UNIXEPOCH(last_update) < UNIXEPOCH('now') - ?3"
        ),
        rusqlite::params![
            &target_ids_param,
            node_type.sql_variant(),
            offline_timeout.as_secs() / 2
        ],
        |row| row.get(0),
    )?;

    tx.execute_cached(
        sql!(
            "UPDATE targets SET last_update = DATETIME('now')
            WHERE target_id IN rarray(?1) AND node_type = ?2"
        ),
        rusqlite::params![&target_ids_param, node_type.sql_variant()],
    )?;

    Ok(reachabilities_changed)
}
