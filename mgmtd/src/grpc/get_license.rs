use super::*;
use protobuf::management::{self as pm, GetLicenseResponse};

pub(crate) async fn get_license(
    ctx: Context,
    req: pm::GetLicenseRequest,
) -> Result<pm::GetLicenseResponse> {
    let reload: bool = required_field(req.reload)?;
    if reload {
        ctx.license
            .load_and_verify_cert(&ctx.info.user_config.license_cert_file)
            .await?;
    }
    let cert_data = ctx.license.get_cert_data()?;
    Ok(GetLicenseResponse {
        cert_data: Some(cert_data),
    })
}
