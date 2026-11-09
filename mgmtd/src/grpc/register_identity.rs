use super::*;
use conn::ConnInfo;

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

    let identity_id = app
        .write_tx({
            let name = name.clone();
            move |tx| crate::bee_msg::register_key(tx, name, &key_id)
        })
        .await?;

    *info.identity.lock().unwrap() = PeerIdentity::Registered {
        key_id,
        identity_id,
    };

    log::info!(
        "Registered key pair ID {key_id} for identity {name:?} (ID {identity_id}) from {}",
        info.addr
    );

    Ok(pm::RegisterIdentityResponse {
        identity_id: Some(identity_id),
    })
}
