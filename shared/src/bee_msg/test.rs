//! Tests for the BeeMsg header, frame header and record framing.

use super::*;
use crate::bee_msg::misc::AuthenticateChannel;
use crate::conn::protocol::*;
use std::str::FromStr;

/// Serializes a message the way the legacy send path does.
fn serialize_v1<M: Msg + Serializable>(msg: &M, buf: &mut [u8]) -> Result<Header> {
    let header = serialize_body(msg, buf)?;
    header.serialize_legacy(&mut buf[..Header::END_POS])?;
    Ok(header)
}

/// Serializes a message the way the new send path does for an unprotected payload.
fn serialize_v2<M: Msg + Serializable>(msg: &M, buf: &mut [u8]) -> Result<Header> {
    let header = serialize_body(msg, buf)?;

    FrameHeader {
        frame_type: FrameType::Message,
        protection: TransportProtectionMode::Plain,
        frame_len: header.msg_len(),
    }
    .serialize(buf)?;
    header.serialize(&mut buf[FrameHeader::END_POS..Header::END_POS])?;

    Ok(header)
}

/// A message with a small, fully determined body.
fn test_msg() -> AuthenticateChannel {
    AuthenticateChannel {
        // 0x0102030405060708, so every body byte is distinct.
        auth_secret: AuthSecret::from_str("72623859790382856").unwrap(),
    }
}

const MSG_ID: [u8; 2] = [0xa7, 0x0f]; // 4007
const BODY: [u8; 8] = [8, 7, 6, 5, 4, 3, 2, 1];

/// Pins the legacy wire format. Other BeeGFS implementations parse exactly these bytes, so a
/// change here is a compatibility break and must be deliberate.
#[test]
fn legacy_golden_bytes() {
    #[rustfmt::skip]
    let expected: [u8; 48] = [
        48, 0, 0, 0,                    // msg_len = 40 + 8
        0, 0,                           // msg_feature_flags
        0,                              // msg_compat_feature_flags
        0,                              // msg_flags
        0, 0, 0, 0, 0x53, 0x46, 0x47, 0x42, // msg_prefix
        MSG_ID[0], MSG_ID[1],           // msg_id
        0, 0,                           // msg_target_id
        0, 0, 0, 0,                     // msg_user_id
        0, 0, 0, 0, 0, 0, 0, 0,         // msg_seq
        0, 0, 0, 0, 0, 0, 0, 0,         // msg_seq_done
        BODY[0], BODY[1], BODY[2], BODY[3], BODY[4], BODY[5], BODY[6], BODY[7],
    ];

    let mut buf = [0u8; 64];
    let len = serialize_v1(&test_msg(), &mut buf).unwrap().msg_len();

    assert_eq!(len, expected.len());
    assert_eq!(&buf[..len], &expected);
}

/// Pins the new wire format against the layout the C++ tree and kernel client must match.
#[test]
fn new_protocol_golden_bytes() {
    #[rustfmt::skip]
    let expected: [u8; 48] = [
        b'B', b'G', b'F', b'S',         // frame magic
        48, 0, 0, 0,                    // frame_len = 12 + 28 + 8
        4,                              // frame_type = Message
        0,                              // frame_flags
        0, 0,                           // reserved
        0, 0,                           // msg_feature_flags
        0,                              // msg_compat_feature_flags
        0,                              // msg_flags
        MSG_ID[0], MSG_ID[1],           // msg_id
        0, 0,                           // msg_target_id
        0, 0, 0, 0,                     // msg_user_id
        0, 0, 0, 0, 0, 0, 0, 0,         // msg_seq
        0, 0, 0, 0, 0, 0, 0, 0,         // msg_seq_done
        BODY[0], BODY[1], BODY[2], BODY[3], BODY[4], BODY[5], BODY[6], BODY[7],
    ];

    let mut buf = [0u8; 64];
    let len = serialize_v2(&test_msg(), &mut buf).unwrap().msg_len();

    assert_eq!(len, expected.len());
    assert_eq!(&buf[..len], &expected);
}

#[test]
fn round_trip() {
    let msg = test_msg();

    let mut buf = [0u8; 64];
    let len = serialize_v1(&msg, &mut buf).unwrap().msg_len();
    let header = Header::deserialize_legacy(&buf).unwrap();
    assert_eq!(header.msg_len(), len);
    assert_eq!(header.msg_id(), AuthenticateChannel::ID);
    assert_eq!(
        deserialize_body::<AuthenticateChannel>(&header, &buf[Header::END_POS..]).unwrap(),
        msg
    );

    let mut buf = [0u8; 64];
    let len = serialize_v2(&msg, &mut buf).unwrap().msg_len();
    let frame = FrameHeader::deserialize(&buf).unwrap();
    assert_eq!(frame.frame_type, FrameType::Message);
    assert_eq!(frame.protection, TransportProtectionMode::Plain);
    // Both count the frame header, so an unprotected Message frame carries the same number twice.
    assert_eq!(frame.frame_len, len);
    let header =
        Header::deserialize(&buf[FrameHeader::END_POS..Header::END_POS], frame.frame_len).unwrap();
    assert_eq!(header.msg_len(), len);
    assert_eq!(header.msg_id(), AuthenticateChannel::ID);
    assert_eq!(
        deserialize_body::<AuthenticateChannel>(&header, &buf[Header::END_POS..]).unwrap(),
        msg
    );
}

/// Both protocols keep the body at the same offset, which is what lets `msg_len` and
/// `deserialize_body` stay protocol agnostic.
#[test]
fn body_offset_is_protocol_independent() {
    let mut legacy = [0u8; 64];
    let mut new = [0u8; 64];

    let legacy_len = serialize_v1(&test_msg(), &mut legacy).unwrap().msg_len();
    let new_len = serialize_v2(&test_msg(), &mut new).unwrap().msg_len();

    assert_eq!(legacy_len, new_len);
    assert_eq!(
        &legacy[Header::END_POS..legacy_len],
        &new[Header::END_POS..new_len]
    );
}

/// A peer controls these lengths, so they must produce errors rather than panics.
#[test]
fn rejects_out_of_range_lengths() {
    let mut buf = [0u8; 64];
    serialize_v1(&test_msg(), &mut buf).unwrap();

    let with_msg_len = |msg_len: u32| {
        let mut b = buf;
        b[0..4].copy_from_slice(&msg_len.to_le_bytes());
        b
    };

    // Too small to even hold the header.
    for msg_len in [0u32, 1, (Header::END_POS - 1) as u32] {
        Header::deserialize_legacy(&with_msg_len(msg_len)).unwrap_err();
    }

    // Bigger than the buffer it is supposed to arrive in.
    Header::deserialize_legacy(&with_msg_len((buf.len() + 1) as u32)).unwrap_err();

    // A buffer too short to hold the header at all must be an error, not a panic.
    Header::deserialize_legacy(&buf[..Header::END_POS - 1]).unwrap_err();

    // Passes the length check, but the body does not fit the buffer handed over.
    let full = with_msg_len(buf.len() as u32);
    let header = Header::deserialize_legacy(&full).unwrap();
    deserialize_body::<AuthenticateChannel>(&header, &full[Header::END_POS..Header::END_POS + 8])
        .unwrap_err();

    // A wrong prefix is what a legacy reader sees when the peer speaks the new protocol.
    let mut bad_prefix = buf;
    bad_prefix[8] ^= 0xff;
    Header::deserialize_legacy(&bad_prefix).unwrap_err();

    // New protocol: a payload too small to hold the header.
    let mut buf = [0u8; 64];
    serialize_v2(&test_msg(), &mut buf).unwrap();
    Header::deserialize(
        &buf[FrameHeader::END_POS..Header::END_POS],
        Header::END_POS - 1,
    )
    .unwrap_err();
}

/// snow rejects a payload whose ciphertext would exceed `MAXMSGLEN`, so a full record must sit
/// exactly on that limit. Pins the constants against a snow update.
#[test]
fn record_sizes_match_noise_limits() {
    assert_eq!(RECORD_PLAINTEXT_LEN + RECORD_TAG_LEN, NOISE_MAX_MSG_LEN);
    assert_eq!(RECORD_LEN, NOISE_MAX_MSG_LEN);
    assert_eq!(RECORD_PLAINTEXT_LEN, 65519);
}

#[test]
fn frame_header_round_trip() {
    for ftype in [
        FrameType::HandshakeInit,
        FrameType::HandshakeResponse,
        FrameType::HandshakeReject,
        FrameType::Message,
    ] {
        for protection in [
            TransportProtectionMode::Plain,
            TransportProtectionMode::Encrypted,
        ] {
            for frame_len in [
                FrameHeader::END_POS,
                FrameHeader::END_POS + 88,
                MAX_FRAME_PAYLOAD_LEN,
                MAX_FRAME_LEN,
            ] {
                let h = FrameHeader {
                    frame_type: ftype,
                    protection,
                    frame_len,
                };

                let mut buf = [0u8; FrameHeader::END_POS];
                h.serialize(&mut buf).unwrap();
                assert_eq!(FrameHeader::deserialize(&buf).unwrap(), h);
            }
        }
    }
}

/// The magic must be the bytes "BGFS" so a peer of the other protocol fails intelligibly.
#[test]
fn frame_magic_wire_bytes() {
    let mut buf = [0u8; FrameHeader::END_POS];
    FrameHeader {
        frame_type: FrameType::Message,
        protection: TransportProtectionMode::Plain,
        frame_len: FrameHeader::END_POS,
    }
    .serialize(&mut buf)
    .unwrap();

    assert_eq!(&buf[0..4], b"BGFS");
    assert!(FrameHeader::MAGIC as usize > crate::conn::TCP_BUF_LEN);
}

#[test]
fn frame_header_rejects_malformed() {
    let good = {
        let mut buf = [0u8; FrameHeader::END_POS];
        FrameHeader {
            frame_type: FrameType::Message,
            protection: TransportProtectionMode::Plain,
            frame_len: FrameHeader::END_POS + 64,
        }
        .serialize(&mut buf)
        .unwrap();
        buf
    };

    FrameHeader::deserialize(&good).unwrap();
    FrameHeader::deserialize(&good[..FrameHeader::END_POS - 1]).unwrap_err();

    let mut bad_magic = good;
    bad_magic[0] ^= 0xff;
    FrameHeader::deserialize(&bad_magic).unwrap_err();

    let mut bad_type = good;
    bad_type[8] = 0;
    FrameHeader::deserialize(&bad_type).unwrap_err();
    bad_type[8] = 5;
    FrameHeader::deserialize(&bad_type).unwrap_err();

    let mut reserved_flag = good;
    reserved_flag[9] = FrameHeader::FLAG_RESERVED;
    FrameHeader::deserialize(&reserved_flag).unwrap_err();

    let mut unused_flag = good;
    unused_flag[9] = 1 << 7;
    FrameHeader::deserialize(&unused_flag).unwrap_err();

    let mut bad_reserved = good;
    bad_reserved[10] = 1;
    FrameHeader::deserialize(&bad_reserved).unwrap_err();

    let mut too_long = good;
    too_long[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    FrameHeader::deserialize(&too_long).unwrap_err();

    let mut just_too_long = good;
    just_too_long[4..8].copy_from_slice(&((MAX_FRAME_LEN + 1) as u32).to_le_bytes());
    FrameHeader::deserialize(&just_too_long).unwrap_err();
}

#[test]
fn record_len_round_trip() {
    const P: usize = RECORD_PLAINTEXT_LEN;

    for plaintext_len in [
        1,
        28,
        P - 1,
        P,
        P + 1,
        2 * P,
        2 * P + 1,
        MAX_FRAME_PAYLOAD_LEN,
    ] {
        let frame_len = record_frame_len(plaintext_len);
        assert_eq!(
            record_plaintext_len(frame_len).unwrap(),
            plaintext_len,
            "plaintext_len {plaintext_len}"
        );
    }

    assert_eq!(record_count(1), 1);
    assert_eq!(record_count(P), 1);
    assert_eq!(record_count(P + 1), 2);
    assert_eq!(record_frame_len(P), FrameHeader::END_POS + RECORD_LEN);
}

#[test]
fn record_len_rejects_non_canonical() {
    // Two records where one filled record would have carried the same plaintext.
    assert!(record_plaintext_len(FrameHeader::END_POS + RECORD_LEN + RECORD_TAG_LEN).is_err());
    // Under-filled first record of a two record split.
    assert!(record_plaintext_len(record_frame_len(RECORD_PLAINTEXT_LEN) + 1).is_err());

    assert!(record_plaintext_len(0).is_err());
    assert!(record_plaintext_len(FrameHeader::END_POS - 1).is_err());
    assert!(record_plaintext_len(FrameHeader::END_POS).is_err());
    assert!(record_plaintext_len(FrameHeader::END_POS + RECORD_TAG_LEN).is_err());
    assert!(record_plaintext_len(MAX_FRAME_LEN + 1).is_err());
}
