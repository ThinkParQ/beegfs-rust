use super::*;
use common::get_targets_with_states;
use shared::bee_msg::target::*;

impl HandleWithResponse for GetTargetStates {
    type Response = GetTargetStatesResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        let pre_shutdown = app.is_pre_shutdown();
        let node_offline_timeout = app.static_info().user_config.node_offline_timeout;

        let targets = app
            .read_tx(move |tx| {
                get_targets_with_states(
                    tx,
                    pre_shutdown,
                    self.node_type.try_into()?,
                    node_offline_timeout,
                )
            })
            .await?;

        let mut target_ids = Vec::with_capacity(targets.len());
        let mut reachability_states = Vec::with_capacity(targets.len());
        let mut consistency_states = Vec::with_capacity(targets.len());

        for e in targets {
            target_ids.push(e.0);
            consistency_states.push(e.1);
            reachability_states.push(e.2);
        }

        let resp = GetTargetStatesResp {
            targets: target_ids,
            consistency_states,
            reachability_states,
        };

        Ok(resp)
    }
}
