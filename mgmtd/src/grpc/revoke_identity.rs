use super::*;
use shared::conn::beecrypt::KeyPairId;

pub(crate) async fn revoke_identity(
    app: &impl App,
    req: pm::RevokeIdentityRequest,
) -> Result<pm::RevokeIdentityResponse> {
    fail_on_pre_shutdown(app)?;
    fail_on_legacy(app)?;

    let given = [
        req.key_id.is_some(),
        req.identity_id.is_some(),
        req.node.is_some(),
    ];
    if given.iter().filter(|g| **g).count() != 1 {
        return Err(anyhow!(
            "Exactly one of key pair ID, identity ID and node must be given"
        ))
        .status_code(Code::InvalidArgument);
    }

    let key_id = req
        .key_id
        .map(|k| k.parse::<KeyPairId>())
        .transpose()
        .map_err(|err| anyhow!("Invalid key pair ID: {err}"))
        .status_code(Code::InvalidArgument)?;
    let node: Option<EntityId> = req.node.map(TryInto::try_into).transpose()?;
    let revoke = !req.unrevoke.unwrap_or(false);

    let (identity_id, version_before, version) = app
        .write_tx(move |tx| {
            let version_before = crate::bee_msg::identities_version(tx)?;

            let identity_id: Option<i64> = if let Some(key_id) = key_id {
                tx.query_row(
                    sql!("SELECT identity_id FROM keys WHERE key = ?1"),
                    [key_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?
            } else if let Some(node) = node {
                let node = node.resolve(tx, EntityType::Node)?;
                tx.query_row(
                    sql!(
                        "SELECT identity_id FROM identity_to_node
                        WHERE node_type = ?1 AND node_id = ?2"
                    ),
                    params![node.node_type().sql_variant(), node.num_id()],
                    |row| row.get(0),
                )
                .optional()?
            } else {
                tx.query_row(
                    sql!("SELECT identity_id FROM identities WHERE identity_id = ?1"),
                    [req.identity_id],
                    |row| row.get(0),
                )
                .optional()?
            };

            let Some(identity_id) = identity_id else {
                bail!("No matching identity found");
            };

            tx.execute(
                sql!("UPDATE identities SET revoked = ?1 WHERE identity_id = ?2"),
                params![revoke, identity_id],
            )?;

            if revoke {
                tx.execute(
                    sql!("DELETE FROM identity_to_node WHERE identity_id = ?1"),
                    [identity_id],
                )?;
            }

            Ok((
                identity_id,
                version_before,
                crate::bee_msg::identities_version(tx)?,
            ))
        })
        .await
        .status_code(Code::NotFound)?;

    log::warn!(
        "Identity {identity_id} has been {}",
        if revoke { "revoked" } else { "unrevoked" }
    );

    if version != version_before {
        crate::bee_msg::notify_identities_changed(app, version).await;
    }

    Ok(pm::RevokeIdentityResponse {
        identity_id: Some(identity_id),
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use crate::bee_msg::{Admission, AdmissionModes, KeyCategory, KeyRequest, admit_key};

    async fn admit(app: &TestApp, c: &str) -> Admission {
        let key_id: KeyPairId = format!("sha256:{}", c.repeat(64)).parse().unwrap();
        let modes = AdmissionModes::from_config(&app.static_info().user_config);
        let req = KeyRequest {
            key_id,
            category: KeyCategory::Client,
            token: None,
            node: None,
            name: None,
            addr: "10.0.0.1:1".to_string(),
        };
        app.write_tx(move |tx| admit_key(tx, modes, &req))
            .await
            .unwrap()
    }

    async fn visible_keys(app: &TestApp) -> usize {
        crate::grpc::get_identities::get_identities(app, pm::GetIdentitiesRequest {})
            .await
            .unwrap()
            .keys
            .len()
    }

    #[tokio::test]
    async fn revoke_and_unrevoke() {
        let app = TestApp::non_legacy().await;

        let Admission::Registered(a) = admit(&app, "a").await else {
            panic!("not registered");
        };
        let Admission::Registered(b) = admit(&app, "b").await else {
            panic!("not registered");
        };
        app.write_tx(move |tx| {
            tx.execute(
                sql!(
                    "INSERT INTO identity_to_node (identity_id, node_type, node_id)
                    VALUES (?1, ?2, 1)"
                ),
                params![b, NodeType::Meta.sql_variant()],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(visible_keys(&app).await, 2);

        let resp = revoke_identity(
            &app,
            pm::RevokeIdentityRequest {
                key_id: Some(format!("sha256:{}", "a".repeat(64))),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(resp.identity_id, Some(a));
        assert!(matches!(admit(&app, "a").await, Admission::Rejected(_)));
        assert_eq!(visible_keys(&app).await, 1);

        let resp = revoke_identity(
            &app,
            pm::RevokeIdentityRequest {
                node: Some(pb::EntityIdSet {
                    alias: Some("meta_node_1".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(resp.identity_id, Some(b));
        assert_eq!(visible_keys(&app).await, 0);
        let mapped: i64 = app
            .read_tx(|tx| {
                Ok(
                    tx.query_row(sql!("SELECT COUNT(*) FROM identity_to_node"), [], |row| {
                        row.get(0)
                    })?,
                )
            })
            .await
            .unwrap();
        assert_eq!(mapped, 0);

        revoke_identity(
            &app,
            pm::RevokeIdentityRequest {
                identity_id: Some(a),
                unrevoke: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(admit(&app, "a").await, Admission::Known(a));
        assert_eq!(visible_keys(&app).await, 1);

        for req in [
            pm::RevokeIdentityRequest::default(),
            pm::RevokeIdentityRequest {
                identity_id: Some(999),
                ..Default::default()
            },
            pm::RevokeIdentityRequest {
                identity_id: Some(a),
                key_id: Some(format!("sha256:{}", "a".repeat(64))),
                ..Default::default()
            },
        ] {
            revoke_identity(&app, req).await.unwrap_err();
        }
    }
}
