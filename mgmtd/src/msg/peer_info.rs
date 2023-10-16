use super::*;
use shared::msg::peer_info::PeerInfo;

pub(super) async fn handle(_msg: PeerInfo, _ctx: &Context, _req: &impl Request) {
    // This is supposed to give some information about a connection, but it looks
    // like this isnt used at all
}
