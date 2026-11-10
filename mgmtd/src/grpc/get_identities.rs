use super::*;

pub(crate) async fn get_identities(
    app: &impl App,
    _req: pm::GetIdentitiesRequest,
) -> Result<pm::GetIdentitiesResponse> {
    let (version, keys) = app
        .read_tx(|tx| {
            let keys = tx.query_map_collect(
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
            )?;

            Ok((crate::bee_msg::identities_version(tx)?, keys))
        })
        .await?;

    Ok(pm::GetIdentitiesResponse {
        keys,
        version: Some(version),
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use crate::bee_msg;
    use shared::conn::beecrypt::KeyPairId;

    #[tokio::test]
    async fn get_identities() {
        let app = TestApp::new().await;
        let key_a: KeyPairId = format!("sha256:{}", "a".repeat(64)).parse().unwrap();
        let key_b: KeyPairId = format!("sha256:{}", "b".repeat(64)).parse().unwrap();

        let (id_a, id_b) = app
            .write_tx(move |tx| {
                let register = |key_id, name: Option<&str>| {
                    let req = bee_msg::KeyRequest {
                        key_id,
                        category: bee_msg::KeyCategory::Client,
                        token: None,
                        node: None,
                        name: name.map(str::to_string),
                        addr: "127.0.0.1:1".to_string(),
                    };
                    let modes = bee_msg::AdmissionModes::from_config(&Default::default());
                    match bee_msg::admit_key(tx, modes, &req)? {
                        bee_msg::Admission::Registered(id) => Ok(id),
                        other => Err(anyhow!("Not registered: {other:?}")),
                    }
                };
                let id_a = register(key_a, Some("a"))?;
                let id_b = register(key_b, None)?;
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
