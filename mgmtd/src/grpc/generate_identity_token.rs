use super::*;
use crate::config::TokenScope;
use pm::generate_identity_token_request::Scope;

pub(crate) async fn generate_identity_token(
    app: &impl App,
    req: pm::GenerateIdentityTokenRequest,
) -> Result<pm::GenerateIdentityTokenResponse> {
    fail_on_pre_shutdown(app)?;
    fail_on_legacy(app)?;

    let scope = match req.scope() {
        Scope::Client => TokenScope::Client,
        Scope::Admin => TokenScope::Admin,
        Scope::Unspecified => {
            return Err(anyhow!("Token scope must be client or admin"))
                .status_code(Code::InvalidArgument);
        }
    };
    let reusable = req.reusable.unwrap_or(false);
    let comment = req.comment.filter(|c| !c.is_empty());

    let token = app
        .write_tx(move |tx| {
            crate::bee_msg::create_identity_token(tx, scope, reusable, comment.as_deref())
        })
        .await?;

    log::info!(
        "Created a {} {scope:?} identity token",
        if reusable { "reusable" } else { "one-time" }
    );

    Ok(pm::GenerateIdentityTokenResponse { token: Some(token) })
}
