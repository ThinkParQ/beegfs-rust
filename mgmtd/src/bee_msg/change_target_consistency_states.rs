use super::*;
use common::update_last_contact_times;
use shared::bee_msg::target::*;

impl HandleWithResponse for ChangeTargetConsistencyStates {
    type Response = ChangeTargetConsistencyStatesResp;

    fn error_response() -> Self::Response {
        ChangeTargetConsistencyStatesResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        // self.old_states is currently completely ignored. If something reports a non-GOOD state, I
        // see no apparent reason to that the old state matches before setting. We have the
        // authority, whatever nodes think their old state was doesn't matter.

        let node_offline_timeout = app.static_info().user_config.node_offline_timeout;
        let target_ids = self.target_ids.clone();
        let (consistencies_changed, reachabilities_changed) = app
            .write_tx(move |tx| {
                let node_type = self.node_type.try_into()?;

                // Check given target Ids exist
                db::target::validate_ids(tx, &target_ids, node_type)?;

                // Old management updates contact time while handling this message (comes usually in
                // every 30 seconds), so we do it as well.
                let reachabilities_changed =
                    update_last_contact_times(tx, &target_ids, node_type, node_offline_timeout)?;

                // ... or if any consistency state changed.
                let consistencies_changed = db::target::update_consistency_states(
                    tx,
                    target_ids.into_iter().zip(self.new_states.iter().copied()),
                    node_type,
                )?;

                Ok((consistencies_changed, reachabilities_changed))
            })
            .await?;

        log::debug!(
            "Updated target states for {:?} targets {:?}, {consistencies_changed} consistency states and {reachabilities_changed} reachability states changed",
            self.node_type,
            self.target_ids,
        );

        // To avoid spamming, we only send out the refresh notification if there is any actual
        // change
        if consistencies_changed > 0 || reachabilities_changed > 0 {
            app.send_notifications(
                &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                &RefreshTargetStates { ack_id: "".into() },
            )
            .await;
        }

        Ok(ChangeTargetConsistencyStatesResp {
            result: OpsErr::SUCCESS,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;

    #[tokio::test]
    async fn change_target_consistency_states() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(ChangeTargetConsistencyStates::ID);

        // Prepare times
        app.db
            .write_tx(|tx| {
                tx.execute("UPDATE targets SET last_update = DATETIME(0)", [])
                    .unwrap();
                tx.execute("UPDATE nodes SET last_contact = DATETIME(0)", [])
                    .unwrap();
                Ok(())
            })
            .await
            .unwrap();

        // No change of consistency states
        let msg = ChangeTargetConsistencyStates {
            node_type: NodeType::Storage,
            target_ids: vec![1, 5],
            old_states: vec![],
            new_states: vec![TargetConsistencyState::Good, TargetConsistencyState::Good],
            ack_id: "".into(),
        };
        let resp = msg.clone().handle(&app, &mut req).await.unwrap();

        assert_eq!(resp.result, OpsErr::SUCCESS);

        // Since the targets were "offline" before, a notification should go out
        assert_eq!(app.sent_notifications::<RefreshTargetStates>(), 1);

        msg.handle(&app, &mut req).await.unwrap();

        // Now the targets were already "online", no additional notification should be sent
        assert_eq!(app.sent_notifications::<RefreshTargetStates>(), 1);

        // Change of consistency states
        let msg = ChangeTargetConsistencyStates {
            node_type: NodeType::Storage,
            target_ids: vec![1, 5],
            old_states: vec![],
            new_states: vec![
                TargetConsistencyState::NeedsResync,
                TargetConsistencyState::Bad,
            ],
            ack_id: "".into(),
        };
        msg.handle(&app, &mut req).await.unwrap();

        // Since consistency states changed, a notification should go out
        assert_eq!(app.sent_notifications::<RefreshTargetStates>(), 2);

        assert_eq_db!(
            app,
            "SELECT COUNT(*) FROM storage_targets WHERE consistency = ?1",
            [TargetConsistencyState::NeedsResync.sql_variant()],
            1
        );
        assert_eq_db!(
            app,
            "SELECT COUNT(*) FROM storage_targets WHERE consistency = ?1",
            [TargetConsistencyState::Bad.sql_variant()],
            1
        );

        // With all that, the node last_contact times of some nodes should also be up to date
        assert_eq_db!(
            app,
            "SELECT COUNT(*) FROM storage_nodes WHERE last_contact > UNIXEPOCH('now') - 30",
            [],
            2
        );
    }
}
