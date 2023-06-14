use super::*;

pub(super) async fn handle(
    msg: msg::AuthenticateChannel,
    ci: impl ComponentInteractor,
    rcc: &mut impl RequestConnectionController,
) {
    if let Some(ref secret) = ci.get_static_info().auth_secret {
        if secret == &msg.auth_secret {
            rcc.authenticate();
        } else {
            log::error!(
                "Peer {:?} tried to authenticate stream with wrong secret",
                rcc.peer()
            );
        }
    } else {
        log::debug!(
            "Peer {:?} tried to authenticate stream, but authentication is not required",
            rcc.peer()
        );
    }
}
