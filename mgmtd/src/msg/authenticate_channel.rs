use super::*;

pub(super) async fn handle(
    msg: msg::AuthenticateChannel,
    mut rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
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
            "Peer {:?} tried to authenticate stream, but authentication is not rccuired",
            rcc.peer()
        );
    }

    Ok(())
}
