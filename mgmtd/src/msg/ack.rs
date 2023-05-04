use super::*;

pub(super) async fn handle(
    msg: msg::Ack,
    chn: impl RequestChannel,
    _hnd: impl ComponentHandles,
) -> Result<()> {
    log::debug!("Ignoring Ack from {:?}: ID: {:?}", chn.peer(), msg.ack_id);

    Ok(())
}
