use super::*;
use crate::bee_msg;
use conn::ConnInfo;
use pm::register_identity_response::Status;

pub(crate) async fn register_identity(
    app: &impl App,
    info: Option<ConnInfo>,
    req: pm::RegisterIdentityRequest,
) -> Result<pm::RegisterIdentityResponse> {
    fail_on_pre_shutdown(app)?;

    let Some(info) = info else {
        return Err(anyhow!(
            "Identities can only be registered with a non-legacy connection protocol"
        ))
        .status_code(Code::FailedPrecondition);
    };

    let current = *info.identity.lock().unwrap();
    let key_id = match current {
        PeerIdentity::Unregistered(key_id) | PeerIdentity::Registered { key_id, .. } => key_id,
        PeerIdentity::Legacy => {
            return Err(anyhow!("Connection has no key pair ID"))
                .status_code(Code::FailedPrecondition);
        }
    };

    let name = req.name.filter(|name| !name.is_empty());
    let key_req = bee_msg::KeyRequest {
        key_id,
        category: bee_msg::KeyCategory::Client,
        token: req.identity_token.filter(|t| !t.is_empty()),
        node: None,
        name: name.clone(),
        addr: info.addr.to_string(),
    };
    let modes = bee_msg::AdmissionModes::from_config(&app.static_info().user_config);

    let (admission, admin, version_before, version) = app
        .write_tx(move |tx| {
            let version_before = bee_msg::identities_version(tx)?;
            let admission = bee_msg::admit_key(tx, modes, &key_req)?;

            let admin = match admission {
                bee_msg::Admission::Registered(identity_id) => tx.query_row(
                    sql!("SELECT admin FROM identities WHERE identity_id = ?1"),
                    [identity_id],
                    |row| row.get(0),
                )?,
                _ => false,
            };

            Ok((
                admission,
                admin,
                version_before,
                bee_msg::identities_version(tx)?,
            ))
        })
        .await?;

    let identity_id = match admission {
        bee_msg::Admission::Registered(identity_id) => identity_id,
        bee_msg::Admission::Pending => {
            log::info!(
                "Key pair ID {key_id} from {} is pending approval",
                info.addr
            );
            return Ok(pm::RegisterIdentityResponse {
                identity_id: None,
                status: Some(Status::Pending.into()),
                message: Some(format!(
                    "Key pair ID {key_id} is pending approval by an admin"
                )),
            });
        }
        bee_msg::Admission::Rejected(reason) => {
            log::warn!("Rejected key pair ID {key_id} from {}: {reason}", info.addr);
            return Ok(pm::RegisterIdentityResponse {
                identity_id: None,
                status: Some(Status::Rejected.into()),
                message: Some(reason),
            });
        }
    };

    *info.identity.lock().unwrap() = PeerIdentity::Registered {
        key_id,
        identity_id,
    };
    info.admin.store(admin, Ordering::Relaxed);

    log::info!(
        "Registered key pair ID {key_id} for identity {name:?} (ID {identity_id}) from {}",
        info.addr
    );

    if version != version_before {
        bee_msg::notify_identities_changed(app, version).await;
    }

    Ok(pm::RegisterIdentityResponse {
        identity_id: Some(identity_id),
        status: Some(Status::Ok.into()),
        message: None,
    })
}
