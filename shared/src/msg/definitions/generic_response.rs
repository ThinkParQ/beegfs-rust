use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GenericResponse {
    pub code: GenericResponseCode,
    #[bee_serde(as = CStr<0>)]
    pub description: Vec<u8>,
}

impl Msg for GenericResponse {
    const ID: MsgID = MsgID(4009);
}