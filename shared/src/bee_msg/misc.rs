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
    const ID: MsgId = 4009;
}

/// Expected response when a UDP message has been received.
///
/// Does actually nothing on BeeGFS nodes (except for maybe printing an error after some timeout).
/// Incoming Acks can just be ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct Ack {
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for Ack {
    const ID: MsgId = 4003;
}

/// Authenticate the communication channel (e.g. the TCP connection on which this message comes in).
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AuthenticateChannel {
    pub auth_secret: AuthSecret,
}

impl Msg for AuthenticateChannel {
    const ID: MsgId = 4007;
}

/// First message of the BeeMsg key exchange, sent by the side opening the connection.
///
/// Travels in the clear - it contains no secrets, only public keys. `static_pub` selects which key
/// the responder should authenticate us against; the responder looks it up in the key list it
/// downloaded from management and rejects the connection if it is not there.
///
/// See [`crate::crypto::handshake`] for the protocol.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct KeyExchangeRequest {
    /// Handshake wire version, see [`crate::crypto::handshake::HANDSHAKE_VERSION`].
    pub version: u32,
    /// Our long-term public key - identifies us to the peer.
    pub static_pub: StaticPubKey,
    /// Our freshly generated ephemeral public key.
    pub ephemeral_pub: [u8; 32],
}

impl Msg for KeyExchangeRequest {
    const ID: MsgId = 4013;
}

/// Second and final message of the BeeMsg key exchange.
///
/// Everything after this message is encrypted under the derived session keys.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct KeyExchangeResponse {
    /// The responder's freshly generated ephemeral public key.
    pub ephemeral_pub: [u8; 32],
    /// HMAC over the handshake transcript, keyed from the derived chaining key. Proves the
    /// responder holds the private key matching the public key we have for it, and that it
    /// recognised us - so a mismatch fails here instead of on undecryptable traffic later.
    pub confirm: [u8; 32],
}

impl Msg for KeyExchangeResponse {
    const ID: MsgId = 4015;
}

/// Tells the existence of a node
///
/// Only used by the client after opening a connection.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct PeerInfo {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
    pub node_id: NodeId,
}

impl Msg for PeerInfo {
    const ID: MsgId = 4011;
}

/// Sets the type of the worker that handles this connection channel.
///
/// Unused/ignored in the Rust code.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetChannelDirect {
    pub is_direct: i32,
}

impl Msg for SetChannelDirect {
    const ID: MsgId = 4001;
}

/// Indicates anodes to fetch fresh capacity info from management (sent via UDP).
///
/// Nodes then request the newest info via [GetNodeCapacityPools]. No idea why the info is not just
/// sent with this message.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RefreshCapacityPools {
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for RefreshCapacityPools {
    const ID: MsgId = 1035;
}

/// Fetches node capacity pools of the given type for all targets / groups.
///
/// Used by ctl, meta
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodeCapacityPools {
    #[bee_serde(as = Int<i32>)]
    pub query_type: CapacityPoolQueryType,
}

impl Msg for GetNodeCapacityPools {
    const ID: MsgId = 1021;
}

/// Response containing node capacity bool mapping
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GetNodeCapacityPoolsResp {
    /// Target or group IDs grouped by storage pool and cap pool.
    ///
    /// The outer Vec has index 0, 1, 2 containing lists with IDs belonging to that pool.
    pub pools: HashMap<PoolId, Vec<Vec<u16>>>,
}

impl Msg for GetNodeCapacityPoolsResp {
    const ID: MsgId = 1022;
}

// Custom BeeSerde impl because nested sequences / maps are not supported by the macro
impl Serializable for GetNodeCapacityPoolsResp {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.map(
            self.pools.iter(),
            false,
            |ser, k| k.serialize(ser),
            |ser, v| {
                ser.seq(v.iter(), true, |ser, e| {
                    ser.seq(e.iter(), true, |ser, g| ser.u16(*g))
                })
            },
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CapacityPoolQueryType {
    #[default]
    Meta,
    Storage,
    MetaMirrored,
    StorageMirrored,
}

impl_enum_bee_msg_traits!(CapacityPoolQueryType, Meta => 0, Storage => 1, MetaMirrored => 2, StorageMirrored => 3);
