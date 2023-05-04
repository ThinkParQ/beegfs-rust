use super::*;
use shared::msg::Ack;

pub(super) async fn handle(
    msg: msg::RefreshCapacityPools,
    chn: impl RequestChannel,
    _hnd: impl ComponentHandles,
) -> Result<()> {
    // This message is superfluos and therefore ignored. It is meant to tell the
    // mgmtd to trigger a capacity pool pull immediately after a node starts.
    // meta and storage send a msg::SetTargetInfo before this msg though,
    // so we handle triggering pulls there.

    chn.respond(&Ack { ack_id: msg.ack_id }).await
}
