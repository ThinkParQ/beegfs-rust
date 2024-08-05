//! Functionality for fetching and updating quota information from / to nodes and the database.

use crate::context::Context;
use crate::db;
use crate::db::node::Node;
use crate::db::quota_usage::QuotaData;
use anyhow::{anyhow, Context as AnyhowContext, Result};
use shared::bee_msg::quota::{
    GetQuotaInfo, GetQuotaInfoResp, SetExceededQuota, SetExceededQuotaResp,
};
use shared::bee_msg::OpsErr;
use shared::log_error_chain;
use shared::types::{NodeType, NodeTypeServer, QuotaId};
use sqlite::ConnectionExt;
use std::collections::HashSet;
use std::path::Path;

/// Fetches quota information for all storage targets, calculates exceeded IDs and distributes them.
pub(crate) async fn update_and_distribute(ctx: &Context) -> Result<()> {
    // Fetch quota data from storage daemons

    let targets = ctx
        .db
        .op(move |tx| db::target::get_with_type(tx, NodeTypeServer::Storage))
        .await?;

    if !targets.is_empty() {
        log::info!(
            "Fetching quota information for {:?}",
            targets.iter().map(|t| t.target_id).collect::<Vec<_>>()
        );
    }

    // The to-be-queried IDs
    let (mut user_ids, mut group_ids) = (HashSet::new(), HashSet::new());

    // If configured, add system User IDS
    let user_ids_min = ctx.info.user_config.quota_user_system_ids_min;

    if let Some(user_ids_min) = user_ids_min {
        system_ids::user_ids()
            .await
            .filter(|e| e >= &user_ids_min)
            .for_each(|e| {
                user_ids.insert(e);
            });
    }

    // If configured, add system Group IDS
    let group_ids_min = ctx.info.user_config.quota_group_system_ids_min;

    if let Some(group_ids_min) = group_ids_min {
        system_ids::group_ids()
            .await
            .filter(|e| e >= &group_ids_min)
            .for_each(|e| {
                group_ids.insert(e);
            });
    }

    // If configured, add user IDs from file
    if let Some(ref path) = ctx.info.user_config.quota_user_ids_file {
        try_read_quota_ids(path, &mut user_ids)?;
    }

    // If configured, add group IDs from file
    if let Some(ref path) = ctx.info.user_config.quota_group_ids_file {
        try_read_quota_ids(path, &mut group_ids)?;
    }

    // If configured, add range based user IDs
    if let Some(range) = &ctx.info.user_config.quota_user_ids_range {
        user_ids.extend(range.clone().map(QuotaId::from));
    }

    // If configured, add range based group IDs
    if let Some(range) = &ctx.info.user_config.quota_group_ids_range {
        group_ids.extend(range.clone().map(QuotaId::from));
    }

    let mut tasks = vec![];
    // Sends one request per target to the respective owner node
    // Requesting is done concurrently.
    for t in targets {
        let pool_id = t
            .pool_id
            .ok_or_else(|| anyhow!("storage targets must have a storage pool assigned"))?;

        let ctx2 = ctx.clone();
        let user_ids2 = user_ids.clone();

        // Users
        tasks.push(tokio::spawn(async move {
            let resp: Result<GetQuotaInfoResp> = ctx2
                .conn
                .request(
                    t.node_uid,
                    &GetQuotaInfo::with_user_ids(user_ids2, t.target_id, pool_id),
                )
                .await;

            // Log immediately so there is no delay if other tasks have to wait and get joined
            // first
            if let Err(ref err) = resp {
                log_error_chain!(
                    err,
                    "Fetching user quota info for storage target {:?} failed",
                    t.target_id
                );
            }
            (t.target_id, resp)
        }));

        let ctx2 = ctx.clone();
        let group_ids2 = group_ids.clone();

        // Groups
        tasks.push(tokio::spawn(async move {
            let resp = ctx2
                .conn
                .request(
                    t.node_uid,
                    &GetQuotaInfo::with_group_ids(group_ids2, t.target_id, pool_id),
                )
                .await;

            // Log immediately so there is no delay if other tasks have to wait and get joined
            // first
            if let Err(ref err) = resp {
                log_error_chain!(
                    err,
                    "Fetching group quota info for storage target {:?} failed",
                    t.target_id
                );
            }

            (t.target_id, resp)
        }));
    }

    // Await all the responses
    for t in tasks {
        let (target_id, resp) = t.await?;
        if let Ok(r) = resp {
            // Insert quota usage data into the database
            ctx.db
                .op(move |tx| {
                    db::quota_usage::update(
                        tx,
                        target_id,
                        r.quota_entry.into_iter().map(|e| QuotaData {
                            quota_id: e.id,
                            id_type: e.id_type,
                            space: e.space,
                            inodes: e.inodes,
                        }),
                    )
                })
                .await?;
        }
    }

    // calculate exceeded quota information and create messages
    let mut msges: Vec<SetExceededQuota> = vec![];
    for e in ctx.db.op(db::quota_usage::all_exceeded_quota_ids).await? {
        if let Some(last) = msges.last_mut() {
            if e.pool_id == last.pool_id
                && e.id_type == last.id_type
                && e.quota_type == last.quota_type
            {
                last.exceeded_quota_ids.push(e.quota_id);
                continue;
            }
        }

        msges.push(SetExceededQuota {
            pool_id: e.pool_id,
            id_type: e.id_type,
            quota_type: e.quota_type,
            exceeded_quota_ids: vec![e.quota_id],
        });
    }

    // Get all meta and storage nodes
    let (meta_nodes, storage_nodes) = ctx
        .db
        .op(move |tx| {
            Ok((
                db::node::get_with_type(tx, NodeType::Meta)?,
                db::node::get_with_type(tx, NodeType::Storage)?,
            ))
        })
        .await?;
    let nodes: Vec<Node> = meta_nodes.into_iter().chain(storage_nodes).collect();

    // Send all messages with exceeded quota information to all meta and storage nodes
    // Since there is one message for each combination of (pool x (user, group) x (space, inode)),
    // this might be very demanding, but can't do anything about that without changing meta and
    // storage too.
    // If this shows as a bottleneck, the requests could be done concurrently though.
    for msg in msges {
        let mut request_fails = 0;
        let mut non_success_count = 0;
        for node in &nodes {
            match ctx
                .conn
                .request::<_, SetExceededQuotaResp>(node.uid, &msg)
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
