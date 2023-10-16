use super::*;
use shared::msg::ack::Ack;
use shared::msg::refresh_capacity_pools::RefreshCapacityPools;

pub(super) async fn handle(msg: RefreshCapacityPools, _ctx: &Context, _req: &impl Request) -> Ack {
    // This message is superfluos and therefore ignored. It is meant to tell the
    // mgmtd to trigger a capacity pool pull immediately after a node starts.
    // meta and storage send a SetTargetInfo before this msg though,
    // so we handle triggering pulls there.

    Ack { ack_id: msg.ack_id }
}
