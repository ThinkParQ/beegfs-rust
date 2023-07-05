use super::*;

pub(super) async fn handle(
    msg: msg::AuthenticateChannel,
    ctx: &impl AppContext,
    req: &mut impl Request,
) {
    if let Some(ref secret) = ctx.get_static_info().auth_secret {
        if secret == &msg.auth_secret {
            req.authenticate_connection();
        } else {
            log::error!(
                "Peer {:?} tried to authenticate stream with wrong secret",
                req.peer_id()
            );
        }
    } else {
        log::debug!(
            "Peer {:?} tried to authenticate stream, but authentication is not required",
            req.peer_id()
        );
    }
}
