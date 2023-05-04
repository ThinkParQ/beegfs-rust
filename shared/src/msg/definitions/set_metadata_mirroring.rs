use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetMetadataMirroring {}

impl Msg for SetMetadataMirroring {
    const ID: MsgID = MsgID(2069);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetMetadataMirroringResp {
    pub result: OpsErr,
}

impl Msg for SetMetadataMirroringResp {
    const ID: MsgID = MsgID(2070);
}
