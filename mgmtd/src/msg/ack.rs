use super::*;

pub(super) async fn handle(
    msg: msg::Ack,
    rcc: impl RequestConnectionController,
    _ci: impl ComponentInteractor,
) -> Result<()> {
    log::debug!("Ignoring Ack from {:?}: ID: {:?}", rcc.peer(), msg.ack_id);

    Ok(())
}
