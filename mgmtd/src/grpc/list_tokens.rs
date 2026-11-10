use super::*;
use pm::generate_identity_token_request::Scope;
use pm::list_tokens_response::Token;

pub(crate) async fn list_tokens(
    app: &impl App,
    _req: pm::ListTokensRequest,
) -> Result<pm::ListTokensResponse> {
    fail_on_legacy(app)?;

    let tokens = app
        .read_tx(|tx| {
            Ok(tx.query_map_collect(
                sql!(
                    "SELECT t.token_id, t.scope, t.reusable, t.revoked, t.comment,
                        CAST(STRFTIME('%s', t.created_at) AS INTEGER),
                        CAST(STRFTIME('%s', t.used_at) AS INTEGER),
                        (SELECT COUNT(*) FROM identities AS i WHERE i.token_id = t.token_id)
                    FROM identity_tokens AS t
                    ORDER BY t.token_id"
                ),
                [],
                |row| {
                    let scope = match row.get_ref(1)?.as_str()? {
                        "client" => Scope::Client,
                        "admin" => Scope::Admin,
                        _ => Scope::Unspecified,
                    };

                    Ok(Token {
                        token_id: Some(row.get(0)?),
                        scope: Some(scope.into()),
                        reusable: Some(row.get(2)?),
                        revoked: Some(row.get(3)?),
                        comment: row.get(4)?,
                        created_at_unix: Some(row.get(5)?),
                        used_at_unix: row.get(6)?,
                        identities: Some(row.get(7)?),
                    })
                },
            )?)
        })
        .await?;

    Ok(pm::ListTokensResponse { tokens })
}
