use super::*;

/// Enables a metadata mirrored system
///
/// Used by old ctl and self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetMetadataMirroring {}

impl Msg for SetMetadataMirroring {
    const ID: MsgID = 2069;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetMetadataMirroringResp {
    pub result: OpsErr,
}

impl Msg for SetMetadataMirroringResp {
    const ID: MsgID = 2070;
}
