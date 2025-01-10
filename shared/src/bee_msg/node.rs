use super::*;
use anyhow::bail;
use std::net::{IpAddr, Ipv4Addr};

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
    #[bee_serde(as = Seq<true, _>)]
    pub nic_list: Vec<Nic>,
    pub num_id: NodeId,
    pub port: Port,
    pub _unused_tcp_port: Port,
    #[bee_serde(as = Int<u8>)]
    pub node_type: NodeType,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Nic {
    pub addr: IpAddr,
    pub name: Vec<u8>,
    pub nic_type: NicType,
}

impl Default for Nic {
    fn default() -> Self {
        Self {
            addr: Ipv4Addr::UNSPECIFIED.into(),
            name: Default::default(),
            nic_type: Default::default(),
        }
    }
}

impl Serializable for Nic {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        match self.addr {
            IpAddr::V4(addr) => {
                // protocol: IPv4
                ser.u8(4)?;
                // address
                ser.u32(u32::from_le_bytes(addr.octets()))?;
            }
            IpAddr::V6(addr) => {
                // protocol: IPv6
                ser.u8(6)?;
                // address
                ser.u128(u128::from_le_bytes(addr.octets()))?;
            }
        }

        // Cut off nic name after 15 bytes
        let name = self.name.chunks(15).next().unwrap_or_default();
        ser.bytes(name)?;
        // Fill the rest with zeroes
        ser.zeroes(16 - name.len())?;

        ser.u8(self.nic_type.into_bee_serde())?;
        ser.zeroes(2)?;
        Ok(())
    }
}

impl Deserializable for Nic {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        let protocol = des.u8()?;
        let addr: IpAddr = match protocol {
            4 => des.u32()?.to_le_bytes().into(),
            6 => des.u128()?.to_le_bytes().into(),
            n => bail!("Nic protocol field must be 4 or 6, is {n}"),
        };

        let mut name = des.bytes(15)?;
        // Ignore the 16th name byte to avoid different names than in C/C++ where this is always set
        // to 0 on deserialization
        des.u8()?;

        let nic_type = NicType::try_from_bee_serde(des.u8()?)?;
        des.skip(2)?;

        // The name might be filled with null bytes, which we don't want to deal with - we remove
        // them
        name.retain(|b| b != &0);

        Ok(Self {
            addr,
            name,
            nic_type,
        })
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
    #[bee_serde(as = Seq<true, _>)]
    pub nic_list: Vec<Nic>,
    #[bee_serde(as = CStr<0>)]
    pub machine_uuid: Vec<u8>,
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
    #[bee_serde(as = Seq<true, _>)]
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
    #[bee_serde(as = CStr<0>)]
    pub machine_uuid: Vec<u8>,
}

impl Msg for RegisterNode {
    const ID: MsgId = 1039;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterNodeResp {
    pub node_num_id: NodeId,
    pub grpc_port: Port,
    #[bee_serde(as = CStr<0>)]
    pub fs_uuid: Vec<u8>,
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
