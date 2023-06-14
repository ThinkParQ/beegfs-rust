use super::*;

pub(super) async fn handle(
    _msg: msg::RemoveNodeResp,
    _ci: impl ComponentInteractor,
    rcc: &impl RequestConnectionController,
) {
    // response from server nodes to the RemoveNode notification
    log::debug!("Ignoring RemoveNodeResp msg from {:?}", rcc.peer());
}
