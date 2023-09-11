//! Contains timers executing periodic tasks.

use crate::app_context::AppContext;
use crate::db::{self};
use crate::quota::update_and_distribute;
use shared::shutdown::Shutdown;
use shared::{log_error_chain, msg, NodeType};
use std::time::Duration;
use tokio::time::{sleep, MissedTickBehavior};

/// Starts the timed tasks.
pub(crate) fn start_tasks(ctx: impl AppContext, shutdown: Shutdown) {
    // TODO send out timer based RefreshTargetStates notification if a reachability
    // state changed ?

    tokio::spawn(delete_stale_clients(ctx.clone(), shutdown.clone()));
    tokio::spawn(update_quota(ctx.clone(), shutdown.clone()));
    tokio::spawn(switchover(ctx, shutdown));
}

/// Deletes client nodes from the database which haven't responded for the configured time.
async fn delete_stale_clients(ctx: impl AppContext, mut shutdown: Shutdown) {
    loop {
        let timeout = ctx.runtime_info().config.client_auto_remove_timeout;

        match ctx
            .db_op(move |tx| db::node::delete_stale_clients(tx, timeout))
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

/// Fetches quota information for all storage targets, calculates exceeded IDs and distributes them.
async fn update_quota(ctx: impl AppContext, mut shutdown: Shutdown) {
    loop {
        if ctx.runtime_info().config.quota_enable {
            match update_and_distribute(&ctx).await {
                Ok(_) => {}
                Err(err) => log_error_chain!(err, "Updating quota failed"),
            }
        }

        tokio::select! {
            _ = sleep(ctx.runtime_info().config.quota_update_interval) => {}
            _ = shutdown.wait() => { break; }
        }
    }

    log::debug!("Timed task update_quota has been shut down");
}

/// Finds buddy groups with switchover condition, swaps them and notifies nodes.
async fn switchover(ctx: impl AppContext, mut shutdown: Shutdown) {
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = timer.tick() => {}
            _ = shutdown.wait() => { break; }
        }

        let timeout = ctx.runtime_info().config.node_offline_timeout;

        match ctx
            .db_op(move |tx| db::buddy_group::check_and_swap_buddies(tx, timeout))
            .await
        {
            Ok(swapped) => {
                if !swapped.is_empty() {
                    log::warn!(
                        "A switchover was triggered for the following buddy groups: {swapped:?}"
                    );

                    ctx.notify_nodes(
                        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
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
