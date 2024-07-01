use super::*;
use anyhow::bail;
use std::net::Ipv4Addr;

/// Fetch all nodes of the given type
///
/// Used by old ctl, fsck, meta, mon, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodes {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
}

impl Msg for GetNodes {
    const ID: MsgId = 1017;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodesResp {
    #[bee_serde(as = Seq<false, _>)]
    pub nodes: Vec<Node>,
    /// If the requested node type was Meta, then this contains the target / buddy group ID which
    /// owns the root inode.
    pub root_num_id: u32,
    /// Determines whether root_num_id is a target or buddy group ID
    pub is_root_mirrored: u8,
}

impl Msg for GetNodesResp {
    const ID: MsgId = 1018;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct Node {
    #[bee_serde(as = CStr<0>)]
    pub alias: Vec<u8>,
    #[bee_serde(as = Seq<false, _>)]
    pub nic_list: Vec<Nic>,
    pub num_id: NodeId,
    pub port: Port,
    pub _unused_tcp_port: Port,
    #[bee_serde(as = Int<u8>)]
    pub node_type: NodeType,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Nic {
    pub addr: Ipv4Addr,
    pub name: Vec<u8>,
    pub nic_type: NicType,
}

impl Default for Nic {
    fn default() -> Self {
        Self {
            addr: Ipv4Addr::UNSPECIFIED,
            name: Default::default(),
            nic_type: Default::default(),
        }
    }
}

impl Serializable for Nic {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u32(u32::from_le_bytes(self.addr.octets()))?;

        if self.name.len() > 16 {
            bail!("Nic alias can not be longer than 16 bytes");
        }
        ser.bytes(self.name.as_ref())?;
        ser.zeroes(16 - self.name.len())?;

        ser.u8(self.nic_type.into_bee_serde())?;
        ser.zeroes(3)?;
        Ok(())
    }
}

impl Deserializable for Nic {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        let mut s = Self {
            addr: des.u32()?.to_le_bytes().into(),
            name: des.bytes(16)?,
            nic_type: NicType::try_from_bee_serde(des.u8()?)?,
        };

        des.skip(3)?;

        // This is filled up with null bytes, which we don't want to deal with - we remove them
        s.name.retain(|b| b != &0);

        Ok(s)
    }
}

/// Requests a heartbeat from a node.
///
/// Usually sent by UDP.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct HeartbeatRequest {}

impl Msg for HeartbeatRequest {
    const ID: MsgId = 1019;
}

/// Updates a node with the given information.
///
/// Similar to [RegisterNode]
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct Heartbeat {
    /// Unused
    pub instance_version: u64,
    /// Unused
    pub nic_list_version: u64,
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = CStr<0>)]
    pub node_alias: Vec<u8>,
    #[bee_serde(as = CStr<4>)]
    pub ack_id: Vec<u8>,
    pub node_num_id: NodeId,
    // The root info is only relevant when sent from meta nodes. There it must contain the meta
    // root nodes ID, but on other nodes it is just irrelevant.
    // Can be a Node ID or a BuddyGroup ID
    pub root_num_id: u32,
    pub is_root_mirrored: u8,
    pub port: Port,
    /// This is transmitted from other nodes but we decided to just use one port for TCP and UDP in
    /// the future
    pub port_tcp_unused: Port,
    #[bee_serde(as = Seq<false, _>)]
    pub nic_list: Vec<Nic>,
}

impl Msg for Heartbeat {
    const ID: MsgId = 1020;
}

/// Registers a new node with the given information.
///
/// Similar to [Heartbeat]
///
/// Used by client, meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterNode {
    /// Unused
    pub instance_version: u64,
    /// Unused
    pub nic_list_version: u64,
    #[bee_serde(as = CStr<0>)]
    pub node_alias: Vec<u8>,
    #[bee_serde(as = Seq<false, _>)]
    pub nics: Vec<Nic>,
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    pub node_id: NodeId,
    pub root_num_id: u32,
    pub is_root_mirrored: u8,
    pub port: Port,
    /// This is transmitted from other nodes but we decided to just use one port for TCP and UDP in
    /// the future
    pub port_tcp_unused: Port,
}

impl Msg for RegisterNode {
    const ID: MsgId = 1039;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterNodeResp {
    pub node_num_id: NodeId,
}

impl Msg for RegisterNodeResp {
    const ID: MsgId = 1040;
}

/// Remove a node from the system
///
/// Used by old ctl, client, self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveNode {
    #[bee_serde(as = Int<i16>)]
    pub node_type: NodeType,
    pub node_id: NodeId,
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for RemoveNode {
    const ID: MsgId = 1013;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveNodeResp {
    pub result: OpsErr,
}

impl Msg for RemoveNodeResp {
    const ID: MsgId = 1014;
}
