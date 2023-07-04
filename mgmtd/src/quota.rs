use crate::config::ConfigCache;
use crate::db::quota_entry::QuotaData;
use crate::notification::request_tcp_by_type;
use crate::{db, MgmtdPool};
use anyhow::{Context, Result};
use shared::*;
use std::collections::HashSet;
use std::path::Path;

fn try_read_quota_ids(path: &Path, read_into: &mut HashSet<QuotaID>) -> Result<()> {
    let data = std::fs::read_to_string(path)?;
    for id in data.split_whitespace().map(|e| e.parse()) {
        read_into.insert(id.context("Invalid syntax in quota file {path}")?);
    }

    Ok(())
}

pub(crate) async fn update_and_distribute(
    db: &db::Connection,
    conn_pool: &MgmtdPool,
    config: &ConfigCache,
) -> Result<()> {
    // Fetch quota data from storage daemons

    let targets = db
        .execute(move |tx| db::target::get_with_type(tx, NodeTypeServer::Storage))
        .await?;

    if !targets.is_empty() {
        log::info!(
            "Requesting quota information for {:?}",
            targets.iter().map(|t| t.target_id).collect::<Vec<_>>()
        );
    }

    let (mut user_ids, mut group_ids) = (HashSet::new(), HashSet::new());

    let user_ids_min = config.get().quota_user_system_ids_min;
    if let Some(user_ids_min) = user_ids_min {
        system_ids::user_ids()
            .await
            .filter(|e| e >= &user_ids_min)
            .for_each(|e| {
                user_ids.insert(e);
            });
    }

    let group_ids_min = config.get().quota_group_system_ids_min;
    if let Some(group_ids_min) = group_ids_min {
        system_ids::group_ids()
            .await
            .filter(|e| e >= &group_ids_min)
            .for_each(|e| {
                group_ids.insert(e);
            });
    }

    if let Some(ref path) = config.get().quota_user_ids_file {
        try_read_quota_ids(path, &mut user_ids)?;
    }
    if let Some(ref path) = config.get().quota_group_ids_file {
        try_read_quota_ids(path, &mut group_ids)?;
    }

    if let Some(range) = config.get().quota_user_ids_range.clone() {
        user_ids.extend(range.into_iter().map(QuotaID::from));
    }
    if let Some(range) = config.get().quota_group_ids_range.clone() {
        group_ids.extend(range.into_iter().map(QuotaID::from));
    }

    let mut tasks = vec![];
    for t in targets {
        let pool_id = t.pool_id.try_into()?;

        let cp = conn_pool.clone();
        let user_ids2 = user_ids.clone();

        tasks.push(tokio::spawn(async move {
            let res: Result<msg::GetQuotaInfoResp> = cp
                .request(
                    PeerID::Node(t.node_uid),
                    &msg::GetQuotaInfo::with_user_ids(user_ids2, t.target_id, pool_id),
                )
                .await;

            (t.target_id, res)
        }));

        let cp = conn_pool.clone();
        let group_ids2 = group_ids.clone();

        tasks.push(tokio::spawn(async move {
            let res: Result<msg::GetQuotaInfoResp> = cp
                .request(
                    PeerID::Node(t.node_uid),
                    &msg::GetQuotaInfo::with_group_ids(group_ids2, t.target_id, pool_id),
                )
                .await;
            (t.target_id, res)
        }));
    }

    for t in tasks {
        let (target_id, resp) = t.await?;
        match resp {
            Ok(r) => {
                db.execute(move |tx| {
                    db::quota_entry::upsert(
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
            Err(err) => {
                log_error_chain!(
                    err,
                    "Getting quota info for storage target {:?} failed",
                    target_id
                );
            }
        }
    }

    // calculate exceeded quota information and send to daemons
    let mut msges: Vec<msg::SetExceededQuota> = vec![];
    for e in db
        .execute(db::quota_entry::all_exceeded_quota_entries)
        .await?
    {
        if let Some(last) = msges.last_mut() {
            if e.pool_id == last.pool_id
                && e.id_type == last.id_type
                && e.quota_type == last.quota_type
            {
                last.exceeded_quota_ids.push(e.quota_id);
                continue;
            }
        }

        msges.push(msg::SetExceededQuota {
            pool_id: e.pool_id,
            id_type: e.id_type,
            quota_type: e.quota_type,
            exceeded_quota_ids: vec![e.quota_id],
        });
    }

    for e in msges {
        let storage_responses: Vec<msg::SetExceededQuotaResp> =
            request_tcp_by_type(conn_pool, db, NodeTypeServer::Storage, e.clone()).await?;

        let meta_responses: Vec<msg::SetExceededQuotaResp> =
            request_tcp_by_type(conn_pool, db, NodeTypeServer::Meta, e.clone()).await?;

        let fail_count = storage_responses
            .iter()
            .chain(&meta_responses)
            .fold(0, |acc, e| {
                if e.result == OpsErr::SUCCESS {
                    acc
                } else {
                    acc + 1
                }
            });

        if fail_count > 0 {
            log::error!("Pushing exceeded quota IDs to nodes failed {fail_count} times");
        }
    }

    Ok(())
}

/// Contains functionality to query the systems user and group database.
mod system_ids {
    use shared::QuotaID;
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

        unsafe {
            libc::setpwent();
        }

        UserIDIter { _lock }
    }

    impl Drop for UserIDIter<'_> {
        fn drop(&mut self) {
            unsafe {
                libc::endpwent();
            }
        }
    }

    impl Iterator for UserIDIter<'_> {
        type Item = QuotaID;

        fn next(&mut self) -> Option<Self::Item> {
            unsafe {
                let passwd: *mut libc::passwd = libc::getpwent();
                if passwd.is_null() {
                    None
                } else {
                    Some((*passwd).pw_uid.into())
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

        unsafe {
            libc::setgrent();
        }

        GroupIDIter { _lock }
    }

    impl Drop for GroupIDIter<'_> {
        fn drop(&mut self) {
            unsafe {
                libc::endgrent();
            }
        }
    }

    impl Iterator for GroupIDIter<'_> {
        type Item = QuotaID;

        fn next(&mut self) -> Option<Self::Item> {
            unsafe {
                let passwd: *mut libc::group = libc::getgrent();
                if passwd.is_null() {
                    None
                } else {
                    Some((*passwd).gr_gid.into())
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
