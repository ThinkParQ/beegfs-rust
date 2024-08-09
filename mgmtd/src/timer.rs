//! Contains timers executing periodic tasks.

use crate::context::Context;
use crate::db::{self};
use crate::license::LicensedFeature;
use crate::quota::update_and_distribute;
use shared::bee_msg::target::RefreshTargetStates;
use shared::log_error_chain;
use shared::shutdown::Shutdown;
use shared::types::NodeType;
use sqlite::ConnectionExt;
use std::time::Duration;
use tokio::time::{sleep, MissedTickBehavior};

/// Starts the timed tasks.
pub(crate) fn start_tasks(ctx: Context, shutdown: Shutdown) {
    // TODO send out timer based RefreshTargetStates notification if a reachability
    // state changed ?

    tokio::spawn(delete_stale_clients(ctx.clone(), shutdown.clone()));
    tokio::spawn(switchover(ctx.clone(), shutdown.clone()));

    if ctx.info.user_config.quota_enable {
        if let Err(err) = ctx.lic.verify_feature(LicensedFeature::Quota) {
            log::error!("Quota is enabled in the config, but the feature could not be verified. Continuing without quota support: {err}");
        } else {
            tokio::spawn(update_quota(ctx, shutdown));
        }
    }
}

/// Deletes client nodes from the database which haven't responded for the configured time.
async fn delete_stale_clients(ctx: Context, mut shutdown: Shutdown) {
    loop {
        let timeout = ctx.info.user_config.client_auto_remove_timeout;

        match ctx
            .db
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

/// Fetches quota information for all storage targets, calculates exceeded IDs and distributes them.
async fn update_quota(ctx: Context, mut shutdown: Shutdown) {
    loop {
        match update_and_distribute(&ctx).await {
            Ok(_) => {}
            Err(err) => log_error_chain!(err, "Updating quota failed"),
        }

        tokio::select! {
            _ = sleep(ctx.info.user_config.quota_update_interval) => {}
            _ = shutdown.wait() => { break; }
        }
    }

    log::debug!("Timed task update_quota has been shut down");
}

/// Finds buddy groups with switchover condition, swaps them and notifies nodes.
async fn switchover(ctx: Context, mut shutdown: Shutdown) {
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = timer.tick() => {}
            _ = shutdown.wait() => { break; }
        }

        let timeout = ctx.info.user_config.node_offline_timeout;

        match ctx
            .db
            .op(move |tx| db::buddy_group::check_and_swap_buddies(tx, timeout))
            .await
        {
            Ok(swapped) => {
                if !swapped.is_empty() {
                    log::warn!(
                        "A switchover was triggered for the following buddy groups: {swapped:?}"
                    );

                    crate::bee_msg::notify_nodes(
                        &ctx,
                        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                        &RefreshTargetStates { ack_id: "".into() },
                    )
                    .await;
                }
            }
            Err(err) => log_error_chain!(err, "Switchover check failed"),
        }
    }

    log::debug!("Timed task check_for_switchover has been shut down");
}
