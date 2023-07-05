use super::*;

/// Indicates a meta or storage node to send its capacity information (via set_target_info)
///
/// The nodes already do this based on a timer by themselves, so this message would be only useful
/// if very up to date values are needed on demand (which they aren't at the moment).
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct PublishCapacities {
    pub ack_id: AckID,
}

impl Msg for PublishCapacities {
    const ID: MsgID = MsgID(1059);
}
