use super::*;
use shared::bee_msg::misc::*;

impl HandleNoResponse for AuthenticateChannel {
    async fn handle(self, ctx: &Context, req: &mut impl Request) -> Result<()> {
        if let Some(ref secret) = ctx.info.auth_secret {
            if secret == &self.auth_secret {
                req.authenticate_connection();
            } else {
                log::error!(
                    "Peer {:?} tried to authenticate stream with wrong secret",
                    req.addr()
                );
            }
        } else {
            log::debug!(
                "Peer {:?} tried to authenticate stream, but authentication is not required",
                req.addr()
            );
        }

        Ok(())
    }
}
