//! Contains timers executing periodic tasks.

use crate::App;
use crate::app::RuntimeApp;
use crate::db::{self};
use crate::license::LicensedFeature;
use crate::types::SqliteEnumExt;
use rusqlite::params;
use shared::bee_msg::target::RefreshTargetStates;
use shared::run_state::RunStateHandle;
use shared::types::NodeType;
use sqlite_check::sql;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, sleep};

/// Starts the timed tasks.
pub(crate) fn start_tasks(app: RuntimeApp, run_state: RunStateHandle) {
    tokio::spawn(delete_stale_clients_loop(app.clone(), run_state.clone()));
    tokio::spawn(check_switchover_loop(app.clone(), run_state.clone()));

    if app.info.user_config.quota_enable {
        if let Err(err) = app.license.verify_licensed_feature(LicensedFeature::Quota) {
            log::error!(
                "Quota is enabled in the config, but the feature could not be verified. Continuing without quota support: {err}"
            );
        } else {
            tokio::spawn(update_quota_loop(app, run_state));
        }
    }
}

/// Deletes client nodes from the database which haven't responded for the configured time.
async fn delete_stale_clients_loop(app: impl App, mut run_state: RunStateHandle) {
    let timeout = app.static_info().user_config.client_auto_remove_timeout;

    loop {
        tokio::select! {
            _ = sleep(timeout) => { delete_stale_clients(&app, timeout).await }
            _ = run_state.wait_for_pre_shutdown() => { break; }
        }
    }

    log::debug!("Timed task delete_stale_clients exited");
}

async fn delete_stale_clients(app: &impl App, timeout: Duration) {
    log::debug!("Running stale client deleter");

    match app
        .write_tx(move |tx| {
            let mut stmt = tx.prepare_cached(sql!(
                "DELETE FROM nodes
                WHERE DATETIME(last_contact) < DATETIME('now', '-' || ?1 || ' seconds')
                AND node_type = ?2"
            ))?;
            stmt.execute(params![timeout.as_secs(), NodeType::Client.sql_variant()])
                .map_err(|err| err.into())
        })
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

/// Fetches quota information for all storage targets, calculates exceeded IDs and distributes them.
async fn update_quota_loop(app: impl App, mut run_state: RunStateHandle) {
    let interval = app.static_info().user_config.quota_update_interval;

    loop {
        log::debug!("Running quota update");

        match crate::quota::update_and_distribute(&app).await {
            Ok(_) => {}
            Err(err) => log::error!("Updating quota failed: {err:#}"),
        }

        tokio::select! {
            _ = sleep(interval) => {}
            _ = run_state.wait_for_pre_shutdown() => { break; }
        }
    }

    log::debug!("Timed task update_quota exited");
}

/// Finds buddy groups with switchover condition, swaps them and notifies nodes.
async fn check_switchover_loop(app: impl App, mut run_state: RunStateHandle) {
    // On the other nodes / old management, the interval in which the switchover checks are done
    // is determined by "1/6 sysTargetOfflineTimeoutSecs".
    // This is also the interval the target states are being pushed to management. To avoid an
    // accidental switchover after management shutdown in case a secondary reports in first but an
    // up-and-running primary doesn't because of their timing, this value should be the same as on
    // the nodes. If we delay the initial check by that time, then a running primary has enough time
    // to report in and update the last contact time before the check happens.
    let offline_timeout = app.static_info().user_config.node_offline_timeout;
    let interval = offline_timeout / 6;

    let mut timer = tokio::time::interval(interval);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // First call of tick completes immediately
    timer.tick().await;

    loop {
        tokio::select! {
            _ = timer.tick() => { check_switchover(&app, offline_timeout).await }
            _ = run_state.wait_for_pre_shutdown() => { break; }
        }
    }

    log::debug!("Timed task check_for_switchover exited");
}

async fn check_switchover(app: &impl App, offline_timeout: Duration) {
    log::debug!("Running switchover check");

    match app
        .write_tx(move |tx| db::buddy_group::check_and_swap_buddies(tx, offline_timeout))
        .await
    {
        Ok(switched) => {
            if !switched.is_empty() {
                log::warn!(
                    "A switchover was triggered for the following buddy groups: {switched:?}"
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

#[cfg(test)]
mod test {
    use crate::App;
    use crate::app::test::*;
    use crate::types::SqliteEnumExt;
    use shared::types::NodeType;
    use std::time::Duration;

    #[tokio::test]
    async fn delete_stale_clients() {
        let app = TestApp::new().await;

        super::delete_stale_clients(&app, Duration::from_secs(10)).await;
        assert_eq_db!(app, "SELECT COUNT(*) FROM client_nodes", [], 4);

        app.write_tx(|tx| {
            tx.execute(
                "UPDATE nodes SET last_contact = DATETIME('now', '-' || 2 || ' seconds')
                WHERE node_type = ?1",
                [NodeType::Client.sql_variant()],
            )
            .unwrap();
            Ok(())
        })
        .await
        .unwrap();

        super::delete_stale_clients(&app, Duration::from_secs(1)).await;
        assert_eq_db!(app, "SELECT COUNT(*) FROM client_nodes", [], 0);
    }
}
