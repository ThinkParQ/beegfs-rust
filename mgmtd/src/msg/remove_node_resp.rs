use super::*;

pub(super) async fn handle(
    _msg: msg::RemoveNodeResp,
    rcc: impl RequestConnectionController,
    _ci: impl ComponentInteractor,
) -> Result<()> {
    // response from server nodes to the RemoveNode notification
    log::debug!("Ignoring RemoveNodeResp msg from {:?}", rcc.peer());
    Ok(())
}
