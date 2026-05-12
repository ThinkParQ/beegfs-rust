use super::*;
use crate::db::config::Config;
use protobuf::management::{self as pm, GetLicenseResponse};
use protobuf::license::CertType;

pub(crate) async fn get_license(
    app: &impl App,
    req: pm::GetLicenseRequest,
) -> Result<pm::GetLicenseResponse> {
    let reload: bool = required_field(req.reload)?;
    if reload {
        let prev_trial_serial: Option<String> = app.read_tx(|tx| {
            db::config::get(tx, Config::TrialSerial)
        })
        .await?;

        let serial = app.load_and_verify_license_cert(&app.static_info().user_config.license_cert_file,
            prev_trial_serial).await?;

        if app.get_license_cert_data()?.data.is_some_and(|d| d.r#type == CertType::Trial.into())
        {
            app.write_tx(|tx| db::config::set(tx, Config::TrialSerial, serial)).await?;
        }

    }
    let cert_data = app.get_license_cert_data()?;
    Ok(GetLicenseResponse {
        cert_data: Some(cert_data),
    })
}
