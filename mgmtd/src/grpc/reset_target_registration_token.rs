use super::*;

pub(crate) async fn reset_target_registration_token(
    app: &impl App,
    req: pm::ResetTargetRegistrationTokenRequest,
) -> Result<pm::ResetTargetRegistrationTokenResponse> {
    fail_on_pre_shutdown(app)?;

    let target: EntityId = required_field(req.target)?;

    let target = app
        .write_tx(move |tx| {
            let target = target.resolve(tx, EntityType::Target)?;

            tx.execute(
                sql!("UPDATE targets SET reg_token = NULL WHERE target_uid = ?1"),
                [target.uid],
            )?;

            Ok(target)
        })
        .await?;

    log::info!("Registration token reset for target {target}");

    Ok(pm::ResetTargetRegistrationTokenResponse {
        target: Some(target.into()),
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;

    #[tokio::test]
    async fn reset_target_registration_token() {
        let app = TestApp::new().await;

        app.db
            .write_tx(|tx| {
                Ok(tx.execute(
                    "UPDATE targets SET reg_token = 'abcd' WHERE target_uid = 201001",
                    [],
                )?)
            })
            .await
            .unwrap();

        super::reset_target_registration_token(
            &app,
            pm::ResetTargetRegistrationTokenRequest {
                target: Some(EntityId::Uid(201001).into()),
            },
        )
        .await
        .unwrap();

        assert_eq_db!(
            app,
            "SELECT COUNT(*) FROM targets WHERE target_uid = 201001 AND reg_token IS NOT NULL",
            [],
            0
        );

        super::reset_target_registration_token(
            &app,
            pm::ResetTargetRegistrationTokenRequest {
                target: Some(EntityId::Uid(201002).into()),
            },
        )
        .await
        .unwrap();
    }
}
