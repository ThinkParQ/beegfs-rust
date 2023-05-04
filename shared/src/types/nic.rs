use super::*;
use std::fmt::Debug;
use std::net::Ipv4Addr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NicType {
    #[default]
    Ethernet,
    Sdp,
    Rdma,
}

impl_enum_to_int!(NicType, Ethernet => 0, Sdp => 1, Rdma => 2);
impl_enum_to_sql_str!(NicType, Ethernet => "ethernet", Sdp => "sdp", Rdma => "rdma");

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Nic {
    pub addr: Ipv4Addr,
    pub alias: EntityAlias,
    pub nic_type: NicType,
}

impl Default for Nic {
    fn default() -> Self {
        Self {
            addr: Ipv4Addr::UNSPECIFIED,
            alias: Default::default(),
            nic_type: Default::default(),
        }
    }
}

impl bee_serde::BeeSerde for Nic {
    fn serialize(&self, ser: &mut bee_serde::Serializer<'_>) -> Result<()> {
        ser.u32(u32::from_le_bytes(self.addr.octets()))?;

        let alias = self.alias.as_ref();
        if alias.len() > 16 {
            bail!("Nic alias can not be longer than 16 bytes");
        }
        ser.bytes(alias)?;
        ser.zeroes(16 - self.alias.as_ref().len())?;

        ser.u8(self.nic_type.into())?;
        ser.zeroes(3)?;
        Ok(())
    }

    fn deserialize(des: &mut bee_serde::Deserializer<'_>) -> Result<Self> {
        let s = Self {
            addr: des.u32()?.to_le_bytes().into(),
            alias: des.bytes(16)?.try_into()?,
            nic_type: des.u8()?.try_into()?,
        };

        des.skip(3)?;

        Ok(s)
    }
}
