use super::*;

pub(super) async fn handle(_msg: msg::RemoveNodeResp, _ctx: &impl AppContext, req: &impl Request) {
    // response from server nodes to the RemoveNode notification
    log::debug!("Ignoring RemoveNodeResp msg from {:?}", req.addr());
}
