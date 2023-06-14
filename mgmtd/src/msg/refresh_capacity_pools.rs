use super::*;

pub(super) async fn handle(
    msg: msg::RefreshCapacityPools,
    _ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::Ack {
    // This message is superfluos and therefore ignored. It is meant to tell the
    // mgmtd to trigger a capacity pool pull immediately after a node starts.
    // meta and storage send a msg::SetTargetInfo before this msg though,
    // so we handle triggering pulls there.

    msg::Ack { ack_id: msg.ack_id }
}
