use crate::config::ConfigCache;
use crate::db::{self};
use crate::notification::notify_nodes;
use crate::quota::update_and_distribute;
use crate::MgmtdPool;
use shared::shutdown::Shutdown;
use shared::{log_error_chain, msg};
use std::time::Duration;
use tokio::time::{sleep, MissedTickBehavior};

pub(crate) fn start_tasks(
    db: db::Connection,
    conn_pool: MgmtdPool,
    config: ConfigCache,
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

async fn delete_stale_clients(db: db::Connection, config: ConfigCache, mut shutdown: Shutdown) {
    loop {
        let timeout = config.get().client_auto_remove_timeout;

        match db
            .op(move |tx| db::node::delete_stale_clients(tx, timeout))
            .await
        {
            Ok(affected) => {
                if affected > 0 {
                    log::info!("Deleted {} stale clients", affected);
                }
            }
            Err(err) => log_error_chain!(err, "Deleting stale clients failed"),
        }

        tokio::select! {
            _ = sleep(timeout) => {}
            _ = shutdown.wait() => { break; }
        }
    }

    log::debug!("Timed task delete_stale_clients has been shut down");
}

async fn update_quota(
    db: db::Connection,
    conn_pool: MgmtdPool,
    config: ConfigCache,
    mut shutdown: Shutdown,
) {
    loop {
        if config.get().quota_enable {
            match update_and_distribute(&db, &conn_pool, &config).await {
                Ok(_) => {}
                Err(err) => log_error_chain!(err, "Updating quota failed"),
            }
        }

        tokio::select! {
            _ = sleep(config.get().quota_update_interval) => {}
            _ = shutdown.wait() => { break; }
        }
    }

    log::debug!("Timed task update_quota has been shut down");
}

async fn check_for_switchover(
    db: db::Connection,
    conn_pool: MgmtdPool,
    config: ConfigCache,
    mut shutdown: Shutdown,
) {
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = timer.tick() => {}
            _ = shutdown.wait() => { break; }
        }

        let timeout = config.get().node_offline_timeout;

        match db
            .op(move |tx| db::buddy_group::check_and_swap_buddies(tx, timeout))
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
            Err(err) => log_error_chain!(err, "Switchover check failed"),
        }
    }

    log::debug!("Timed task check_for_switchover has been shut down");
}
