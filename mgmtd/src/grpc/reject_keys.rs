use super::*;

pub(crate) async fn reject_keys(
    app: &impl App,
    req: pm::RejectKeysRequest,
) -> Result<pm::RejectKeysResponse> {
    fail_on_pre_shutdown(app)?;
    fail_on_legacy(app)?;

    let key_ids = approve_keys::parse_key_ids(&req.key_ids)?;

    app.write_tx({
        let key_ids = key_ids.clone();
        move |tx| {
            for key_id in &key_ids {
                crate::bee_msg::reject_pending_key(tx, key_id)?;
            }
            Ok(())
        }
    })
    .await
    .status_code(Code::NotFound)?;

    for key_id in &key_ids {
        log::info!("Rejected key pair ID {key_id}");
    }

    Ok(pm::RejectKeysResponse {})
}
