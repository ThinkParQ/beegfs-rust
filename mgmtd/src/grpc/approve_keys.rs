use super::*;
use shared::conn::beecrypt::KeyPairId;

pub(crate) fn parse_key_ids(key_ids: &[String]) -> Result<Vec<KeyPairId>> {
    if key_ids.is_empty() {
        return Err(anyhow!("No key pair IDs given")).status_code(Code::InvalidArgument);
    }

    key_ids
        .iter()
        .map(|id| {
            id.parse::<KeyPairId>()
                .map_err(|err| anyhow!("Invalid key pair ID {id:?}: {err}"))
                .status_code(Code::InvalidArgument)
        })
        .collect()
}

pub(crate) async fn approve_keys(
    app: &impl App,
    req: pm::ApproveKeysRequest,
) -> Result<pm::ApproveKeysResponse> {
    fail_on_pre_shutdown(app)?;
    fail_on_legacy(app)?;

    let key_ids = parse_key_ids(&req.key_ids)?;

    let (version_before, version) = app
        .write_tx({
            let key_ids = key_ids.clone();
            move |tx| {
                let version_before = crate::bee_msg::identities_version(tx)?;
                for key_id in &key_ids {
                    crate::bee_msg::approve_pending_key(tx, key_id)?;
                }
                Ok((version_before, crate::bee_msg::identities_version(tx)?))
            }
        })
        .await
        .status_code(Code::NotFound)?;

    for key_id in &key_ids {
        log::info!("Approved key pair ID {key_id}");
    }

    if version != version_before {
        crate::bee_msg::notify_identities_changed(app, version).await;
    }

    Ok(pm::ApproveKeysResponse {})
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use crate::bee_msg::{Admission, AdmissionModes, KeyCategory, KeyRequest, admit_key};
    use crate::config::{Config, RegisterServerMode};
    use crate::grpc::get_pending_keys::get_pending_keys;
    use crate::grpc::reject_keys::reject_keys;

    async fn register_storage(app: &TestApp, c: &str) -> (KeyPairId, Admission) {
        let key_id: KeyPairId = format!("sha256:{}", c.repeat(64)).parse().unwrap();
        let modes = AdmissionModes::from_config(&app.static_info().user_config);
        let req = KeyRequest {
            key_id,
            category: KeyCategory::Server,
            token: None,
            node: Some((NodeType::Storage, 0)),
            name: None,
            addr: "10.0.0.2:8003".to_string(),
        };
        let admission = app
            .write_tx(move |tx| admit_key(tx, modes, &req))
            .await
            .unwrap();
        (key_id, admission)
    }

    #[tokio::test]
    async fn approve_and_reject() {
        let app = TestApp::non_legacy_with_config(Config {
            register_server_mode: RegisterServerMode::Approval,
            ..Default::default()
        })
        .await;

        let (key_a, admission) = register_storage(&app, "a").await;
        assert_eq!(admission, Admission::Pending);
        let (key_b, _) = register_storage(&app, "b").await;

        let pending = get_pending_keys(&app, pm::GetPendingKeysRequest {})
            .await
            .unwrap()
            .keys;
        assert_eq!(pending.len(), 2);
        assert_eq!(
            pending[0].node_type,
            Some(NodeType::Storage.into_proto_i32())
        );
        assert_eq!(pending[0].rejected, Some(false));

        approve_keys(
            &app,
            pm::ApproveKeysRequest {
                key_ids: vec![key_a.to_string()],
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            register_storage(&app, "a").await.1,
            Admission::Known(_)
        ));

        reject_keys(
            &app,
            pm::RejectKeysRequest {
                key_ids: vec![key_b.to_string()],
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            register_storage(&app, "b").await.1,
            Admission::Rejected(_)
        ));

        let pending = get_pending_keys(&app, pm::GetPendingKeysRequest {})
            .await
            .unwrap()
            .keys;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].rejected, Some(true));

        for key_ids in [vec![key_a.to_string()], vec!["invalid".to_string()], vec![]] {
            approve_keys(&app, pm::ApproveKeysRequest { key_ids })
                .await
                .unwrap_err();
        }
    }

    #[tokio::test]
    async fn legacy_refused() {
        let app = TestApp::new().await;
        get_pending_keys(&app, pm::GetPendingKeysRequest {})
            .await
            .unwrap_err();
        crate::grpc::generate_identity_token::generate_identity_token(
            &app,
            pm::GenerateIdentityTokenRequest {
                scope: Some(pm::generate_identity_token_request::Scope::Client.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    }

    #[tokio::test]
    async fn generate_token() {
        use crate::grpc::generate_identity_token::generate_identity_token;
        use pm::generate_identity_token_request::Scope;

        let app = TestApp::non_legacy().await;

        let resp = generate_identity_token(
            &app,
            pm::GenerateIdentityTokenRequest {
                scope: Some(Scope::Admin.into()),
                reusable: Some(true),
                comment: Some("test".to_string()),
            },
        )
        .await
        .unwrap();
        assert!(resp.token.unwrap().starts_with("beegfs-idt-1."));

        generate_identity_token(&app, pm::GenerateIdentityTokenRequest::default())
            .await
            .unwrap_err();
    }
}
