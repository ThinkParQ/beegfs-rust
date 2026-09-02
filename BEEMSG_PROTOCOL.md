# The BeeMsg wire protocol

Reference for reimplementing BeeMsg framing outside this repository — the C++ servers and the
kernel client. Everything here is a wire contract: changing any of it breaks interoperability and
needs a coordinated change in all trees.

The Rust implementation lives in `shared/src/conn/protocol.rs` (framing, the frame header and the
static keys), `shared/src/bee_msg.rs` (both BeeMsg header layouts), `shared/src/conn/stream.rs` (the
send and receive paths), `shared/src/conn/noise.rs` (the key exchange primitives) and
`shared/src/conn/handshake.rs` (the handshake frames). The golden vectors in
`shared/src/conn/handshake.rs` are the authoritative test data; embed the same hex.

All integers are little-endian and naturally aligned, so a packed C struct plus `memcpy` works.

## Protocol selection

One of four values, configured identically on every node of a system. There is **no negotiation** at
the framing level: a mismatch shows up as a failed connection, not as a fallback.

| Value | Framing | Key exchange | Message protection |
|---|---|---|---|
| `legacy` | legacy header | `AuthenticateChannel` shared secret | none |
| `plain` | frame header | none | none |
| `authenticated` | frame header | Noise KK | none after setup |
| `encrypted` | frame header | Noise KK | Noise records |

`legacy` is the default and must stay byte-identical to pre-8.x releases.

**UDP datagrams always use `legacy`**, whatever the setting. Nothing in this document applies to
them.

`authenticated` deliberately protects nothing after connection setup: it proves who the peer is and
then trusts the TCP stream, which is exactly what `AuthenticateChannel` does today. An on-path
attacker can still tamper with traffic. Use `encrypted` where that matters.

## Legacy framing

A stream is a bare sequence of `[40-byte header][body]`.

| off | size | field |
|---|---|---|
| 0 | 4 | `msg_len` — total length including the header |
| 4 | 2 | `msg_feature_flags` |
| 6 | 1 | `msg_compat_feature_flags` |
| 7 | 1 | `msg_flags` |
| 8 | 8 | `msg_prefix` = `0x4247465300000000` |
| 16 | 2 | `msg_id` |
| 18 | 2 | `msg_target_id` |
| 20 | 4 | `msg_user_id` |
| 24 | 8 | `msg_seq` |
| 32 | 8 | `msg_seq_done` |

A receiver must reject `msg_len` below 40 or above its own buffer *before* slicing on it.

## New framing

The first 12 bytes are a frame header and are always cleartext. Everything after them can be
protected.

```
frame header — 12 bytes, always cleartext
  0..4    magic: u32 = 0x53464742        reads as "BGFS" on the wire
  4..8    frame_len: u32                 total frame length, including this header
  8..9    frame_type: u8                 1 HandshakeInit, 2 HandshakeResponse,
                                         3 HandshakeReject, 4 Message
  9..10   frame_flags: u8                bit0 = payload is Noise records
                                         bit1 reserved, bits 2..7 MUST be 0
  10..12  reserved: u16 = 0              MUST be 0
```

A receiver MUST reject a non-zero `reserved`, an unknown `frame_type`, and any set bit among
`frame_flags` bits 1..7. Rejecting them is what keeps those bits usable for a later extension.

A receiver MUST also reject a `frame_len` below 12 or larger than the biggest frame it can hold —
for the record layer that is the buffer limit plus the header plus one tag per record — and it MUST
do so while parsing the frame header, before reading or slicing on the announced length. `frame_len`
is fully peer controlled and is the only length the new framing carries.

`frame_len` counts the frame header itself, matching the legacy `msg_len` convention. For a Message
frame that is not protected, `frame_len` is therefore numerically equal to what `msg_len` would have
been for the same message.

For `frame_type = 4` (Message) the payload begins with the BeeMsg header:

```
BeeMsg header — 28 bytes, inside the protected region
  12..14  msg_feature_flags: u16
  14..15  msg_compat_feature_flags: u8
  15..16  msg_flags: u8
  16..18  msg_id: u16
  18..20  msg_target_id: u16
  20..24  msg_user_id: u32
  24..32  msg_seq: u64
  32..40  msg_seq_done: u64
```

`msg_len` and `msg_prefix` are **gone** from the BeeMsg header — `frame_len` is the only length on
the wire, and the frame magic is the only prefix. Having exactly one length is what removes the
"two lengths that can disagree" class of bug. Body serialization is otherwise unchanged from
`legacy`, so only a new header struct is needed, not new message code.

12 + 28 = 40, the same as the legacy header, so the body starts at the same offset in both
protocols.

`0x53464742` interpreted as a legacy `msg_len` is 1,397,509,442, far beyond any sane buffer, so a
legacy peer fed new-protocol bytes fails its length check instead of waiting for data that never
arrives. In the other direction the magic check fails. Do **not** sniff the first bytes to accept
both — the protocol is configuration, not negotiation.

## Record layer (`encrypted` only)

With `frame_flags` bit 0 set, the payload is a sequence of Noise records. One record is exactly one
Noise transport message.

```
P = 65519    Noise MAXMSGLEN (65535) minus the 16-byte Poly1305 tag
T = 16       tag length
C = P + T = 65535
```

Records are **maximally filled**: for a plaintext of `L` bytes there are `k = ceil(L / P)` records,
the first `k-1` carrying exactly `P` plaintext bytes and the last carrying the remainder.

```
encode:  frame_len = 12 + L + k * T

decode:  payload = frame_len - 12               reject if frame_len < 12
         k = ceil(payload / C)
         L = payload - k * T
         reject unless L > (k - 1) * P          <- canonicality
         reject unless 28 <= L <= <buffer> - 12
```

There is no per-record length field; boundaries follow from `frame_len` alone. The canonicality
check is mandatory, not cosmetic: `frame_len = 12 + C + T` decodes to `k = 2, L = P`, which a
canonical sender would have emitted as one record. Rejecting non-maximal splits leaves a peer no freedom over
record boundaries.

**Nonces are implicit.** One Noise message per record, the counter starts at 0 per direction and is
never reset. There is no rekeying, and no out-of-order record decryption — a dropped or reordered
record must fail authentication rather than be tolerated. 2^64 records per direction is about
2.9e14 TiB at this record size; an implementation should error on exhaustion rather than wrap.

A sender should put the frame header and the first record in a single write. Records come out of a
scratch buffer, so writing the 12-byte header separately produces a write-write-read sequence that
Nagle plus the peer's delayed ACK can stall by tens of milliseconds.

## Key exchange (`authenticated` and `encrypted`)

Pattern: **`Noise_KK_25519_ChaChaPoly_SHA256`**.

KK means both peers hold a long-term X25519 keypair and know the other's public key in advance, so
one round trip yields a mutual, forward-secret session. The proof of identity is the `ss`
Diffie-Hellman term — only the two holders of those private keys can compute it, so there is no
signature to verify. Keys are pre-shared; distribution is out of scope here.

ChaChaPoly rather than AESGCM is deliberate: no AES-NI dependence, and X25519, ChaCha20-Poly1305 and
SHA-256 are all available in the Linux kernel crypto API and in libsodium. Switching to AES would be
a new pattern string and a wire break.

KK message 1 with an empty payload is 48 bytes; message 2 with a 2-byte payload is 50.

The responder must select the remote static key *before* it can process message 1, which is why the
initiator's public key travels in the clear as an identity selector.

```
HandshakeInit — frame_type = 1, flags = 0, frame_len = 100 (12 + 88)
  0..2    hs_version: u16 = 1
  2..4    modes: u16                     requested protection; bit0 = encrypt
  4..8    reserved: u32 = 0              senders MUST write zero
  8..40   init_static_pub: [u8; 32]      identity selector, cleartext
  40..88  noise_msg_1

HandshakeResponse — frame_type = 2, flags = 0, frame_len = 64 (12 + 52)
  0..2    hs_version: u16 = 1
  2..52   noise_msg_2, whose Noise payload is agreed_modes: u16

HandshakeReject — frame_type = 3, flags = 0, frame_len = 14 (12 + 2)
  0..2    reason: u16
```

**The Noise prologue is `HandshakeInit` payload bytes 0..40** — everything before `noise_msg_1` —
byte-exact on both sides. This is the single most security-critical detail in the design. Without
it, an on-path attacker flips one bit of the cleartext `modes` and silently downgrades a configured
`encrypted` connection to `authenticated`, which protects no traffic at all.

`agreed_modes` rides inside the encrypted Noise payload of message 2, so it is authenticated for
free. The initiator MUST abort unless it equals what it requested.

After `HandshakeResponse`, both sides enter Noise transport mode when `agreed_modes` bit 0 is set, and
otherwise discard the handshake state. A receiver MUST reject a Message frame whose `frame_flags`
bit 0 disagrees with what was negotiated.

The responder sends `HandshakeReject` instead of `HandshakeResponse` and then closes the
connection. The payload is the `reason` code alone — no free-form text, so nothing derived from what
the peer sent is ever echoed back. An initiator that does not recognise a `reason` MUST still report
the rejection rather than treat the frame as malformed; the codes are append only, so a newer
responder may send one it has never seen.

`reason` is a stable numeric enum; new codes append only.

| reason | meaning |
|---|---|
| 0 | unspecified |
| 1 | public key not registered |
| 2 | unsupported key exchange version |
| 3 | requested protection not permitted |
| 4 | key exchange failed to authenticate |
| 5 | the new BeeMsg protocol is disabled on this node (reserved, not currently sent) |

The key exchange belongs in the socket/channel layer, not the message dispatcher: it completes
before the first BeeMsg is read, so it consumes no message id and no handler sees it.

## Golden vectors

Static keys are the raw scalars `01…01` and `02…02`; ephemeral keys are `03…03` and `04…04`;
`modes = 1` (encrypt). Reproduced from `shared/src/conn/handshake.rs::test::wire_vectors`.

```
initiator static public  a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209
responder static public  ce8d3ad1ccb633ec7b70c17814a5c76ecd029685050d344745ba05870e587d59

HandshakeInit payload (88 bytes)
  0100010000000000
  a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209
  5dfedd3b6bd47f6fa28ee15d969d5bb0ea53774d488bdaf9df1c6e0124b3ef22
  7b407cca059a3e7dafbca3dcf1e4f296

HandshakeResponse payload (52 bytes)
  0100
  ac01b2209e86354fb853237b5de0f4fab13c7fcbf433a61c019369617fecf10b
  abc977b34d742620eed958cbc07786246910

first initiator-to-responder record, plaintext "BeeGFS"
  a65d0e3ec569e1f907a24588e763da3a1f9c1952021a
```

## What counts as a wire break

Coordinate across all trees before changing any of these:

1. The 12-byte frame header layout, the magic, or the meaning of any `frame_flags` bit.
2. The BeeMsg header layout.
3. The record size constants or the `frame_len` arithmetic, including the canonicality rule.
4. The Noise pattern string.
5. The prologue definition.
6. The nonce discipline — one Noise message per record, per-direction counters from 0, no rekey.
7. The set or numbering of protocol values, or the `modes` bit assignment.
8. `HandshakeReject` reason numbering.
9. The golden vectors.
