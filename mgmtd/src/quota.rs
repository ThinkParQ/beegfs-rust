//! Functionality for fetching and updating quota information from / to nodes and the database.

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
        system_ids::user_ids()
            .await
            .filter(|e| e >= &user_ids_min)
            .for_each(|e| {
                user_ids.insert(e);
            });
    }

    // If configured, add system Group IDS
    let group_ids_min = app.static_info().user_config.quota_group_system_ids_min;

    if let Some(group_ids_min) = group_ids_min {
        system_ids::group_ids()
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

/// Contains functionality to query the systems user and group database.
mod system_ids {
    use std::sync::OnceLock;
    use tokio::sync::{Mutex, MutexGuard};

    // SAFETY (applies to both user and group id iterators)
    //
    // * The global mutex assures that no more than one iterator object exists and therefore
    // undefined results by concurrent access are prevented (it obviously doesn't prevent reusing
    // libc::setpwent() elsewhere, don't do this!)
    // * getpwent() / getgrent() return the next entry or a nullptr in case EOF is reached or an
    // error occurs. Both cases are covered.

    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    /// Iterator over system user IDs
    pub struct UserIDIter<'a> {
        _lock: MutexGuard<'a, ()>,
    }

    /// Retrieves system user IDs.
    ///
    /// Uses `getpwent()` libc call family.
    ///
    /// # Return value
    /// An iterator iterating over the systems user IDs. This function will block all other tasks
    /// until the iterator is dropped.
    pub async fn user_ids<'a>() -> UserIDIter<'a> {
        let _lock = MUTEX.get_or_init(|| Mutex::new(())).lock().await;

        // SAFETY: See above
        unsafe {
            libc::setpwent();
        }

        UserIDIter { _lock }
    }

    impl Drop for UserIDIter<'_> {
        fn drop(&mut self) {
            // SAFETY: See above
            unsafe {
                libc::endpwent();
            }
        }
    }

    impl Iterator for UserIDIter<'_> {
        type Item = u32;

        fn next(&mut self) -> Option<Self::Item> {
            // SAFETY: See above
            unsafe {
                let passwd: *mut libc::passwd = libc::getpwent();
                if passwd.is_null() {
                    None
                } else {
                    Some((*passwd).pw_uid)
                }
            }
        }
    }

    /// Iterator over system group IDs
    pub struct GroupIDIter<'a> {
        _lock: MutexGuard<'a, ()>,
    }

    /// Retrieves system group IDs.
    ///
    /// Uses `getgrent()` libc call.
    ///
    /// # Return value
    /// An iterator iterating over the systems group IDs. This function will block all other tasks
    /// until the iterator is dropped.
    pub async fn group_ids<'a>() -> GroupIDIter<'a> {
        let _lock = MUTEX.get_or_init(|| Mutex::new(())).lock().await;

        // SAFETY: See above
        unsafe {
            libc::setgrent();
        }

        GroupIDIter { _lock }
    }

    impl Drop for GroupIDIter<'_> {
        fn drop(&mut self) {
            // SAFETY: See above
            unsafe {
                libc::endgrent();
            }
        }
    }

    impl Iterator for GroupIDIter<'_> {
        type Item = u32;

        fn next(&mut self) -> Option<Self::Item> {
            // SAFETY: See above
            unsafe {
                let passwd: *mut libc::group = libc::getgrent();
                if passwd.is_null() {
                    None
                } else {
                    Some((*passwd).gr_gid)
                }
            }
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;
        use itertools::Itertools;

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn user_ids_thread_safety() {
            let tasks: Vec<_> = (0..16)
                .map(|_| tokio::spawn(async { user_ids().await.collect() }))
                .collect();

            let mut results = vec![];
            for t in tasks {
                let r: Vec<_> = t.await.unwrap();
                results.push(r);
            }

            assert!(results.into_iter().all_equal());
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn group_ids_thread_safety() {
            let tasks: Vec<_> = (0..16)
                .map(|_| tokio::spawn(async { group_ids().await.collect() }))
                .collect();

            let mut results = vec![];
            for t in tasks {
                let r: Vec<_> = t.await.unwrap();
                results.push(r);
            }

            assert!(results.into_iter().all_equal());
        }
    }
}
