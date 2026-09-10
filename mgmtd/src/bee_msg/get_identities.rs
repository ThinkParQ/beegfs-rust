use super::*;
use shared::bee_msg::node::*;

impl HandleWithResponse for GetIdentities {
    type Response = GetIdentitiesResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        let identities = app
            .read_tx(move |tx| {
                let identities = tx.query_map_collect(
                    sql!(
                        "SELECT name, node_type, node_id, key FROM identities
                        INNER JOIN identity_to_node USING (identity_id)
                        INNER JOIN keys USING (identity_id)"
                    ),
                    [],
                    |row| {
                        Ok(Identity {
                            name: row.get_ref(0)?.as_bytes()?.to_owned(),
                            identity_type: 1,
                            node_type: NodeType::from_row(row, 1)?,
                            node_id: row.get(2)?,
                            public_key: row.get_ref(3)?.as_str()?.parse().map_err(
                                |err: anyhow::Error| {
                                    rusqlite::Error::FromSqlConversionFailure(
                                        3,
                                        rusqlite::types::Type::Text,
                                        err.into(),
                                    )
                                },
                            )?,
                        })
                    },
                )?;

                Ok(identities)
            })
            .await?;

        let resp = GetIdentitiesResp { identities };

        Ok(resp)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use shared::bee_msg::Header;

    #[tokio::test]
    async fn get_identities() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(Header::default());

        let resp = GetIdentities {}.handle(&app, &mut req).await.unwrap();

        assert_eq_db!(
            app,
            "SELECT COUNT(*) FROM identities",
            [],
            resp.identities.len()
        );
    }
}
