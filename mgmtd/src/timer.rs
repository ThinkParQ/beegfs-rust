//! Contains timers executing periodic tasks.

use crate::App;
use crate::app::RuntimeApp;
use crate::db::{self};
use crate::license::LicensedFeature;
use crate::quota::update_and_distribute;
use shared::bee_msg::target::RefreshTargetStates;
use shared::run_state::RunStateHandle;
use shared::types::NodeType;
use tokio::time::{MissedTickBehavior, sleep};

/// Starts the timed tasks.
pub(crate) fn start_tasks(app: RuntimeApp, run_state: RunStateHandle) {
    // TODO send out timer based RefreshTargetStates notification if a reachability
    // state changed ?

    tokio::spawn(delete_stale_clients(app.clone(), run_state.clone()));
    tokio::spawn(switchover(app.clone(), run_state.clone()));

    if app.info.user_config.quota_enable {
        if let Err(err) = app.license.verify_licensed_feature(LicensedFeature::Quota) {
            log::error!(
                "Quota is enabled in the config, but the feature could not be verified. Continuing without quota support: {err}"
            );
        } else {
            tokio::spawn(update_quota(app, run_state));
        }
    }
}

/// Deletes client nodes from the database which haven't responded for the configured time.
async fn delete_stale_clients(app: RuntimeApp, mut run_state: RunStateHandle) {
    let timeout = app.info.user_config.client_auto_remove_timeout;

    loop {
        tokio::select! {
            _ = sleep(timeout) => {}
            _ = run_state.wait_for_pre_shutdown() => { break; }
        }

        log::debug!("Running stale client deleter");

        match app
            .db
            .write_tx(move |tx| db::node::delete_stale_clients(tx, timeout))
            .await
        {
            Ok(affected) => {
                if affected > 0 {
                    log::info!("Deleted {affected} stale clients");
                }
            }
            Err(err) => log::error!("Deleting stale clients failed: {err:#}"),
        }
    }

    log::debug!("Timed task delete_stale_clients exited");
}

/// Fetches quota information for all storage targets, calculates exceeded IDs and distributes them.
async fn update_quota(app: RuntimeApp, mut run_state: RunStateHandle) {
    loop {
        log::debug!("Running quota update");

        match update_and_distribute(&app).await {
            Ok(_) => {}
            Err(err) => log::error!("Updating quota failed: {err:#}"),
        }

        tokio::select! {
            _ = sleep(app.info.user_config.quota_update_interval) => {}
            _ = run_state.wait_for_pre_shutdown() => { break; }
        }
    }

    log::debug!("Timed task update_quota exited");
}

/// Finds buddy groups with switchover condition, swaps them and notifies nodes.
async fn switchover(app: RuntimeApp, mut run_state: RunStateHandle) {
    // On the other nodes / old management, the interval in which the switchover checks are done
    // is determined by "1/6 sysTargetOfflineTimeoutSecs".
    // This is also the interval the target states are being pushed to management. To avoid an
    // accidental switchover after management shutdown in case a secondary reports in first but an
    // up-and-running primary doesn't because of their timing, this value should be the same as on
    // the nodes. If we delay the initial check by that time, then a running primary has enough time
    // to report in and update the last contact time before the check happens.
    let interval = app.info.user_config.node_offline_timeout / 6;
    let mut timer = tokio::time::interval(interval);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // First call of tick completes immediately
    timer.tick().await;

    loop {
        tokio::select! {
            _ = timer.tick() => {}
            _ = run_state.wait_for_pre_shutdown() => { break; }
        }

        log::debug!("Running switchover check");

        let timeout = app.info.user_config.node_offline_timeout;

        match app
            .db
            .write_tx(move |tx| db::buddy_group::check_and_swap_buddies(tx, timeout))
            .await
        {
            Ok(swapped) => {
                if !swapped.is_empty() {
                    log::warn!(
                        "A switchover was triggered for the following buddy groups: {swapped:?}"
                    );

                    app.send_notifications(
                        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                        &RefreshTargetStates { ack_id: "".into() },
                    )
                    .await;
                }
            }
            Err(err) => log::error!("Switchover check failed: {err:#}"),
        }
    }

    log::debug!("Timed task check_for_switchover exited");
}
