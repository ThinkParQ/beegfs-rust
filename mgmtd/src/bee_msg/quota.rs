use super::*;
use crate::db::quota_limit::SpaceAndInodeLimits;
use crate::db::quota_usage::PoolOrTargetId;
use shared::bee_msg::quota::*;
use shared::types::QuotaType;

impl Handler for GetDefaultQuota {
    type Response = GetDefaultQuotaResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                // Check pool ID exists
                let _ =
                    resolve_num_id(tx, EntityType::Pool, NodeType::Storage, self.pool_id.into())?;

                let res = db::quota_default_limit::get_with_pool_id(tx, self.pool_id)?;

                Ok(res)
            })
            .await;

        match res {
            Ok(res) => GetDefaultQuotaResp {
                limits: QuotaDefaultLimits {
                    user_space_limit: res.user_space_limit.unwrap_or_default(),
                    user_inode_limit: res.user_inodes_limit.unwrap_or_default(),
                    group_space_limit: res.group_space_limit.unwrap_or_default(),
                    group_inode_limit: res.group_inodes_limit.unwrap_or_default(),
                },
            },
            Err(err) => {
                log_error_chain!(
                    err,
                    "Getting default quota info for storage pool {} failed",
                    self.pool_id
                );

                GetDefaultQuotaResp {
                    limits: QuotaDefaultLimits::default(),
                }
            }
        }
    }
}

impl Handler for SetDefaultQuota {
    type Response = SetDefaultQuotaResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                // Check pool ID exists
                let _ =
                    resolve_num_id(tx, EntityType::Pool, NodeType::Storage, self.pool_id.into())?;

                match self.space {
                    0 => db::quota_default_limit::delete(
                        tx,
                        self.pool_id,
                        self.id_type,
                        QuotaType::Space,
                    )?,
                    n => db::quota_default_limit::upsert(
                        tx,
                        self.pool_id,
                        self.id_type,
                        QuotaType::Space,
                        n,
                    )?,
                };

                match self.inodes {
                    0 => db::quota_default_limit::delete(
                        tx,
                        self.pool_id,
                        self.id_type,
                        QuotaType::Inode,
                    )?,
                    n => db::quota_default_limit::upsert(
                        tx,
                        self.pool_id,
                        self.id_type,
                        QuotaType::Inode,
                        n,
                    )?,
                };

                Ok(())
            })
            .await;

        match res {
            Ok(_) => {
                log::info!(
                    "Set default quota of type {:?} for storage pool {}",
                    self.id_type,
                    self.pool_id,
                );
                SetDefaultQuotaResp { result: 1 }
            }

            Err(err) => {
                log_error_chain!(
                    err,
                    "Setting default quota of type {:?} for storage pool {} failed",
                    self.id_type,
                    self.pool_id
                );
                SetDefaultQuotaResp { result: 0 }
            }
        }
    }
}

impl Handler for GetQuotaInfo {
    type Response = GetQuotaInfoResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        // TODO Respect the requested transfer method. Or, at least, query by target, not by storage
        // pool (since this and only this is used by ctl for the request).

        let pool_id = self.pool_id;

        let res = ctx
            .db
            .op(move |tx| {
                // Check pool id exists
                let _ =
                    resolve_num_id(tx, EntityType::Pool, NodeType::Storage, self.pool_id.into())?;

                let limits = match self.query_type {
                    QuotaQueryType::None => return Ok(vec![]),
                    QuotaQueryType::Single => db::quota_limit::with_quota_id_range(
                        tx,
                        self.id_range_start..=self.id_range_start,
                        self.pool_id,
                        self.id_type,
                    )?,
                    QuotaQueryType::Range => db::quota_limit::with_quota_id_range(
                        tx,
                        self.id_range_start..=self.id_range_end,
                        self.pool_id,
                        self.id_type,
                    )?,
                    QuotaQueryType::List => db::quota_limit::with_quota_id_list(
                        tx,
                        self.id_list,
                        self.pool_id,
                        self.id_type,
                    )?,
                    QuotaQueryType::All => {
                        // This is actually unused on the old ctl side, if --all is provided, it
                        // sends a list
                        db::quota_limit::all(tx, self.pool_id, self.id_type)?
                    }
                };

                let res = limits
                    .into_iter()
                    .map(|limit| QuotaEntry {
                        space: limit.space.unwrap_or_default(),
                        inodes: limit.inodes.unwrap_or_default(),
                        id: limit.quota_id,
                        id_type: self.id_type,
                        valid: 1,
                    })
                    .collect();

                Ok(res)
            })
            .await;

        match res {
            Ok(data) => GetQuotaInfoResp {
                quota_inode_support: QuotaInodeSupport::Unknown,
                quota_entry: data,
            },
            Err(err) => {
                log_error_chain!(
                    err,
                    "Getting quota info for storage pool {} failed",
                    pool_id,
                );

                GetQuotaInfoResp {
                    quota_inode_support: QuotaInodeSupport::Unknown,
                    quota_entry: vec![],
                }
            }
        }
    }
}

impl Handler for SetQuota {
    type Response = SetQuotaResp;

    async fn handle(self, ctx: &Context, __req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                // Check pool ID exists
                let _ =
                    resolve_num_id(tx, EntityType::Pool, NodeType::Storage, self.pool_id.into())?;

                db::quota_limit::update(
                    tx,
                    self.quota_entry.into_iter().map(|e| {
                        (
                            e.id_type,
                            self.pool_id,
                            SpaceAndInodeLimits {
                                quota_id: e.id,
                                space: match e.space {
                                    0 => None,
                                    n => Some(n),
                                },
                                inodes: match e.inodes {
                                    0 => None,
                                    n => Some(n),
                                },
                            },
                        )
                    }),
                )
            })
            .await;

        match res {
            Ok(_) => {
                log::info!("Set quota for storage pool {}", self.pool_id,);
                SetQuotaResp { result: 1 }
            }

            Err(err) => {
                log_error_chain!(
                    err,
                    "Setting quota for storage pool {} failed",
                    self.pool_id
                );

                SetQuotaResp { result: 0 }
            }
        }
    }
}

impl Handler for RequestExceededQuota {
    type Response = RequestExceededQuotaResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                let exceeded_ids = db::quota_usage::exceeded_quota_ids(
                    tx,
                    if self.pool_id != 0 {
                        PoolOrTargetId::PoolID(self.pool_id)
                    } else {
                        PoolOrTargetId::TargetID(self.target_id)
                    },
                    self.id_type,
                    self.quota_type,
                )?;

                Ok(SetExceededQuota {
                    pool_id: self.pool_id,
                    id_type: self.id_type,
                    quota_type: self.quota_type,
                    exceeded_quota_ids: exceeded_ids,
                })
            })
            .await;

        match res {
            Ok(inner) => RequestExceededQuotaResp {
                result: OpsErr::SUCCESS,
                inner,
            },
            Err(err) => {
                log_error_chain!(
                    err,
                    "Fetching exceeded quota ids for storage pool {} or target {} failed",
                    self.pool_id,
                    self.target_id
                );
                RequestExceededQuotaResp {
                    result: OpsErr::INTERNAL,
                    inner: SetExceededQuota::default(),
                }
            }
        }
    }
}
