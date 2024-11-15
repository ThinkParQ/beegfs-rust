use super::*;
use protobuf::management::{self as pm, GetLicenseResponse};

pub(crate) async fn get(
    app: &impl AppExt,
    req: pm::GetLicenseRequest,
) -> Result<pm::GetLicenseResponse> {
    let reload: bool = required_field(req.reload)?;
    if reload {
        app.load_and_verify_cert(&app.static_info().user_config.license_cert_file)
            .await?;
    }
    let cert_data = app.get_cert_data()?;
    Ok(GetLicenseResponse {
        cert_data: Some(cert_data),
    })
}
