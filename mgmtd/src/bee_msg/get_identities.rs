use super::*;
use shared::bee_msg::misc::*;
use shared::bee_serde::BeeSerdeConversion;

impl HandleWithResponse for GetIdentities {
    type Response = GetIdentitiesResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        let (version, keys) = app
            .read_tx(|tx| {
                let keys = tx.query_map_collect(
                    sql!(
                        "SELECT k.key, itn.node_type, itn.node_id FROM keys AS k
                        LEFT JOIN identity_to_node AS itn USING (identity_id)
                        ORDER BY k.key_id"
                    ),
                    [],
                    |row| {
                        let node_type = match row.get_ref(1)?.as_i64_or_null()? {
                            Some(_) => NodeType::from_row(row, 1)?.into_bee_serde(),
                            None => 0,
                        };

                        Ok(IdentityKey {
                            key_id: row.get::<_, String>(0)?.into_bytes(),
                            node_type,
                            node_id: row.get::<_, Option<NodeId>>(2)?.unwrap_or_default(),
                        })
                    },
                )?;

                Ok((identities_version(tx)?, keys))
            })
            .await?;

        Ok(GetIdentitiesResp { version, keys })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use shared::bee_msg::Header;
    use shared::conn::PeerIdentity;
    use shared::conn::beecrypt::KeyPairId;

    #[tokio::test]
    async fn get_identities() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(Header::default());

        let resp = GetIdentities {}.handle(&app, &mut req).await.unwrap();
        assert!(resp.keys.is_empty());

        let key_id: KeyPairId = format!("sha256:{}", "a".repeat(64)).parse().unwrap();
        req.identity = PeerIdentity::Unregistered(key_id);
        RegisterIdentity {
            name: b"id1".to_vec(),
            key_id: key_id.to_string().into_bytes(),
        }
        .handle(&app, &mut req)
        .await
        .unwrap();

        let resp = GetIdentities {}.handle(&app, &mut req).await.unwrap();
        assert_eq!(
            resp.keys,
            vec![IdentityKey {
                key_id: key_id.to_string().into_bytes(),
                node_type: 0,
                node_id: 0,
            }]
        );
    }
}
