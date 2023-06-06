use crate::db::{self};
use crate::notification::notify_nodes;
use crate::quota::update_and_distribute;
use crate::MgmtdPool;
use config::Cache;
use shared::config::{
    BeeConfig, ClientAutoRemoveTimeout, NodeOfflineTimeout, QuotaEnable, QuotaUpdateInterval,
};
use shared::msg;
use shared::shutdown::Shutdown;
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
            match update_and_distribute(&db, &conn_pool, &config).await {
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
