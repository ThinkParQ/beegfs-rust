use super::*;
use crate::types::BuddyGroupQuotaAccounting;

/// Modify the settings of an existing buddy group
pub(crate) async fn modify_buddy_group(
    app: &impl App,
    req: pm::ModifyBuddyGroupRequest,
) -> Result<pm::ModifyBuddyGroupResponse> {
    fail_on_missing_license(app, LicensedFeature::Mirroring)?;
    fail_on_pre_shutdown(app)?;

    let group: EntityId = required_field(req.group)?;
    let options: pm::BuddyGroupOptions = required_field(req.options)?;
    let quota_accounting: Option<BuddyGroupQuotaAccounting> =
        optional_field(options.quota_accounting)?;

    let group = app
        .write_tx(move |tx| {
            let group = group.resolve(tx, EntityType::BuddyGroup)?;

            if quota_accounting.is_some() && group.node_type() != NodeType::Storage {
                bail!("The quota accounting mode can only be set to storage buddy groups");
            }

            tx.execute_cached(
                sql!(
                    "UPDATE buddy_groups
                    SET quota_accounting = COALESCE(?1, quota_accounting)
                    WHERE group_uid = ?2"
                ),
                params![quota_accounting.map(|e| e.sql_variant()), group.uid],
            )?;

            Ok(group)
        })
        .await?;

    log::info!("Buddy group {group} modified: quota_accounting={quota_accounting:?}");

    Ok(pm::ModifyBuddyGroupResponse {})
}
