use super::*;

pub(super) async fn handle(
    _msg: msg::RemoveNodeResp,
    chn: impl RequestChannel,
    _hnd: impl ComponentHandles,
) -> Result<()> {
    // response from server nodes to the RemoveNode notification
    log::debug!("Ignoring RemoveNodeResp msg from {:?}", chn.peer());
    Ok(())
}
