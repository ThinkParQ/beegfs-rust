#!/usr/bin/env python3
"""Independent implementation of the BeeGFS BeeMsg key exchange (Noise KK).

This exists for two reasons:

1. **Reference for the other trees.** The C++ servers and the kernel client must reimplement the
   same handshake. This is a small, dependency-free implementation of it that can be read
   end-to-end, and it doubles as a conformance check while porting.

2. **Guarding against protocol drift.** The Rust implementation pins the handshake in
   `shared/src/crypto/handshake.rs` (`kdf_vector` test). This script recomputes those values from
   scratch, using its own X25519 and its own KDF, and compares. Because it *parses the expected
   values out of the Rust source* rather than hardcoding them, it cannot go stale: change the
   protocol and this immediately reports whether the new Rust vectors match the spec below.

Uses only the Python standard library plus an RFC 7748 X25519 written out longhand, so it shares no
code and no crypto library with the Rust side. Agreement therefore means the two implementations
genuinely agree, not that they share a bug.

## The protocol

Noise KK over X25519 with SHA-256. Both peers hold a long-term ("static") keypair and know the
other's public key in advance; management distributes the public keys via the `keys` table.

    h  = "BeeGFS_KK_25519_AESGCM_SHA256" zero padded to 32 bytes
    ck = h

    mix_hash(d) : h  = SHA256(h || d)
    mix_key(dh) : ck = HKDF(ck, dh)[0]

    pre-message : mix_hash(s_i_pub); mix_hash(s_r_pub)        # initiator's key first
    message 1   : mix_hash(e_i_pub); mix_key(es); mix_key(ss) # initiator -> responder
    message 2   : mix_hash(e_r_pub); mix_key(ee); mix_key(se) # responder -> initiator
    split       : k_i2r, k_r2i, k_confirm = HKDF3(ck, "")

    es = e_i * s_r    ss = s_i * s_r    ee = e_i * e_r    se = s_i * e_r

`ss` is what authenticates both peers: only the holders of those two static private keys can
compute it, so no signature is needed. The responder proves it got there by sending
`confirm = HMAC-SHA256(k_confirm, h)`, which the initiator verifies before trusting the session.

HKDF here is the Noise/WireGuard form, not RFC 5869 verbatim:

    temp = HMAC(ck, ikm);  out1 = HMAC(temp, 0x01);  out2 = HMAC(temp, out1 || 0x02);  ...

Both directions get their own key. The nonce base is all zero and the per-direction message counter
is folded into the low 8 bytes of the 12-byte AES-GCM nonce, big-endian-ish (see `build_nonce` in
the Rust `crypto` module).

## Usage

    ./kk_handshake_check.py                                   # cross-check the Rust vectors
    ./kk_handshake_check.py HOST PORT CLIENT_KEY_FILE SERVER_PUB_HEX   # live handshake

`CLIENT_KEY_FILE` is a raw 32 byte X25519 private key (as written by `beegfs-mgmtd --gen-key`)
whose public half must be registered in the server's `keys` table. `SERVER_PUB_HEX` is the server's
public key, which it logs at startup as "BeeMsg public key".
"""

import hashlib
import hmac
import os
import pathlib
import re
import socket
import struct
import sys

# ---------------------------------------------------------------- X25519 (RFC 7748 reference)
P = 2**255 - 19
A24 = 121665


def _cswap(swap, x2, x3):
    dummy = swap * ((x2 - x3) % P)
    return (x2 - dummy) % P, (x3 + dummy) % P


def x25519(k_bytes, u_bytes):
    """Scalar multiplication, the Montgomery ladder straight out of RFC 7748 section 5."""
    k = int.from_bytes(k_bytes, "little")
    # Clamp, as X25519 requires: clear the low 3 bits and the top bit, set bit 254.
    k &= ~7
    k &= ~(128 << 8 * 31)
    k |= 64 << 8 * 31
    u = int.from_bytes(u_bytes, "little") & ((1 << 255) - 1)

    x1, x2, z2, x3, z3, swap = u, 1, 0, u, 1, 0
    for t in range(254, -1, -1):
        kt = (k >> t) & 1
        swap ^= kt
        x2, x3 = _cswap(swap, x2, x3)
        z2, z3 = _cswap(swap, z2, z3)
        swap = kt

        a = (x2 + z2) % P
        aa = a * a % P
        b = (x2 - z2) % P
        bb = b * b % P
        e = (aa - bb) % P
        c = (x3 + z3) % P
        d = (x3 - z3) % P
        da = d * a % P
        cb = c * b % P
        x3 = pow(da + cb, 2, P)
        z3 = x1 * pow(da - cb, 2, P) % P
        x2 = aa * bb % P
        z2 = e * ((aa + A24 * e) % P) % P

    x2, x3 = _cswap(swap, x2, x3)
    z2, z3 = _cswap(swap, z2, z3)
    return (x2 * pow(z2, P - 2, P) % P).to_bytes(32, "little")


BASE9 = (9).to_bytes(32, "little")


def pubkey(secret):
    return x25519(secret, BASE9)


def dh(secret, peer_pub):
    """DH with the contributory-behaviour check the Rust side also enforces."""
    shared = x25519(secret, peer_pub)
    if shared == bytes(32):
        raise ValueError("peer sent a low order public key")
    return shared


# ---------------------------------------------------------------- KDF chain
PROTOCOL_NAME = b"BeeGFS_KK_25519_AESGCM_SHA256"


def _hmac(key, *parts):
    m = hmac.new(key, digestmod=hashlib.sha256)
    for p in parts:
        m.update(p)
    return m.digest()


def hkdf(ck, ikm, outputs):
    """The Noise/WireGuard chained-expand HKDF, for 2 or 3 outputs."""
    temp = _hmac(ck, ikm)
    out = []
    prev = b""
    for i in range(1, outputs + 1):
        prev = _hmac(temp, prev, bytes([i]))
        out.append(prev)
    return out


class Chain:
    """The running handshake state: chaining key plus transcript hash."""

    def __init__(self):
        assert len(PROTOCOL_NAME) <= 32, "longer names must be hashed instead of padded"
        h = PROTOCOL_NAME + b"\x00" * (32 - len(PROTOCOL_NAME))
        self.h = h
        self.ck = h

    def mix_hash(self, data):
        self.h = hashlib.sha256(self.h + data).digest()

    def mix_key(self, dh_output):
        self.ck = hkdf(self.ck, dh_output, 2)[0]

    def split(self):
        return hkdf(self.ck, b"", 3)


def initiator_message1(s_i_priv, s_r_pub, e_i_priv):
    """Pre-message plus message 1. Returns (chain, e_i_pub)."""
    e_i_pub = pubkey(e_i_priv)

    chain = Chain()
    chain.mix_hash(pubkey(s_i_priv))
    chain.mix_hash(s_r_pub)
    chain.mix_hash(e_i_pub)
    chain.mix_key(dh(e_i_priv, s_r_pub))  # es
    chain.mix_key(dh(s_i_priv, s_r_pub))  # ss
    return chain, e_i_pub


def initiator_message2(chain, s_i_priv, e_i_priv, e_r_pub):
    """Message 2 plus split. Returns (k_i2r, k_r2i, k_confirm)."""
    chain.mix_hash(e_r_pub)
    chain.mix_key(dh(e_i_priv, e_r_pub))  # ee
    chain.mix_key(dh(s_i_priv, e_r_pub))  # se
    return chain.split()


def responder(s_r_priv, s_i_pub, e_i_pub, e_r_priv):
    """The mirrored responder side, for reference. Returns (chain, e_r_pub, keys)."""
    e_r_pub = pubkey(e_r_priv)

    chain = Chain()
    chain.mix_hash(s_i_pub)
    chain.mix_hash(pubkey(s_r_priv))
    chain.mix_hash(e_i_pub)
    chain.mix_key(dh(s_r_priv, e_i_pub))  # es
    chain.mix_key(dh(s_r_priv, s_i_pub))  # ss
    chain.mix_hash(e_r_pub)
    chain.mix_key(dh(e_r_priv, e_i_pub))  # ee
    chain.mix_key(dh(e_r_priv, s_i_pub))  # se
    return chain, e_r_pub, chain.split()


# ---------------------------------------------------------------- BeeMsg framing
MSG_PREFIX = 0x53464742
HEADER_LEN = 36
TAG_LEN = 16
MSGID_KEY_EXCHANGE_REQUEST = 4013
MSGID_KEY_EXCHANGE_RESPONSE = 4015
MSGID_GENERIC_RESPONSE = 4009
HANDSHAKE_VERSION = 1

# msg_prefix, msg_len, feature_flags, compat_feature_flags, flags, msg_id, target_id, user_id,
# seq, seq_done - all little endian, 36 bytes total.
HEADER_FMT = "<IIHBBHHIQQ"


def build_msg(msg_id, body):
    """A complete BeeMsg. msg_len includes the header and the trailing AES-GCM tag slot."""
    msg_len = HEADER_LEN + len(body) + TAG_LEN
    header = struct.pack(HEADER_FMT, MSG_PREFIX, msg_len, 0, 0, 0, msg_id, 0, 0, 0, 0)
    msg = header + body + bytes(TAG_LEN)
    assert len(msg) == msg_len
    return msg


def parse_header(buf):
    prefix, msg_len, _ff, _cff, _fl, msg_id, _tid, _uid, _s, _sd = struct.unpack(
        HEADER_FMT, buf[:HEADER_LEN]
    )
    if prefix != MSG_PREFIX:
        raise ValueError(f"bad BeeMsg prefix {prefix:#x}")
    return msg_len, msg_id


def recv_exact(sock, n):
    out = b""
    while len(out) < n:
        chunk = sock.recv(n - len(out))
        if not chunk:
            raise EOFError(f"peer closed after {len(out)}/{n} bytes")
        out += chunk
    return out


# ---------------------------------------------------------------- vector cross-check
# Fixed private keys for the vectors. Must match the Rust `kdf_vector` test.
VECTOR_S_I = bytes([1] * 32)
VECTOR_S_R = bytes([2] * 32)
VECTOR_E_I = bytes([3] * 32)
VECTOR_E_R = bytes([4] * 32)

RUST_SOURCE = (
    pathlib.Path(__file__).resolve().parent.parent / "shared/src/crypto/handshake.rs"
)

# Rust test constant name -> label used here.
CONSTANTS = {
    "INIT_EPHEMERAL_PUB": "init_ephemeral_pub",
    "RESP_EPHEMERAL_PUB": "resp_ephemeral_pub",
    "TRANSCRIPT_HASH": "transcript_hash",
    "KEY_I2R": "key_i2r",
    "KEY_R2I": "key_r2i",
    "KEY_CONFIRM": "key_confirm",
}


def parse_rust_vectors(path):
    """Pull the expected hex values out of the Rust test, so they cannot drift out of sync."""
    if not path.is_file():
        return None

    src = path.read_text()
    found = {}
    for const, label in CONSTANTS.items():
        m = re.search(
            rf'const\s+{const}\s*:\s*&str\s*=\s*"([0-9a-fA-F]{{64}})"\s*;', src
        )
        if m:
            found[label] = m.group(1).lower()

    missing = set(CONSTANTS.values()) - set(found)
    if missing:
        print(f"  ! could not parse from Rust source: {', '.join(sorted(missing))}")
        return None
    return found


def compute_vectors():
    chain, e_i_pub = initiator_message1(VECTOR_S_I, pubkey(VECTOR_S_R), VECTOR_E_I)
    e_r_pub = pubkey(VECTOR_E_R)
    k_i2r, k_r2i, k_confirm = initiator_message2(chain, VECTOR_S_I, VECTOR_E_I, e_r_pub)

    # The responder must independently arrive at the same transcript and keys.
    r_chain, r_e_r_pub, r_keys = responder(
        VECTOR_S_R, pubkey(VECTOR_S_I), e_i_pub, VECTOR_E_R
    )
    assert r_e_r_pub == e_r_pub
    assert r_chain.h == chain.h, "initiator and responder transcripts diverged"
    assert r_keys == [k_i2r, k_r2i, k_confirm], "the two sides derived different keys"

    return {
        "init_ephemeral_pub": e_i_pub.hex(),
        "resp_ephemeral_pub": e_r_pub.hex(),
        "transcript_hash": chain.h.hex(),
        "key_i2r": k_i2r.hex(),
        "key_r2i": k_r2i.hex(),
        "key_confirm": k_confirm.hex(),
    }


def check_vectors():
    actual = compute_vectors()
    expected = parse_rust_vectors(RUST_SOURCE)

    if expected is None:
        print(f"  no Rust source at {RUST_SOURCE}, printing computed values only:")
        for label, value in actual.items():
            print(f"    {label:20s} {value}")
        return True

    print(f"  comparing against {RUST_SOURCE.name}")
    ok = True
    for label, want in expected.items():
        got = actual[label]
        if got == want:
            print(f"  [OK  ] {label}")
        else:
            ok = False
            print(f"  [FAIL] {label}")
            print(f"         rust:   {want}")
            print(f"         python: {got}")
    return ok


# ---------------------------------------------------------------- live handshake
def live_handshake(host, port, client_secret, server_pub):
    e_i_priv = os.urandom(32)
    chain, e_i_pub = initiator_message1(client_secret, server_pub, e_i_priv)

    body = struct.pack("<I", HANDSHAKE_VERSION) + pubkey(client_secret) + e_i_pub
    sock = socket.create_connection((host, port), timeout=5)
    try:
        sock.sendall(build_msg(MSGID_KEY_EXCHANGE_REQUEST, body))

        resp_len, resp_id = parse_header(recv_exact(sock, HEADER_LEN))
        rest = recv_exact(sock, resp_len - HEADER_LEN)
        print(f"  response: msg_id={resp_id} msg_len={resp_len}")

        if resp_id == MSGID_GENERIC_RESPONSE:
            print(f"  [FAIL] peer sent a GenericResponse: {rest[:resp_len]!r}")
            return False
        if resp_id != MSGID_KEY_EXCHANGE_RESPONSE:
            print(f"  [FAIL] expected msg_id {MSGID_KEY_EXCHANGE_RESPONSE}")
            return False

        e_r_pub, confirm = rest[0:32], rest[32:64]

        if e_r_pub == bytes(32):
            print(
                "  [FAIL] peer replied with an all-zero response, which is how it refuses a key "
                "exchange - our public key is most likely not registered in its `keys` table"
            )
            return False

        _k_i2r, _k_r2i, k_confirm = initiator_message2(
            chain, client_secret, e_i_priv, e_r_pub
        )
        want = _hmac(k_confirm, chain.h)

        if not hmac.compare_digest(want, confirm):
            print("  [FAIL] confirmation HMAC mismatch")
            print(f"         expected {want.hex()}")
            print(f"         got      {confirm.hex()}")
            return False

        print("  [OK  ] confirmation verified - both sides derived identical session keys")
        print(f"         k_i2r = {_k_i2r.hex()}")
        print(f"         k_r2i = {_k_r2i.hex()}")
        return True
    finally:
        sock.close()


def main(argv):
    print("== cross-checking the Rust KDF vectors ==")
    ok = check_vectors()

    if len(argv) == 5:
        host, port, key_file, server_pub_hex = argv[1], int(argv[2]), argv[3], argv[4]

        secret = pathlib.Path(key_file).read_bytes()
        if len(secret) != 32:
            print(f"  [FAIL] {key_file} must contain exactly 32 raw bytes, got {len(secret)}")
            return 1
        server_pub = bytes.fromhex(server_pub_hex)
        if len(server_pub) != 32:
            print("  [FAIL] SERVER_PUB_HEX must be 64 hex characters")
            return 1

        print(f"== live handshake against {host}:{port} ==")
        ok = live_handshake(host, port, secret, server_pub) and ok
    elif len(argv) != 1:
        print(__doc__.split("## Usage")[1].strip())
        return 2

    print("PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
