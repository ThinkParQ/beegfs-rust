use super::*;
use pm::list_identities_response::Identity;
use std::collections::HashMap;

pub(crate) async fn list_identities(
    app: &impl App,
    _req: pm::ListIdentitiesRequest,
) -> Result<pm::ListIdentitiesResponse> {
    fail_on_legacy(app)?;

    let identities = app
        .read_tx(|tx| {
            let mut keys: HashMap<i64, Vec<String>> = HashMap::new();
            let key_rows: Vec<(i64, String)> = tx.query_map_collect(
                sql!("SELECT identity_id, key FROM keys ORDER BY key_id"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            for (identity_id, key) in key_rows {
                keys.entry(identity_id).or_default().push(key);
            }

            Ok(tx.query_map_collect(
                sql!(
                    "SELECT i.identity_id, i.name, i.admin, i.revoked, i.token_id,
                        n.node_uid, n.node_id, n.node_type, n.alias
                    FROM identities AS i
                    LEFT JOIN identity_to_node AS itn USING (identity_id)
                    LEFT JOIN nodes_ext AS n
                        ON n.node_type = itn.node_type AND n.node_id = itn.node_id
                    ORDER BY i.identity_id"
                ),
                [],
                |row| {
                    let identity_id: i64 = row.get(0)?;
                    let node = match row.get::<_, Option<Uid>>(5)? {
                        Some(uid) => Some(pb::EntityIdSet {
                            uid: Some(uid),
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(6)?,
                                node_type: NodeType::from_row(row, 7)?.into_proto_i32(),
                            }),
                            alias: row.get(8)?,
                        }),
                        None => None,
                    };

                    Ok(Identity {
                        identity_id: Some(identity_id),
                        name: row.get(1)?,
                        admin: Some(row.get(2)?),
                        revoked: Some(row.get(3)?),
                        key_ids: keys.remove(&identity_id).unwrap_or_default(),
                        node,
                        token_id: row.get(4)?,
                    })
                },
            )?)
        })
        .await?;

    Ok(pm::ListIdentitiesResponse { identities })
}
