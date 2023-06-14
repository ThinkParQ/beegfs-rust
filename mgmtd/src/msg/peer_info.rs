use super::*;

pub(super) async fn handle(
    _msg: msg::PeerInfo,
    _ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) {
    // This is supposed to give some information about a connection, but it looks
    // like this isnt used at all
}
