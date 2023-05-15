use super::*;

pub(super) async fn handle(
    _msg: msg::SetMirrorBuddyGroupResp,
    _rcc: impl RequestConnectionController,
    _ci: impl ComponentInteractor,
) -> Result<()> {
    Ok(())
}
