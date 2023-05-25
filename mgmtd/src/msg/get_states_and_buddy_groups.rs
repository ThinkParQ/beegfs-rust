use super::*;
use shared::config::NodeOfflineTimeout;

pub(super) async fn handle(
    msg: msg::GetStatesAndBuddyGroups,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match ci
        .execute_db(move |tx| {
            let targets = db::targets::with_type(tx, msg.node_type)?;
            let groups = db::buddy_groups::with_type(tx, msg.node_type)?;

            Ok((targets, groups))
        })
        .await
    {
        Ok((targets, groups)) => {
            let states: HashMap<_, _> = targets
                .into_iter()
                .map(|e| {
                    (
                        e.target_id,
                        msg::types::CombinedTargetState {
                            reachability: db::misc::calc_reachability_state(
                                e.last_contact,
                                ci.get_config::<NodeOfflineTimeout>(),
                            ),
                            consistency: e.consistency,
                        },
                    )
                })
                .collect();

            rcc.respond(&msg::GetStatesAndBuddyGroupsResp {
                groups: groups
                    .into_iter()
                    .map(|e| {
                        (
                            e.id,
                            msg::types::BuddyGroup {
                                primary_target_id: e.primary_target_id,
                                secondary_target_id: e.secondary_target_id,
                            },
                        )
                    })
                    .collect(),
                states,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Getting states and buddy groups for {} nodes failed:\n{:?}",
                msg.node_type,
                err
            );

            rcc.respond(&msg::GetStatesAndBuddyGroupsResp {
                groups: HashMap::new(),
                states: HashMap::new(),
            })
            .await
        }
    }
}
