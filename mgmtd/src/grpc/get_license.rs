use super::*;
use crate::db::config::Config;
use protobuf::license::CertType;
use protobuf::management::{self as pm, GetLicenseResponse};

pub(crate) async fn get_license(
    app: &impl App,
    req: pm::GetLicenseRequest,
) -> Result<pm::GetLicenseResponse> {
    let reload: bool = required_field(req.reload)?;
    if reload {
        let prev_trial_serial: Option<String> = app
            .read_tx(|tx| db::config::get(tx, Config::TrialSerial))
            .await?;

        let serial = app
            .load_and_verify_license_cert(
                &app.static_info().user_config.license_cert_file,
                prev_trial_serial.as_deref(),
            )
            .await?;

        if let Some(d) = app.get_license_cert_data()?.data
            && d.r#type() == CertType::Trial
            && let None = prev_trial_serial
        {
            app.write_tx(|tx| db::config::set(tx, Config::TrialSerial, serial))
                .await?;
        }
    }
    let cert_data = app.get_license_cert_data()?;
    Ok(GetLicenseResponse {
        cert_data: Some(cert_data),
    })
}
