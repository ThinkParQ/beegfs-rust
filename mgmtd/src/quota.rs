//! Functionality for fetching and updating quota information from / to nodes and the database.

mod system_id;

use crate::app::*;
use crate::types::SqliteEnumExt;
use anyhow::{Context as AnyhowContext, Result};
use rusqlite::params;
use shared::bee_msg::OpsErr;
use shared::bee_msg::quota::{
    GetQuotaInfo, GetQuotaInfoResp, SetExceededQuota, SetExceededQuotaResp,
};
use shared::types::{NodeType, PoolId, QuotaId, QuotaIdType, QuotaType, TargetId, Uid};
use sqlite::TransactionExt;
use sqlite_check::sql;
use std::collections::HashSet;
use std::path::Path;

/// Fetches quota information for all storage targets, calculates exceeded IDs and distributes them.
pub(crate) async fn update_and_distribute(app: &impl App) -> Result<()> {
    // Fetch quota data from storage daemons

    let targets: Vec<(TargetId, PoolId, Uid)> = app
        .read_tx(move |tx| {
            tx.query_map_collect(
                sql!(
                    "SELECT target_id, pool_id, node_uid
                    FROM storage_targets
                    INNER JOIN nodes USING(node_type, node_id)
                    WHERE node_id IS NOT NULL"
                ),
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(Into::into)
        })
        .await?;

    if targets.is_empty() {
        return Ok(());
    }

    log::info!(
        "Fetching quota information for {} storage targets",
        targets.len()
    );

    // The to-be-queried IDs
    let (mut user_ids, mut group_ids) = (HashSet::new(), HashSet::new());

    // If configured, add system User IDS
    let user_ids_min = app.static_info().user_config.quota_user_system_ids_min;

    if let Some(user_ids_min) = user_ids_min {
        system_id::user_ids()
            .await
            .filter(|e| e >= &user_ids_min)
            .for_each(|e| {
                user_ids.insert(e);
            });
    }

    // If configured, add system Group IDS
    let group_ids_min = app.static_info().user_config.quota_group_system_ids_min;

    if let Some(group_ids_min) = group_ids_min {
        system_id::group_ids()
            .await
            .filter(|e| e >= &group_ids_min)
            .for_each(|e| {
                group_ids.insert(e);
            });
    }

    // If configured, add user IDs from file
    if let Some(ref path) = app.static_info().user_config.quota_user_ids_file {
        try_read_quota_ids(path, &mut user_ids)?;
    }

    // If configured, add group IDs from file
    if let Some(ref path) = app.static_info().user_config.quota_group_ids_file {
        try_read_quota_ids(path, &mut group_ids)?;
    }

    // If configured, add range based user IDs
    if let Some(range) = &app.static_info().user_config.quota_user_ids_range {
        user_ids.extend(range.clone());
    }

    // If configured, add range based group IDs
    if let Some(range) = &app.static_info().user_config.quota_group_ids_range {
        group_ids.extend(range.clone());
    }

    let mut tasks = vec![];
    // Sends one request per target to the respective owner node
    // Requesting is done concurrently.
    for (target_id, pool_id, node_uid) in targets {
        let app2 = app.clone();
        let user_ids2 = user_ids.clone();
        let group_ids2 = group_ids.clone();

        tasks.push(tokio::spawn(async move {
            let resp_users: Result<GetQuotaInfoResp> = app2
                .request(
                    node_uid,
                    &GetQuotaInfo::with_user_ids(user_ids2, target_id, pool_id),
                )
                .await;

            let resp_groups: Result<GetQuotaInfoResp> = app2
                .request(
                    node_uid,
                    &GetQuotaInfo::with_group_ids(group_ids2, target_id, pool_id),
                )
                .await;

            if resp_users.is_err() || resp_groups.is_err() {
                log::error!(
                    "Fetching quota info for storage target {target_id} from node with uid
{node_uid} failed. Users: {resp_users:?}, Groups: {resp_groups:?}"
                );

                return (target_id, None);
            }

            let mut entries = resp_users.expect("impossible").quota_entry;
            entries.append(&mut resp_groups.expect("impossible").quota_entry);

            (target_id, Some(entries))
        }));
    }

    // Await all the responses
    for t in tasks {
        let (target_id, entries) = t.await?;

        // Only process that target if there were not errors when fetching for this target
        if let Some(entries) = entries {
            app.write_tx(move |tx| {
                // Always delete all the old entries for that target to make sure entries for no
                // longer queried ids are removed. We always get the complete list from the
                // storages and we only update if there was no fetch error.
                tx.execute_cached(
                    sql!("DELETE FROM quota_usage WHERE target_id = ?1"),
                    [target_id],
                )?;

                let mut insert_stmt = tx.prepare_cached(sql!(
                    "INSERT INTO quota_usage (quota_id, id_type, quota_type, target_id, value)
                    VALUES (?1, ?2, ?3 ,?4 ,?5)"
                ))?;

                log::debug!(
                    "Setting {} quota usage entries for target {target_id}",
                    entries.len()
                );

                for e in entries {
                    if e.space > 0 {
                        insert_stmt.execute(params![
                            e.id,
                            e.id_type.sql_variant(),
                            QuotaType::Space.sql_variant(),
                            target_id,
                            e.space
                        ])?;
                    }

                    if e.inodes > 0 {
                        insert_stmt.execute(params![
                            e.id,
                            e.id_type.sql_variant(),
                            QuotaType::Inode.sql_variant(),
                            target_id,
                            e.inodes
                        ])?;
                    }
                }

                Ok(())
            })
            .await?;
        }
    }

    if app.static_info().user_config.quota_enforce {
        exceeded_quota(app).await?;
    }

    Ok(())
}

/// Calculate and push exceeded quota info to the nodes
async fn exceeded_quota(app: &impl App) -> Result<()> {
    log::info!("Calculating and pushing exceeded quota");

    let (msges, nodes) = app
        .read_tx(|tx| {
            let pools: Vec<_> =
                tx.query_map_collect(sql!("SELECT pool_id FROM pools"), [], |row| row.get(0))?;

            // Prepare empty messages. It is important to always send a message for each (PoolId,
            // QuotaIdType, QuotaType) to each node, even if there are no exceeded ids, to remove
            // previously existing exceeded ids on the servers.
            let mut msges: Vec<SetExceededQuota> = vec![];
            for pool_id in pools {
                for id_type in [QuotaIdType::User, QuotaIdType::Group] {
                    for quota_type in [QuotaType::Space, QuotaType::Inode] {
                        msges.push(SetExceededQuota {
                            pool_id,
                            id_type,
                            quota_type,
                            exceeded_quota_ids: vec![],
                        });
                    }
                }
            }

            // Fill the prepared messages with matching exceeded quota ids
            let mut stmt = tx.prepare_cached(sql!(
                "SELECT DISTINCT e.quota_id, e.id_type, e.quota_type, st.pool_id
                FROM quota_usage AS e
                INNER JOIN targets AS st USING(node_type, target_id)
                LEFT JOIN quota_default_limits AS d USING(id_type, quota_type, pool_id)
                LEFT JOIN quota_limits AS l USING(quota_id, id_type, quota_type, pool_id)
                GROUP BY e.quota_id, e.id_type, e.quota_type, st.pool_id
                HAVING SUM(e.value) > COALESCE(l.value, d.value)"
            ))?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                for m in &mut msges {
                    if row.get::<_, PoolId>(3)? == m.pool_id
                        && QuotaIdType::from_row(row, 1)? == m.id_type
                        && QuotaType::from_row(row, 2)? == m.quota_type
                    {
                        m.exceeded_quota_ids.push(row.get(0)?);
                        break;
                    }
                }
            }

            // Get all node uids to send the messages to
            let nodes: Vec<Uid> = tx.query_map_collect(
                sql!("SELECT node_uid FROM nodes WHERE node_type IN (?1,?2)"),
                [
                    NodeType::Meta.sql_variant(),
                    NodeType::Storage.sql_variant(),
                ],
                |row| row.get(0),
            )?;

            Ok((msges, nodes))
        })
        .await?;

    // Send all messages with exceeded quota information to all meta and storage nodes
    // Since there is one message for each combination of (pool x (user, group) x (space, inode)),
    // this might be very demanding, but can't do anything about that without changing meta and
    // storage too.
    // If this shows as a bottleneck, the requests could be done concurrently though.
    for msg in msges {
        let mut request_fails = 0;
        let mut non_success_count = 0;

        for node_uid in &nodes {
            match app
                .request::<_, SetExceededQuotaResp>(*node_uid, &msg)
                .await
            {
                Ok(resp) => {
                    if resp.result != OpsErr::SUCCESS {
                        non_success_count += 1;
                    }
                }
                Err(_) => {
                    request_fails += 1;
                }
            }
        }

        if request_fails > 0 || non_success_count > 0 {
            log::error!(
                "Pushing exceeded quota IDs to some nodes failed. Request failures: \
                 {request_fails}, received non-success responses: {non_success_count}"
            );
        }
    }

    Ok(())
}

/// Tries to read quota IDs (users, groups) from a file
///
/// IDs must be in numerical form and separated by any whitespace.
fn try_read_quota_ids(path: &Path, read_into: &mut HashSet<QuotaId>) -> Result<()> {
    let data = std::fs::read_to_string(path)?;
    for id in data.split_whitespace().map(|e| e.parse()) {
        read_into.insert(id.context("Invalid syntax in quota file {path}")?);
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::Config;
    use crate::app::test::*;
    use crate::types::SqliteEnumExt;
    use shared::bee_msg::OpsErr;
    use shared::bee_msg::quota::{
        GetQuotaInfo, GetQuotaInfoResp, QuotaEntry, QuotaInodeSupport, SetExceededQuota,
        SetExceededQuotaResp,
    };
    use shared::types::{QuotaIdType, QuotaType};

    #[tokio::test]
    async fn update() {
        let app = TestApp::with_config(Config {
            quota_enable: true,
            quota_enforce: false, // Exceeded calculation and push is tested separately
            quota_user_ids_range: Some(0..=9),
            quota_group_ids_range: Some(0..=9),
            ..Default::default()
        })
        .await;

        app.set_request_handler(|req| {
            let r = req.downcast_ref::<GetQuotaInfo>().unwrap();

            let mut quota_entry = vec![];

            // Provide dummy quota values for target 1 depending on the id and type
            if r.target_id == 1 {
                for id in r.id_list.iter().copied() {
                    quota_entry.push(QuotaEntry {
                        space: id as u64 * 1000 + r.id_type.sql_variant() as u64,
                        inodes: id as u64 * 100 + r.id_type.sql_variant() as u64,
                        id,
                        id_type: r.id_type,
                        valid: 1,
                    });
                }
            } else if r.target_id == 2 && r.id_type == QuotaIdType::User {
                quota_entry.push(QuotaEntry {
                    space: 999,
                    inodes: 999,
                    id: 5,
                    id_type: QuotaIdType::User,
                    valid: 1,
                });
            }

            Ok(Box::new(GetQuotaInfoResp {
                quota_inode_support: QuotaInodeSupport::AllBlockDevices,
                quota_entry,
            }))
        });

        super::update_and_distribute(&app).await.unwrap();

        // Find the amount of target 1 entries which values match the schema they have been reported
        // with
        let t1_sql = format!(
            "SELECT COUNT(*) FROM quota_usage WHERE target_id = 1 AND (
                (quota_type = {s} AND id_type = {u} AND value = quota_id * 1000 + {u})
                OR (quota_type = {s} AND id_type = {g} AND value = quota_id * 1000 + {g})
                OR (quota_type = {i} AND id_type = {u} AND value = quota_id * 100 + {u})
                OR (quota_type = {i} AND id_type = {g} AND value = quota_id * 100 + {g})
            )",
            s = QuotaType::Space.sql_variant(),
            i = QuotaType::Inode.sql_variant(),
            u = QuotaIdType::User.sql_variant(),
            g = QuotaIdType::Group.sql_variant()
        );

        // Assert that the entries in the db are exactly the ones provided above
        let t1_sql2 = t1_sql.clone();
        app.db
            .read_tx(move |tx| {
                let usage_entries: i32 =
                    tx.query_row("SELECT COUNT(*) FROM quota_usage", [], |row| row.get(0))?;
                assert_eq!(usage_entries, 42);

                let usage_entries: i32 = tx.query_row(&t1_sql2, [], |row| row.get(0))?;
                assert_eq!(usage_entries, 40);

                let usage_entries: i32 = tx.query_row(
                    "SELECT COUNT(*) FROM quota_usage
                        WHERE target_id = 2 AND value == 999 AND quota_id = 5",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(usage_entries, 2);

                Ok(())
            })
            .await
            .unwrap();

        // Now test updating and removing entries, and test that fetch errors don't lead to updates
        app.set_request_handler(|req| {
            let r = req.downcast_ref::<GetQuotaInfo>().unwrap();

            // Fail request for target 1 user quota (only)
            if r.target_id == 1 && r.id_type == QuotaIdType::User {
                return Err(anyhow::anyhow!("target 1 fail"));
            }

            Ok(Box::new(GetQuotaInfoResp {
                quota_inode_support: QuotaInodeSupport::AllBlockDevices,
                quota_entry: vec![],
            }))
        });

        super::update_and_distribute(&app).await.unwrap();

        // Now target 2 quota should be empty, target 1 quota should be completely untouched due to
        // the error (even if it only failed for user quota request)
        app.db
            .read_tx(move |tx| {
                let usage_entries: i32 =
                    tx.query_row("SELECT COUNT(*) FROM quota_usage", [], |row| row.get(0))?;
                assert_eq!(usage_entries, 40);

                let usage_entries: i32 = tx.query_row(&t1_sql, [], |row| row.get(0))?;
                assert_eq!(usage_entries, 40);

                Ok(())
            })
            .await
            .unwrap();

        // Now test setting some new values to target 1
        app.set_request_handler(|req| {
            let r = req.downcast_ref::<GetQuotaInfo>().unwrap();

            let mut quota_entry = vec![];

            if r.target_id == 1 {
                quota_entry.push(QuotaEntry {
                    space: 999,
                    inodes: 999,
                    id: 1,
                    id_type: r.id_type,
                    valid: 1,
                });
            }

            Ok(Box::new(GetQuotaInfoResp {
                quota_inode_support: QuotaInodeSupport::AllBlockDevices,
                quota_entry,
            }))
        });

        super::update_and_distribute(&app).await.unwrap();

        // Target 1 should now only have the couple of entries resulting from above
        app.db
            .read_tx(move |tx| {
                let usage_entries: i32 =
                    tx.query_row("SELECT COUNT(*) FROM quota_usage", [], |row| row.get(0))?;
                assert_eq!(usage_entries, 4);

                let usage_entries: i32 = tx.query_row(
                    "SELECT COUNT(*) FROM quota_usage WHERE target_id = 1 AND value == 999",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(usage_entries, 4);

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn exceeded_quota() {
        // This fn doesn't need special config
        let app = TestApp::new().await;

        app.set_request_handler(move |req| {
            let r = req.downcast_ref::<SetExceededQuota>().unwrap();

            match (r.pool_id, r.id_type, r.quota_type) {
                (1, QuotaIdType::User, QuotaType::Space) => {
                    assert_eq!(r.exceeded_quota_ids.as_slice(), &[2, 4, 10])
                }
                (1, QuotaIdType::Group, QuotaType::Space) => {
                    assert_eq!(r.exceeded_quota_ids.as_slice(), &[2, 4, 11])
                }
                (1, QuotaIdType::User, QuotaType::Inode) => {
                    assert_eq!(r.exceeded_quota_ids.as_slice(), &[2, 4, 12])
                }
                (1, QuotaIdType::Group, QuotaType::Inode) => {
                    assert_eq!(r.exceeded_quota_ids.as_slice(), &[2, 4, 13])
                }
                (2, QuotaIdType::User, QuotaType::Space) => {
                    assert_eq!(r.exceeded_quota_ids.as_slice(), &[20])
                }
                _ => {
                    assert_eq!(r.exceeded_quota_ids.as_slice(), &[]);
                }
            }

            Ok(Box::new(SetExceededQuotaResp {
                result: OpsErr::SUCCESS,
            }))
        });

        super::exceeded_quota(&app).await.unwrap();
    }
}
