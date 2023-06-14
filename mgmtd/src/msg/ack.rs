use super::*;

pub(super) async fn handle(
    msg: msg::Ack,
    _ci: impl ComponentInteractor,
    rcc: &impl RequestConnectionController,
) {
    log::debug!("Ignoring Ack from {:?}: ID: {:?}", rcc.peer(), msg.ack_id);
}
