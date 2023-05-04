use super::*;
use crate::logic;
use shared::config::NodeOfflineTimeout;

pub(super) async fn handle(
    msg: msg::GetTargetStates,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| db::targets::with_type(tx, msg.node_type))
        .await
    {
        Ok(res) => {
            let mut targets = Vec::with_capacity(res.len());
            let mut reachability_states = Vec::with_capacity(res.len());
            let mut consistency_states = Vec::with_capacity(res.len());

            for e in res {
                targets.push(e.target_id);
                reachability_states.push(logic::calc_reachability_state(
                    e.last_contact,
                    hnd.get_config::<NodeOfflineTimeout>(),
                ));
                consistency_states.push(e.consistency);
            }

            chn.respond(&msg::GetTargetStatesResp {
                targets,
                reachability_states,
                consistency_states,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Getting target states for {} nodes failed:\n{:?}",
                msg.node_type,
                err
            );
            chn.respond(&msg::GetTargetStatesResp {
                targets: vec![],
                reachability_states: vec![],
                consistency_states: vec![],
            })
            .await
        }
    }
}
