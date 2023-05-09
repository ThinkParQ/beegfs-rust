use crate::db::quota_entries::QuotaData;
use crate::db::{self};
use crate::notification::{notify_nodes, request_tcp_by_type};
use crate::MgmtdPool;
use anyhow::Result;
use config::Cache;
use shared::config::{
    BeeConfig, ClientAutoRemoveTimeout, NodeOfflineTimeout, QuotaEnable, QuotaGroupIDs,
    QuotaUpdateInterval, QuotaUserIDs,
};
use shared::conn::PeerID;
use shared::shutdown::Shutdown;
use shared::{msg, NodeTypeServer, OpsErr};
use std::time::Duration;
use tokio::time::{sleep, MissedTickBehavior};

pub(crate) fn start_tasks(
    db: db::Handle,
    conn_pool: MgmtdPool,
    config: Cache<BeeConfig>,
    shutdown: Shutdown,
) {
    // TODO send out timer based RefreshTargetStates notification if a reachability
    // state changed ?

    tokio::spawn(delete_stale_clients(
        db.clone(),
        config.clone(),
        shutdown.clone(),
    ));
    tokio::spawn(update_quota(
        db.clone(),
        conn_pool.clone(),
        config.clone(),
        shutdown.clone(),
    ));
    tokio::spawn(check_for_switchover(db, conn_pool, config, shutdown));
}

async fn delete_stale_clients(db: db::Handle, config: Cache<BeeConfig>, mut shutdown: Shutdown) {
    loop {
        let timeout = config.get::<ClientAutoRemoveTimeout>();

        match db
            .execute(move |tx| db::nodes::delete_stale_clients(tx, timeout))
            .await
        {
            Ok(affected) => {
                if affected > 0 {
                    log::info!("Deleted {} stale clients", affected);
                }
            }
            Err(err) => log::error!("Deleting stale clients failed:\n{:?}", err),
        }

        tokio::select! {
            _ = sleep(config.get::<ClientAutoRemoveTimeout>()) => {}
            _ = shutdown.wait() => { break; }
        }
    }

    log::debug!("Timed task delete_stale_clients has been shut down");
}

async fn update_quota(
    db: db::Handle,
    conn_pool: MgmtdPool,
    config: Cache<BeeConfig>,
    mut shutdown: Shutdown,
) {
    loop {
        if config.get::<QuotaEnable>() {
            match update_quota_inner(&db, &conn_pool, &config).await {
                Ok(_) => {}
                Err(err) => log::error!("Updating quota failed:\n{:?}", err),
            }
        }

        tokio::select! {
            _ = sleep(config.get::<QuotaUpdateInterval>()) => {}
            _ = shutdown.wait() => { break; }
        }
    }

    log::debug!("Timed task update_quota has been shut down");
}

async fn update_quota_inner(
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

    let mut tasks = vec![];
    for t in targets {
        let cp = conn_pool.clone();
        let cfg = config.clone();
        let pool_id = t.pool_id.try_into()?;

        tasks.push(tokio::spawn(async move {
            let res: Result<msg::GetQuotaInfoResp> = cp
                .request(
                    PeerID::Node(t.node_uid),
                    &msg::GetQuotaInfo::with_user_ids(
                        cfg.get::<QuotaUserIDs>(),
                        t.target_id,
                        pool_id,
                    ),
                )
                .await;

            (t.target_id, res)
        }));

        let cp = conn_pool.clone();
        let cfg = config.clone();

        tasks.push(tokio::spawn(async move {
            let res: Result<msg::GetQuotaInfoResp> = cp
                .request(
                    PeerID::Node(t.node_uid),
                    &msg::GetQuotaInfo::with_group_ids(
                        cfg.get::<QuotaGroupIDs>(),
                        t.target_id,
                        pool_id,
                    ),
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
        .execute(db::quota_entries::exceeded_quota_entries)
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

async fn check_for_switchover(
    db: db::Handle,
    conn_pool: MgmtdPool,
    config: Cache<BeeConfig>,
    mut shutdown: Shutdown,
) {
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = timer.tick() => {}
            _ = shutdown.wait() => { break; }
        }

        let timeout = config.get::<NodeOfflineTimeout>();

        match db
            .execute(move |tx| db::buddy_groups::check_and_swap_buddies(tx, timeout))
            .await
        {
            Ok(swapped) => {
                if !swapped.is_empty() {
                    log::warn!(
                        "A switchover was triggered for the following buddy groups: {swapped:?}"
                    );

                    notify_nodes(
                        &conn_pool,
                        &db,
                        &msg::RefreshTargetStates { ack_id: "".into() },
                    )
                    .await;
                }
            }
            Err(err) => log::error!("Switchover check failed:\n{err}"),
        }
    }

    log::debug!("Timed task check_for_switchover has been shut down");
}
