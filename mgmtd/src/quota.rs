//! Functionality for fetching and updating quota information from / to nodes and the database.

mod system_id;

use crate::app::*;
use crate::license::LicensedFeature;
use crate::types::SqliteEnumExt;
use anyhow::{Context as AnyhowContext, Result, bail};
use rusqlite::params;
use shared::bee_msg::OpsErr;
use shared::bee_msg::quota::{
    GetQuotaInfo, GetQuotaInfoResp, QuotaEntry, SetExceededQuota, SetExceededQuotaResp,
};
use shared::types::{NodeType, PoolId, QuotaId, QuotaIdType, QuotaType, TargetId, Uid};
use sqlite::TransactionExt;
use sqlite_check::sql;
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio::time::Instant;

#[derive(Debug, Clone, Copy)]
struct TargetToQuery {
    target_id: TargetId,
    pool_id: PoolId,
    node_uid: Uid,
}

/// Fetches quota information for all storage targets and updates the quota usage database
pub(crate) async fn fetch_and_update(app: &impl App) -> Result<()> {
    if app.verify_licensed_feature(LicensedFeature::Quota).is_err() {
        log::warn!("Quota is enabled but feature not licensed. Skipping quota collection");
        return Ok(());
    }

    // Fetch quota data from storage daemons
    let targets_to_query: Vec<TargetToQuery> = app
        .read_tx(move |tx| {
            tx.query_map_collect(
                sql!(
                    "SELECT target_id, pool_id, node_uid
                    FROM storage_targets
                    INNER JOIN nodes USING(node_type, node_id)"
                ),
                [],
                |row| {
                    Ok(TargetToQuery {
                        target_id: row.get(0)?,
                        pool_id: row.get(1)?,
                        node_uid: row.get(2)?,
                    })
                },
            )
            .map_err(Into::into)
        })
        .await?;

    if targets_to_query.is_empty() {
        return Ok(());
    }

    let targets_to_query_count = targets_to_query.len();
    let start_time = Instant::now();

    let tasks = create_and_send_requests(app, targets_to_query).await?;

    // Await all the responses
    let mut entry_counter = 0;
    for (target, jh) in tasks {
        let res = async {
            let entries = jh.await?;

            // Only process that target if there were not errors when fetching for this target
            if let Some(entries) = entries {
                entry_counter += entries.len();

                app.write_tx(move |tx| {
                    // Always delete all the old entries for that target to make sure entries for no
                    // longer queried ids are removed. We always get the complete list from the
                    // storages and we only update if there was no fetch error.
                    // There is one task per target with merged results from multiple queries, so no
                    // accidental override here.
                    tx.execute_cached(
                        sql!("DELETE FROM quota_usage WHERE target_id = ?1"),
                        [target.target_id],
                    )?;

                    // The entry list can contain duplicated entries if both range and list mode
                    // are configured as they use two separate requests, thus the OR IGNORE.
                    let mut insert_stmt = tx.prepare_cached(sql!(
                        "INSERT OR IGNORE
                        INTO quota_usage (quota_id, id_type, quota_type, target_id, value)
                        VALUES (?1, ?2, ?3 ,?4 ,?5)"
                    ))?;

                    for e in entries {
                        if e.space > 0 {
                            insert_stmt.execute(params![
                                e.id,
                                e.id_type.sql_variant(),
                                QuotaType::Space.sql_variant(),
                                target.target_id,
                                e.space
                            ])?;
                        }

                        if e.inodes > 0 {
                            insert_stmt.execute(params![
                                e.id,
                                e.id_type.sql_variant(),
                                QuotaType::Inode.sql_variant(),
                                target.target_id,
                                e.inodes
                            ])?;
                        }
                    }

                    Ok(())
                })
                .await?;
            }

            Ok(()) as Result<_>
        }
        .await;

        if let Err(err) = res {
            log::error!(
                "Receiving and storing quota info from storage target {} failed: {err:#}",
                target.target_id
            );
        }
    }

    log::info!(
        "Fetched and stored {entry_counter} quota entries from {} targets in {:?}",
        targets_to_query_count,
        start_time.elapsed()
    );

    Ok(())
}

/// Request all quota entries for the given target list for the configured ids (system, list,
/// range, all). Returns the async request tasks.
async fn create_and_send_requests(
    app: &impl App,
    targets: Vec<TargetToQuery>,
) -> Result<Vec<(TargetToQuery, JoinHandle<Option<Vec<QuotaEntry>>>)>> {
    let config = &app.static_info().user_config;

    // The to-be-queried IDs
    let (mut user_list, mut group_list) = (HashSet::new(), HashSet::new());

    // If configured, add system User IDS
    let user_ids_min = config.quota_user_system_ids_min;

    if let Some(user_ids_min) = user_ids_min {
        system_id::user_ids()
            .await
            .filter(|e| e >= &user_ids_min)
            .for_each(|e| {
                user_list.insert(e);
            });
    }

    // If configured, add system Group IDS
    let group_ids_min = config.quota_group_system_ids_min;

    if let Some(group_ids_min) = group_ids_min {
        system_id::group_ids()
            .await
            .filter(|e| e >= &group_ids_min)
            .for_each(|e| {
                group_list.insert(e);
            });
    }

    // If configured, add user IDs from file
    if let Some(ref path) = config.quota_user_ids_file {
        try_read_quota_ids(path, &mut user_list)?;
    }

    // If configured, add group IDs from file
    if let Some(ref path) = config.quota_group_ids_file {
        try_read_quota_ids(path, &mut group_list)?;
    }

    // Use the automatic "all" mode when no id selection at all is configured for that id type.
    let user_use_all = config.quota_user_system_ids_min.is_none()
        && config.quota_user_ids_file.is_none()
        && config.quota_user_ids_range.is_none();
    let group_use_all = config.quota_group_system_ids_min.is_none()
        && config.quota_group_ids_file.is_none()
        && config.quota_group_ids_range.is_none();

    if !user_use_all && user_list.is_empty() && config.quota_user_ids_range.is_none() {
        bail!("User quota ID selection is configured but resolved to no IDs");
    }
    if !group_use_all && group_list.is_empty() && config.quota_group_ids_range.is_none() {
        bail!("Group quota ID selection is configured but resolved to no IDs");
    }

    let mut tasks: Vec<(TargetToQuery, JoinHandle<Option<_>>)> = vec![];

    // These bound the concurrent requests going on to one node so these potentially long-running
    // requests don't block all available connections
    let mut semaphores = HashMap::new();

    // Sends one request per (target, id_type, list|range|all) to the respective owner node
    // Requesting is done concurrently for multiple targets but serialized for the different fetch
    // modes and multiple chunks.
    for target in targets {
        let app = app.clone();
        let user_list = user_list.clone();
        let group_list = group_list.clone();
        let user_range = config.quota_user_ids_range.clone();
        let group_range = config.quota_group_ids_range.clone();
        let semaphore = semaphores
            .entry(target.node_uid)
            .or_insert_with(|| Arc::new(Semaphore::new((config.connection_limit / 2).max(1))))
            .clone();

        tasks.push((target, tokio::spawn(async move {
            let mut responses = vec![];

            let _permit = match semaphore.acquire().await {
                Ok(p) => p,
                Err(err) => {
                    log::error!(
                        "Acquiring permit for fetching quota info for storage target {} from node \
                            with uid {} failed: {err:#}",
                        target.target_id,
                        target.node_uid
                    );

                    return None;
                }
            };

            if user_use_all {
                // If configured, query the whole id space
                range_requests(
                    app.clone(),
                    target.node_uid,
                    QuotaIdType::User,
                    &target,
                    &(0..=QuotaId::MAX),
                    "User id all",
                    &mut responses,
                )
                .await;
            } else {
                // Otherwise query the configured ids via list and range
                if let Some(ref range) = user_range {
                    range_requests(
                        app.clone(),
                        target.node_uid,
                        QuotaIdType::User,
                        &target,
                        range,
                        "User id range",
                        &mut responses,
                    )
                    .await;
                }
                if !user_list.is_empty() {
                    let resp: Result<GetQuotaInfoResp> = app
                        .request(
                            target.node_uid,
                            &GetQuotaInfo::with_list(
                                QuotaIdType::User,
                                target.target_id,
                                target.pool_id,
                                user_list,
                            ),
                        )
                        .await;

                    responses.push(("User id list", resp));
                }
            }

            if group_use_all {
                range_requests(
                    app.clone(),
                    target.node_uid,
                    QuotaIdType::Group,
                    &target,
                    &(0..=QuotaId::MAX),
                    "Group id all",
                    &mut responses,
                )
                .await;
            } else {
                // Otherwise query the configured ids via list and range
                if let Some(ref range) = group_range {
                    range_requests(
                        app.clone(),
                        target.node_uid,
                        QuotaIdType::Group,
                        &target,
                        range,
                        "Group id range",
                        &mut responses,
                    )
                    .await;
                }
                if !group_list.is_empty() {
                    let resp: Result<GetQuotaInfoResp> = app
                        .request(
                            target.node_uid,
                            &GetQuotaInfo::with_list(
                                QuotaIdType::Group,
                                target.target_id,
                                target.pool_id,
                                group_list,
                            ),
                        )
                        .await;

                    responses.push(("Group id list", resp));
                }
            }

            extract_results(&target, responses)
        })));
    }

    Ok(tasks)
}

async fn range_requests(
    app: impl App,
    node_uid: Uid,
    id_type: QuotaIdType,
    target: &TargetToQuery,
    range: &RangeInclusive<QuotaId>,
    log_str: &'static str,
    responses: &mut Vec<(&str, Result<GetQuotaInfoResp>)>,
) {
    let mut range_start = *range.start();
    let range_end = *range.end();
    let mut has_more = true;

    while has_more && range_start <= range_end {
        let resp = app
            .request_with_header::<_, GetQuotaInfoResp>(
                node_uid,
                &GetQuotaInfo::with_range(
                    id_type,
                    target.target_id,
                    target.pool_id,
                    &(range_start..=range_end),
                ),
            )
            .await;

        (has_more, range_start) = resp
            .as_ref()
            .map(|e| {
                let range_start =
                    e.0.quota_entry
                        .last()
                        .map(|s| s.id.saturating_add(1))
                        .unwrap_or_default();
                let has_more = range_start > 0
                    && e.1.msg_compat_feature_flags & GetQuotaInfoResp::HAS_MORE_ENTRIES_COMPATFLAG
                        != 0;

                (has_more, range_start)
            })
            .unwrap_or_default();

        responses.push((log_str, resp.map(|e| e.0)));
    }
}

/// Extracts the quota entries from the response message or log the errors
fn extract_results(
    target: &TargetToQuery,
    responses: Vec<(&str, Result<GetQuotaInfoResp>)>,
) -> Option<Vec<QuotaEntry>> {
    let mut results = vec![];
    let mut errs = String::new();

    for resp in responses {
        match resp.1 {
            Ok(mut msg) => {
                results.append(&mut msg.quota_entry);
            }
            Err(err) => {
                errs.push_str(&format!("\n{}: {err:#}", resp.0));
            }
        }
    }

    if errs.is_empty() {
        Some(results)
    } else {
        log::error!(
            "Fetching quota info for storage target {} from node with uid \
            {} failed:{errs}",
            target.target_id,
            target.node_uid
        );

        None
    }
}

/// Finds exceeded quota ids
///
/// The three parameters can be set to filter the data put into the result (before grouping) or set
/// to `None` to get everything. This uses a hardcoded `2 = both` for the quota accounting mode -
/// usage on a secondary is only counted when set to that mode.
///
/// Note that `quota_usage` is scanned either way: its primary key starts with `quota_id`, so
/// neither the fixed nor the optional form of the id type / quota type filters can seek on it (thus
/// no difference in performance).
pub(crate) const EXCEEDED_QUOTA_IDS_SQL: &str = sql!(
    "SELECT e.quota_id, e.id_type, e.quota_type, st.pool_id
    FROM quota_usage AS e
    INNER JOIN targets AS st USING(node_type, target_id)
    LEFT JOIN buddy_groups AS bg ON st.target_id = bg.s_target_id
        AND st.node_type = bg.node_type
    LEFT JOIN quota_default_limits AS d USING(id_type, quota_type, pool_id)
    LEFT JOIN quota_limits AS l USING(quota_id, id_type, quota_type, pool_id)
    WHERE (?1 IS NULL OR e.id_type = ?1)
        AND (?2 IS NULL OR e.quota_type = ?2)
        AND (?3 IS NULL OR st.pool_id = ?3)
        AND (bg.quota_accounting IS NULL OR bg.quota_accounting = 2)
    GROUP BY e.quota_id, e.id_type, e.quota_type, st.pool_id
    HAVING SUM(e.value) > COALESCE(l.value, d.value)"
);

/// Calculates and pushes exceeded quota info to the nodes
pub(crate) async fn distribute_exceeded(app: &impl App) -> Result<()> {
    if !app.static_info().user_config.quota_enforce {
        return Ok(());
    }

    let quota_licensed = app.verify_licensed_feature(LicensedFeature::Quota).is_ok();

    let (msges, nodes) = app
        .read_tx(move |tx| {
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

            if quota_licensed {
                // Fill the prepared messages with matching exceeded quota ids
                let mut stmt = tx.prepare_cached(EXCEEDED_QUOTA_IDS_SQL)?;
                let mut rows = stmt.query(params![None::<i64>, None::<i64>, None::<i64>])?;
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
            } else {
                // If quota is unlicensed, make sure the exceeding ids are removed from the servers.
                // Otherwise exceeded ids could stay exceeded forever if quota was used before.
                log::info!(
                    "Quota enforcement enabled but feature not licensed. Removing quota limits \
                    from nodes"
                );
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

    let start_time = Instant::now();
    let mut id_counter = 0;

    // Send all messages with exceeded quota information to all meta and storage nodes
    // Since there is one message for each combination of (pool x (user, group) x (space, inode)),
    // this might be very demanding, but can't do anything about that without changing meta and
    // storage too.
    // If this shows as a bottleneck, the requests could be done concurrently though.
    for msg in &msges {
        let mut request_fails = 0;
        let mut non_success_count = 0;

        id_counter += msg.exceeded_quota_ids.len();

        for node_uid in &nodes {
            match app.request::<_, SetExceededQuotaResp>(*node_uid, msg).await {
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

    log::info!(
        "Pushed {} exceeded quota ids to {} nodes using {} messages in {:?}",
        id_counter,
        nodes.len(),
        msges.len(),
        start_time.elapsed()
    );

    Ok(())
}

/// Tries to read quota IDs (users, groups) from a file
///
/// IDs must be in numerical form and separated by any whitespace.
fn try_read_quota_ids(path: &Path, read_into: &mut HashSet<QuotaId>) -> Result<()> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("Could not read quota id file {path:?}"))?;
    for id in data.split_whitespace().map(|e| e.parse()) {
        read_into.insert(id.with_context(|| format!("Invalid syntax in quota id file {path:?}"))?);
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::Config;
    use crate::app::test::*;
    use crate::types::{BuddyGroupQuotaAccounting, SqliteEnumExt};
    use shared::bee_msg::OpsErr;
    use shared::bee_msg::quota::{
        GetQuotaInfo, GetQuotaInfoResp, QuotaEntry, QuotaInodeSupport, QuotaQueryType,
        SetExceededQuota, SetExceededQuotaResp,
    };
    use shared::types::{QuotaIdType, QuotaType};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn update() {
        // Configure explicit ids via a file, which opts into list mode
        let mut path = std::env::temp_dir();
        path.push(format!("beegfs_quota_test_ids_{}", std::process::id()));
        std::fs::write(&path, "5 7 2398239").unwrap();

        let app = TestApp::with_config(Config {
            quota_enable: true,
            quota_user_ids_range: Some(0..=9),
            quota_group_ids_range: Some(0..=9),
            quota_user_ids_file: Some(path.clone()),
            quota_group_ids_file: Some(path.clone()),
            ..Default::default()
        })
        .await;

        app.set_request_handler(|req| {
            let r = req.downcast_ref::<GetQuotaInfo>().unwrap();

            let mut quota_entry = vec![];

            // Provide dummy quota values for target 1 depending on the id and type
            if r.target_id == 1 && r.query_type == QuotaQueryType::Range {
                for id in r.id_range_start..=r.id_range_end {
                    quota_entry.push(QuotaEntry {
                        space: id as u64 * 1000 + r.id_type.sql_variant() as u64,
                        inodes: id as u64 * 100 + r.id_type.sql_variant() as u64,
                        id,
                        id_type: r.id_type,
                        valid: 1,
                    });
                }
            } else if r.target_id == 2 && r.id_type == QuotaIdType::User {
                // This result is intentionally returned for both range and list query. The test
                // thus also checks if the result handling ignores duplicated
                // results.
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

        super::fetch_and_update(&app).await.unwrap();

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

        super::fetch_and_update(&app).await.unwrap();

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

        super::fetch_and_update(&app).await.unwrap();

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
    async fn distribute_exceeded() {
        // EXCEEDED_QUOTA_IDS_SQL hardcodes this value, it must not silently change
        assert_eq!(BuddyGroupQuotaAccounting::Both.sql_variant(), 2);

        // Without both of these, distribute_exceeded() returns early and nothing is asserted
        let app = TestApp::with_config(Config {
            quota_enable: true,
            quota_enforce: true,
            ..Default::default()
        })
        .await;

        let msg_count = Arc::new(AtomicUsize::new(0));
        let handler_count = msg_count.clone();

        app.set_request_handler(move |req| {
            handler_count.fetch_add(1, Ordering::SeqCst);
            let r = req.downcast_ref::<SetExceededQuota>().unwrap();

            match (r.pool_id, r.id_type, r.quota_type) {
                (1, QuotaIdType::User, QuotaType::Space) => {
                    assert_eq!(r.exceeded_quota_ids.as_slice(), &[2, 4, 10, 51])
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

        super::distribute_exceeded(&app).await.unwrap();

        // Guards against the assertions above silently not running at all
        assert!(
            msg_count.load(Ordering::SeqCst) > 0,
            "no SetExceededQuota messages were sent"
        );
    }
}
