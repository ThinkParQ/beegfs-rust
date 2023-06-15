use crate::db::quota_entries::QuotaData;
use crate::notification::request_tcp_by_type;
use crate::{db, MgmtdPool};
use ::config::Cache;
use anyhow::{anyhow, Context, Result};
use shared::config::{BeeConfig, *};
use shared::*;
use std::collections::HashSet;
use std::path::Path;

fn try_read_system_ids(file: &str, min: QuotaID, read_into: &mut HashSet<QuotaID>) -> Result<()> {
    for l in std::fs::read_to_string(file)?.lines() {
        let id = l
            .split(':')
            .nth(2)
            .ok_or_else(|| anyhow!("Not enough fields in {file}"))?
            .parse()
            .with_context(|| anyhow!("Could not parse ID in {file}"))?;

        if id >= min {
            read_into.insert(id);
        }
    }

    Ok(())
}

fn try_read_system_user_ids(min: QuotaID, read_into: &mut HashSet<QuotaID>) -> Result<()> {
    try_read_system_ids("/etc/passwd", min, read_into)
}

fn try_read_system_group_ids(min: QuotaID, read_into: &mut HashSet<QuotaID>) -> Result<()> {
    try_read_system_ids("/etc/group", min, read_into)
}

fn try_read_quota_ids(path: &Path, read_into: &mut HashSet<QuotaID>) -> Result<()> {
    let data = std::fs::read_to_string(path)?;
    for id in data.split_whitespace().map(|e| e.parse()) {
        read_into.insert(id.context("Invalid syntax in quota file {path}")?);
    }

    Ok(())
}

pub(crate) async fn update_and_distribute(
    db: &db::Handle,
    conn_pool: &MgmtdPool,
    config: &Cache<BeeConfig>,
) -> Result<()> {
    // Fetch quota data from storage daemons

    let targets = db
        .execute(move |tx| db::targets::with_type(tx, NodeTypeServer::Storage))
        .await?;

    if !targets.is_empty() {
        log::info!(
            "Requesting quota information for {:?}",
            targets.iter().map(|t| t.target_id).collect::<Vec<_>>()
        );
    }

    let (mut user_ids, mut group_ids) = (HashSet::new(), HashSet::new());

    if let Some(min_id) = config.get::<QuotaUserSystemIDsMin>() {
        try_read_system_user_ids(min_id, &mut user_ids)?;
    }
    if let Some(min_id) = config.get::<QuotaGroupSystemIDsMin>() {
        try_read_system_group_ids(min_id, &mut group_ids)?;
    }

    if let Some(ref path) = config.get::<QuotaUserIDsFile>() {
        try_read_quota_ids(path, &mut user_ids)?;
    }
    if let Some(ref path) = config.get::<QuotaGroupIDsFile>() {
        try_read_quota_ids(path, &mut group_ids)?;
    }

    if let Some(range) = config.get::<QuotaUserIDsRange>() {
        user_ids.extend(range.into_iter().map(QuotaID::from));
    }
    if let Some(range) = config.get::<QuotaGroupIDsRange>() {
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
                    db::quota_entries::upsert(
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
                log::error!("Getting quota info for storage target {target_id:?} failed:\n{err:?}");
            }
        }
    }

    // calculate exceeded quota information and send to daemons
    let mut msges: Vec<msg::SetExceededQuota> = vec![];
    for e in db
        .execute(db::quota_entries::all_exceeded_quota_entries)
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn users() {
        let mut output = HashSet::new();
        try_read_system_user_ids(0.into(), &mut output).unwrap();

        assert!(!output.is_empty());
    }
}
