use super::*;

pub(super) async fn handle(
    msg: msg::AuthenticateChannel,
    mut chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    if let Some(ref secret) = hnd.get_static_info().auth_secret {
        if secret == &msg.auth_secret {
            chn.authenticate();
        } else {
            log::error!(
                "Peer {:?} tried to authenticate stream with wrong secret",
                chn.peer()
            );
        }
    } else {
        log::debug!(
            "Peer {:?} tried to authenticate stream, but authentication is not chnuired",
            chn.peer()
        );
    }

    Ok(())
}
