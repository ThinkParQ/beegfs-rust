use super::*;

pub(super) async fn handle(
    _msg: msg::PeerInfo,
    _chn: impl RequestChannel,
    _hnd: impl ComponentHandles,
) -> Result<()> {
    // This is supposed to give some information about a connection, but it looks
    // like this isnt used at all
    Ok(())
}
