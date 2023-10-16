use super::*;
use shared::msg::map_targets::MapTargetsResp;

pub(super) async fn handle(_msg: MapTargetsResp, _ctx: &Context, _req: &impl Request) {
    // This is sent from the nodes as a result of the MapTargets notification after
    // map_targets was called. We just ignore it.
}
