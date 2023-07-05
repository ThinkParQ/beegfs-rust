use super::*;

pub(super) async fn handle(_msg: msg::MapTargetsResp, _ctx: &impl AppContext, _req: &impl Request) {
    // This is sent from the nodes as a result of the MapTargets notification after
    // map_targets was called. We just ignore it.
}
