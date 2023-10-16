use super::*;

/// The BeeGFS generic response code
pub type GenericResponseCode = i32;
pub const TRY_AGAIN: GenericResponseCode = 0;
pub const INDIRECT_COMM_ERR: GenericResponseCode = 1;
pub const NEW_SEQ_NO_BASE: GenericResponseCode = 2;

/// Replaces the expected response to a message and signals the requester that something went wrong.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GenericResponse {
    pub code: GenericResponseCode,
    #[bee_serde(as = CStr<0>)]
    pub description: Vec<u8>,
}

impl Msg for GenericResponse {
    const ID: MsgID = 4009;
}
