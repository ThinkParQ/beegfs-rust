use super::*;
use anyhow::bail;
use std::net::Ipv4Addr;

/// Fetch all nodes of the given type
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodes {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
}

impl Msg for GetNodes {
    const ID: MsgID = 1017;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodesResp {
    #[bee_serde(as = Seq<false, _>)]
    pub nodes: Vec<Node>,
    /// If the requested node type was Meta, then this contains the target / buddy group ID which
    /// owns the root inode.
    pub root_num_id: u32,
    /// Determines wether root_num_id is a target or buddy group ID
    pub is_root_mirrored: u8,
}

impl Msg for GetNodesResp {
    const ID: MsgID = 1018;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct Node {
    #[bee_serde(as = CStr<0>)]
    pub alias: Vec<u8>,
    #[bee_serde(as = Seq<false, _>)]
    pub nic_list: Vec<Nic>,
    #[bee_serde(as = Int<u32>)]
    pub num_id: NodeID,
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

impl BeeSerde for Nic {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u32(u32::from_le_bytes(self.addr.octets()))?;

        if self.name.len() > 16 {
            bail!("Nic alias can not be longer than 16 bytes");
        }
        ser.bytes(self.name.as_ref())?;
        ser.zeroes(16 - self.name.len())?;

        ser.u8(self.nic_type.into())?;
        ser.zeroes(3)?;
        Ok(())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        let s = Self {
            addr: des.u32()?.to_le_bytes().into(),
            name: des.bytes(16)?,
            nic_type: des.u8()?.try_into()?,
        };

        des.skip(3)?;

        Ok(s)
    }
}
