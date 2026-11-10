use super::*;

pub(crate) async fn revoke_tokens(
    app: &impl App,
    req: pm::RevokeTokensRequest,
) -> Result<pm::RevokeTokensResponse> {
    fail_on_pre_shutdown(app)?;
    fail_on_legacy(app)?;

    if req.token_ids.is_empty() {
        return Err(anyhow!("No token IDs given")).status_code(Code::InvalidArgument);
    }

    let token_ids = req.token_ids;

    let results = app
        .write_tx(move |tx| {
            let mut results = vec![];

            for token_id in token_ids {
                let token: Option<(bool, bool)> = tx
                    .query_row(
                        sql!(
                            "SELECT reusable, used_at IS NOT NULL FROM identity_tokens
                            WHERE token_id = ?1"
                        ),
                        [token_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;

                let deleted = match token {
                    None => bail!("Identity token {token_id} does not exist"),
                    Some((_, false)) => {
                        tx.execute(
                            sql!("DELETE FROM identity_tokens WHERE token_id = ?1"),
                            [token_id],
                        )?;
                        true
                    }
                    Some((true, true)) => {
                        tx.execute(
                            sql!("UPDATE identity_tokens SET revoked = 1 WHERE token_id = ?1"),
                            [token_id],
                        )?;
                        false
                    }
                    Some((false, true)) => {
                        bail!("Identity token {token_id} has already been used and is spent")
                    }
                };

                results.push((token_id, deleted));
            }

            Ok(results)
        })
        .await
        .status_code(Code::FailedPrecondition)?;

    for (token_id, deleted) in results {
        log::warn!(
            "Identity token {token_id} has been revoked ({})",
            if deleted { "deleted" } else { "kept for audit" }
        );
    }

    Ok(pm::RevokeTokensResponse {})
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use crate::bee_msg::{Admission, AdmissionModes, KeyCategory, KeyRequest, admit_key};
    use crate::config::TokenScope;
    use shared::conn::beecrypt::KeyPairId;

    async fn use_token(app: &TestApp, c: &str, token: &str) -> Admission {
        let key_id: KeyPairId = format!("sha256:{}", c.repeat(64)).parse().unwrap();
        let modes = AdmissionModes::from_config(&app.static_info().user_config);
        let req = KeyRequest {
            key_id,
            category: KeyCategory::Client,
            token: Some(token.to_string()),
            node: None,
            name: None,
            addr: "10.0.0.1:1".to_string(),
        };
        app.write_tx(move |tx| admit_key(tx, modes, &req))
            .await
            .unwrap()
    }

    async fn token(app: &TestApp, reusable: bool) -> (i64, String) {
        let token = app
            .write_tx(move |tx| {
                crate::bee_msg::create_identity_token(tx, TokenScope::Client, reusable, None)
            })
            .await
            .unwrap();
        let id = token.split('.').nth(1).unwrap().parse().unwrap();
        (id, token)
    }

    async fn revoke(app: &TestApp, id: i64) -> Result<pm::RevokeTokensResponse> {
        revoke_tokens(
            app,
            pm::RevokeTokensRequest {
                token_ids: vec![id],
            },
        )
        .await
    }

    async fn remaining(app: &TestApp) -> Vec<(i64, bool)> {
        crate::grpc::list_tokens::list_tokens(app, pm::ListTokensRequest {})
            .await
            .unwrap()
            .tokens
            .into_iter()
            .map(|t| (t.token_id(), t.revoked()))
            .collect()
    }

    #[tokio::test]
    async fn revoke_rules() {
        let app = TestApp::non_legacy().await;

        let (unused, unused_token) = token(&app, false).await;
        revoke(&app, unused).await.unwrap();
        assert!(matches!(
            use_token(&app, "a", &unused_token).await,
            Admission::Rejected(_)
        ));

        let (spent, spent_token) = token(&app, false).await;
        assert!(matches!(
            use_token(&app, "b", &spent_token).await,
            Admission::Registered(_)
        ));
        revoke(&app, spent).await.unwrap_err();

        let (reusable, reusable_token) = token(&app, true).await;
        assert!(matches!(
            use_token(&app, "c", &reusable_token).await,
            Admission::Registered(_)
        ));
        revoke(&app, reusable).await.unwrap();
        assert!(matches!(
            use_token(&app, "d", &reusable_token).await,
            Admission::Rejected(_)
        ));

        assert_eq!(
            remaining(&app).await,
            vec![(spent, false), (reusable, true)]
        );

        let listed = crate::grpc::list_tokens::list_tokens(&app, pm::ListTokensRequest {})
            .await
            .unwrap()
            .tokens;
        assert_eq!(listed[1].identities, Some(1));

        revoke(&app, 999).await.unwrap_err();
    }
}
