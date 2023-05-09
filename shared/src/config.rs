use crate::conn::{AddrResolver, ConnPool, PeerID};
use crate::{msg, CapPoolDynamicLimits, CapPoolLimits, QuotaID};
use async_trait::async_trait;
use config::{BoxedError, ConfigMap, Source};
use std::collections::HashSet;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ManagementSource<Res: AddrResolver> {
    management_addr: PeerID,
    conn: ConnPool<Res>,
}

impl<Res: AddrResolver> ManagementSource<Res> {
    pub fn new(management_addr: PeerID, conn: ConnPool<Res>) -> Self {
        Self {
            management_addr,
            conn,
        }
    }
}

#[async_trait]
impl<Res: AddrResolver> Source for ManagementSource<Res> {
    async fn get(&self) -> Result<ConfigMap, BoxedError> {
        Ok(self
            .conn
            .request::<_, msg::GetAllConfigResp>(self.management_addr, &msg::GetConfig {})
            .await?
            .entries)
    }
}

config::define_config!(
    struct BeeConfig,

    // Misc
    RegistrationEnable: bool = true,
    NodeOfflineTimeout: Duration = Duration::from_secs(180),
    ClientAutoRemoveTimeout: Duration = Duration::from_secs(30 * 60),

    // Quota
    QuotaEnable: bool = false,
    QuotaUserIDs: HashSet<QuotaID> = HashSet::new(),
    QuotaGroupIDs: HashSet<QuotaID> = HashSet::new(),
    QuotaUpdateInterval: Duration = Duration::from_secs(30),

    // Capacity pools
    CapPoolMetaLimits: CapPoolLimits = CapPoolLimits {
        inodes_low: (10 * 1000 * 1000).into(),
        inodes_emergency: (1000 * 1000).into(),
        space_low: (10 * 1024 * 1024 * 1024).into(),
        space_emergency: (3 * 1024 * 1024 * 1024).into()
    },
    CapPoolStorageLimits: CapPoolLimits = CapPoolLimits {
        inodes_low: (10 * 1000 * 1000).into(),
        inodes_emergency: (1000 * 1000).into(),
        space_low: (512 * 1024 * 1024 * 1024).into(),
        space_emergency: (10 * 1024 * 1024 * 1024).into()
    },

    // Dynamic capacity pools
    CapPoolDynamicMetaLimits: Option<CapPoolDynamicLimits> = None,
    CapPoolDynamicStorageLimits: Option<CapPoolDynamicLimits> = None,
);
