use super::*;
use shared::msg::remove_node::RemoveNodeResp;

pub(super) async fn handle(_msg: RemoveNodeResp, _ctx: &Context, req: &impl Request) {
    // response from server nodes to the RemoveNode notification
    log::debug!("Ignoring RemoveNodeResp msg from {:?}", req.addr());
}
