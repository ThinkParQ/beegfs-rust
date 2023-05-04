use super::*;

pub(super) async fn handle(
    _msg: msg::MapTargetsResp,
    _chn: impl RequestChannel,
    _hnd: impl ComponentHandles,
) -> Result<()> {
    // This is sent from the nodes as a result of the MapTargets notification after
    // map_targets was called. We just ignore it.
    Ok(())
}
