use super::*;
use crate::cap_pool::{CapPoolCalculator, CapacityInfo};
use std::time::Duration;

impl CapacityInfo for &pm::get_targets_response::Target {
    fn free_space(&self) -> u64 {
        self.free_space_bytes.unwrap()
    }

    fn free_inodes(&self) -> u64 {
        self.free_inodes.unwrap()
    }
}

/// Delivers the list of targets
pub(crate) async fn get_targets(
    app: &impl App,
    _req: pm::GetTargetsRequest,
) -> Result<pm::GetTargetsResponse> {
    let node_offline_timeout = app.static_info().user_config.node_offline_timeout;
    let pre_shutdown = app.is_pre_shutdown();

    let fetch_op = move |tx: &Transaction| {
        let targets_q = sql!(
            "SELECT t.target_uid, t.alias, t.target_id, t.node_type,
                n.node_uid, n.alias, n.node_id,
                p.pool_uid, p.alias, p.pool_id,
                t.consistency, (UNIXEPOCH('now') - UNIXEPOCH(t.last_update)),
                t.free_space, t.free_inodes, t.total_space, t.total_inodes,
                gp.p_target_id, gs.s_target_id
            FROM targets_ext AS t
            INNER JOIN nodes_ext AS n USING(node_uid)
            LEFT JOIN pools_ext AS p USING(node_type, pool_id)
            LEFT JOIN buddy_groups AS gp ON gp.p_target_id = t.target_id
                AND gp.node_type = t.node_type
            LEFT JOIN buddy_groups AS gs ON gs.s_target_id = t.target_id
                AND gs.node_type = t.node_type"
        );

        let targets_f = move |row: &rusqlite::Row| {
            let node_type = NodeType::from_row(row, 3)?.into_proto_i32();
            let age = Duration::from_secs(row.get(11)?);
            let is_primary = row.get::<_, Option<TargetId>>(16)?.is_some();
            let is_secondary = row.get::<_, Option<TargetId>>(17)?.is_some();

            Ok(pm::get_targets_response::Target {
                id: Some(pb::EntityIdSet {
                    uid: row.get(0)?,
                    legacy_id: Some(pb::LegacyId {
                        num_id: row.get(2)?,
                        node_type,
                    }),
                    alias: row.get(1)?,
                }),
                node_type,
                node: Some(pb::EntityIdSet {
                    uid: row.get(4)?,
                    legacy_id: Some(pb::LegacyId {
                        num_id: row.get(6)?,
                        node_type,
                    }),
                    alias: row.get(5)?,
                }),
                storage_pool: if let Some(uid) = row.get::<_, Option<Uid>>(7)? {
                    Some(pb::EntityIdSet {
                        uid: Some(uid),
                        legacy_id: Some(pb::LegacyId {
                            num_id: row.get(9)?,
                            node_type,
                        }),
                        alias: row.get(8)?,
                    })
                } else {
                    None
                },

                reachability_state: if !pre_shutdown || is_secondary {
                    if !is_primary && age > node_offline_timeout {
                        pb::ReachabilityState::Offline
                    } else if age > node_offline_timeout / 2 {
                        pb::ReachabilityState::Poffline
                    } else {
                        pb::ReachabilityState::Online
                    }
                    .into()
                } else {
                    pb::ReachabilityState::Poffline.into()
                },
                consistency_state: TargetConsistencyState::from_row(row, 10)?.into_proto_i32(),
                last_contact_s: row.get(11)?,
                free_space_bytes: row.get(12)?,
                free_inodes: row.get(13)?,
                cap_pool: pb::CapacityPool::Unspecified.into(),
                total_space_bytes: row.get(14)?,
                total_inodes: row.get(15)?,
            })
        };

        let pools_q = sql!("SELECT pool_uid FROM storage_pools");

        Ok((
            tx.query_map_collect(targets_q, [], targets_f)?,
            tx.query_map_collect(pools_q, [], |row| row.get(0))?,
        ))
    };

    let (mut targets, pools): (Vec<pm::get_targets_response::Target>, Vec<Uid>) =
        app.read_tx(fetch_op).await.status_code(Code::Internal)?;

    let cap_pool_meta_calc = CapPoolCalculator::new(
        app.static_info().user_config.cap_pool_meta_limits.clone(),
        app.static_info()
            .user_config
            .cap_pool_dynamic_meta_limits
            .as_ref(),
        targets
            .iter()
            .filter(|t| t.node_type() == pb::NodeType::Meta),
    )?;

    for t in targets
        .iter_mut()
        .filter(|t| t.node_type() == pb::NodeType::Meta)
    {
        if t.free_space_bytes.is_some() && t.free_inodes.is_some() {
            t.cap_pool = pb::CapacityPool::from(
                cap_pool_meta_calc.cap_pool(t.free_space_bytes.unwrap(), t.free_inodes.unwrap()),
            )
            .into();
        }
    }

    for sp_uid in pools {
        let cap_pool_storage_calc = CapPoolCalculator::new(
            app.static_info()
                .user_config
                .cap_pool_storage_limits
                .clone(),
            app.static_info()
                .user_config
                .cap_pool_dynamic_storage_limits
                .as_ref(),
            targets.iter().filter(|t| {
                t.storage_pool
                    .as_ref()
                    .is_some_and(|e| e.uid == Some(sp_uid))
            }),
        )?;

        for t in targets.iter_mut().filter(|t| {
            t.storage_pool
                .as_ref()
                .is_some_and(|e| e.uid == Some(sp_uid))
        }) {
            if t.free_space_bytes.is_some() && t.free_inodes.is_some() {
                t.cap_pool = pb::CapacityPool::from(
                    cap_pool_storage_calc
                        .cap_pool(t.free_space_bytes.unwrap(), t.free_inodes.unwrap()),
                )
                .into();
            }
        }
    }

    Ok(pm::GetTargetsResponse { targets })
}
