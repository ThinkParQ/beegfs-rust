use super::*;
use shared::bee_msg::misc::*;

impl HandleNoResponse for AuthenticateChannel {
    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<()> {
        if let Some(ref secret) = app.static_info().auth_secret {
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;

    #[tokio::test]
    async fn authenticate_channel() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(AuthenticateChannel::ID);

        AuthenticateChannel {
            auth_secret: AuthSecret::hash_from_bytes("secret"),
        }
        .handle(&app, &mut req)
        .await
        .unwrap();

        assert!(req.authenticate_connection);
    }
}
