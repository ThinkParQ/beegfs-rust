use super::*;

pub(crate) async fn get_identities(
    app: &impl App,
    _req: pm::GetIdentitiesRequest,
) -> Result<pm::GetIdentitiesResponse> {
    let keys = app
        .read_tx(|tx| {
            Ok(tx.query_map_collect(
                sql!(
                    "SELECT k.key, k.identity_id, n.node_uid, n.node_id, n.node_type, n.alias
                    FROM keys AS k
                    LEFT JOIN identity_to_node AS itn USING (identity_id)
                    LEFT JOIN nodes_ext AS n
                        ON n.node_type = itn.node_type AND n.node_id = itn.node_id
                    ORDER BY k.key_id"
                ),
                [],
                |row| {
                    let node = match row.get::<_, Option<Uid>>(2)? {
                        Some(uid) => Some(pb::EntityIdSet {
                            uid: Some(uid),
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(3)?,
                                node_type: NodeType::from_row(row, 4)?.into_proto_i32(),
                            }),
                            alias: row.get(5)?,
                        }),
                        None => None,
                    };

                    Ok(pm::get_identities_response::IdentityKey {
                        key_id: Some(row.get(0)?),
                        identity_id: Some(row.get(1)?),
                        node,
                    })
                },
            )?)
        })
        .await?;

    Ok(pm::GetIdentitiesResponse { keys })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use shared::conn::beecrypt::KeyPairId;

    #[tokio::test]
    async fn get_identities() {
        let app = TestApp::new().await;
        let key_a: KeyPairId = format!("sha256:{}", "a".repeat(64)).parse().unwrap();
        let key_b: KeyPairId = format!("sha256:{}", "b".repeat(64)).parse().unwrap();

        let (id_a, id_b) = app
            .write_tx(move |tx| {
                let id_a = crate::bee_msg::register_key(tx, Some("a".into()), &key_a)?;
                let id_b = crate::bee_msg::register_key(tx, None, &key_b)?;
                tx.execute(
                    sql!(
                        "INSERT INTO identity_to_node (identity_id, node_type, node_id)
                        VALUES (?1, ?2, 1)"
                    ),
                    params![id_a, NodeType::Meta.sql_variant()],
                )?;
                Ok((id_a, id_b))
            })
            .await
            .unwrap();

        let resp = super::get_identities(&app, pm::GetIdentitiesRequest {})
            .await
            .unwrap();

        assert_eq!(resp.keys.len(), 2);
        assert_eq!(resp.keys[0].key_id(), key_a.to_string());
        assert_eq!(resp.keys[0].identity_id(), id_a);
        let node = resp.keys[0].node.as_ref().unwrap();
        assert_eq!(node.uid(), 101001);
        assert_eq!(node.alias(), "meta_node_1");
        assert_eq!(node.legacy_id.unwrap().num_id, 1);

        assert_eq!(resp.keys[1].key_id(), key_b.to_string());
        assert_eq!(resp.keys[1].identity_id(), id_b);
        assert!(resp.keys[1].node.is_none());
    }
}
