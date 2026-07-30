//! Symmetric protection of serialized BeeMsgs.
//!
//! A [`Session`] holds the symmetric state for one communication channel: an AES-256-GCM key per
//! direction, established by the key exchange in [`handshake`]. Streams own their session (see
//! [`crate::conn::stream::Stream`]) and pass their per-direction message counter into
//! [`Session::encrypt`] / [`Session::decrypt`] to derive the nonce.

pub mod handshake;

use aes_gcm::aead::AeadInPlace;
use aes_gcm::{self, Aes256Gcm, Key, KeyInit, Nonce, Tag};
use anyhow::{Context, Result, anyhow};

/// The legacy fixed key, used for datagrams when no authentication secret is configured.
///
/// Predates the key exchange and provides no confidentiality whatsoever - the value is right here
/// in the source. Only still reachable on the UDP path, see [`Session::for_datagrams`].
const LEGACY_KEY: [u8; 32] = *b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0";
/// The legacy fixed nonce base, counterpart to [`LEGACY_KEY`].
const LEGACY_NONCE: [u8; 12] = *b"BeeGFSNonce\0";

pub const AES_TAG_LEN: usize = 16;
/// Length of the cleartext prefix (magic + `msg_len`) at the start of a serialized message. These
/// bytes are authenticated as additional data but never encrypted: the receiver must read `msg_len`
/// to frame the message before it can decrypt. Mirrors `AES_MSG_CLEARTEXT_LEN` in the C++ codebase
/// and must equal [`crate::bee_msg::Header::ENCRYPTION_INFO_LEN`].
const AES_MSG_CLEARTEXT_LEN: usize = 8;
const ENCRYPT: bool = true;

/// The symmetric key for one direction of a channel.
///
/// The AES key schedule is computed once, when the key is installed, rather than per message.
pub struct DirKey {
    cipher: Aes256Gcm,
    nonce_base: [u8; 12],
}

impl DirKey {
    pub fn new(key: &[u8; 32], nonce_base: [u8; 12]) -> Self {
        Self {
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)),
            nonce_base,
        }
    }

    /// Derives the nonce for `counter` by folding it into the low bytes of the nonce base.
    ///
    /// Must stay in sync with the C++ implementation.
    fn nonce(&self, counter: u64) -> Nonce<typenum::U12> {
        let mut nonce = self.nonce_base;

        for i in 0..8 {
            nonce[11 - i] ^= (counter >> (8 * i)) as u8;
        }

        nonce.into()
    }
}

/// The symmetric state of one communication channel.
///
/// Holds a separate key per direction. This is not cosmetic: the send and receive counters of a
/// channel both start at zero, so a single shared key would reuse the (key, nonce) pair on the
/// first message in each direction - a total break of AES-GCM. Distinct keys also make reflected
/// ciphertext fail authentication for free.
pub struct Session {
    /// Key for messages *we* send.
    tx: DirKey,
    /// Key for messages we receive.
    rx: DirKey,
}

impl std::fmt::Debug for Session {
    /// Deliberately opaque: a [`Session`] is held by types that derive [`Debug`], and key material
    /// must never reach a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session(..)")
    }
}

impl Session {
    /// Builds a session from two directional keys.
    pub fn new(tx: DirKey, rx: DirKey) -> Self {
        Self { tx, rx }
    }

    /// Builds a session that uses the same key in both directions.
    ///
    /// Only correct where the channel is not point-to-point and every peer must be able to read
    /// every other peer's messages - which in practice means datagrams, see
    /// [`Session::for_datagrams`]. Do **not** use this for streams.
    pub fn shared(key: &[u8; 32], nonce_base: [u8; 12]) -> Self {
        Self::new(DirKey::new(key, nonce_base), DirKey::new(key, nonce_base))
    }

    /// The process-wide session used for UDP datagrams.
    ///
    /// # Known weakness
    /// Datagrams are connectionless, so there is no handshake and no per-channel key. Every node
    /// shares one key and the counter is always zero (see the UDP paths in
    /// [`crate::conn::outgoing`] and [`crate::conn::incoming`]), which means **every datagram ever
    /// sent reuses the same (key, nonce) pair**. That breaks AES-GCM confidentiality and permits
    /// forgery. Datagram traffic must be treated as neither confidential nor authentic until the
    /// UDP path is removed.
    ///
    /// The key is derived from the authentication secret so that it is at least not a compile-time
    /// constant; note that this caps its strength at the 64 bits the secret carries.
    pub fn for_datagrams(auth_secret: Option<crate::types::AuthSecret>) -> Self {
        match auth_secret {
            Some(secret) => {
                let key = handshake::derive_datagram_key(secret);
                Self::shared(&key, LEGACY_NONCE)
            }
            None => Self::shared(&LEGACY_KEY, LEGACY_NONCE),
        }
    }

    /// Encrypts/authenticates a fully serialized message in place, using the nonce derived from
    /// `counter`.
    ///
    /// `buf` must be the **whole** serialized message starting at the cleartext prefix (magic +
    /// length), with the last [`AES_TAG_LEN`] bytes reserved for the GCM tag. The
    /// [`AES_MSG_CLEARTEXT_LEN`]-byte prefix is authenticated as additional data but left in the
    /// clear (the receiver needs `msg_len` to frame the message before it can decrypt); when
    /// [`ENCRYPT`] is enabled the remaining body is encrypted, otherwise the whole message is
    /// authenticate-only. The tag is written into the final [`AES_TAG_LEN`] bytes. Must stay in
    /// sync with the C++ `aes256_encrypt`.
    pub fn encrypt(&self, counter: u64, buf: &mut [u8]) -> Result<()> {
        anyhow::ensure!(
            buf.len() >= AES_MSG_CLEARTEXT_LEN + AES_TAG_LEN,
            "Message too short to encrypt: {} bytes",
            buf.len()
        );

        let nonce = self.tx.nonce(counter);

        let clear_len = buf.len() - AES_TAG_LEN;
        let (msg, tag_slot) = buf.split_at_mut(clear_len);

        let tag = if ENCRYPT {
            // Authenticate the cleartext prefix as additional data; encrypt the body after it.
            let (prefix, body) = msg.split_at_mut(AES_MSG_CLEARTEXT_LEN);
            self.tx
                .cipher
                .encrypt_in_place_detached(&nonce, prefix, body)
        } else {
            // Authenticate-only: the entire message (minus tag) is additional data, nothing
            // encrypted.
            self.tx
                .cipher
                .encrypt_in_place_detached(&nonce, msg, &mut [])
        }
        .map_err(|err| anyhow!(err))
        .context("AES256 encryption failed")?;

        tag_slot.clone_from_slice(&tag);

        Ok(())
    }

    /// Decrypts and verifies a serialized message in place (inverse of [`Session::encrypt`]).
    pub fn decrypt(&self, counter: u64, buf: &mut [u8]) -> Result<()> {
        anyhow::ensure!(
            buf.len() >= AES_MSG_CLEARTEXT_LEN + AES_TAG_LEN,
            "Message too short to decrypt: {} bytes",
            buf.len()
        );

        let nonce = self.rx.nonce(counter);

        let clear_len = buf.len() - AES_TAG_LEN;
        let (msg, tag_slot) = buf.split_at_mut(clear_len);
        let tag = Tag::clone_from_slice(tag_slot);

        if ENCRYPT {
            // Cleartext prefix is additional data; the body after it is the ciphertext.
            let (prefix, body) = msg.split_at_mut(AES_MSG_CLEARTEXT_LEN);
            self.rx
                .cipher
                .decrypt_in_place_detached(&nonce, prefix, body, &tag)
        } else {
            // Authenticate-only: the entire message (minus tag) is additional data.
            self.rx
                .cipher
                .decrypt_in_place_detached(&nonce, msg, &mut [], &tag)
        }
        .map_err(|err| anyhow!(err))
        .context("AES256 decryption failed")?;

        Ok(())
    }
}

/// Re-exported from `aes-gcm`s generic-array stack so [`DirKey::nonce`] can name the nonce type.
mod typenum {
    pub use aes_gcm::aes::cipher::consts::U12;
}

#[cfg(test)]
mod test {
    use super::*;

    /// The session used by the tests below: the legacy fixed key/nonce in both directions, which is
    /// what the cross-tree vectors were generated against.
    fn legacy_session() -> Session {
        Session::shared(&LEGACY_KEY, LEGACY_NONCE)
    }

    #[test]
    fn encrypt_decrypt() {
        const PLAIN: &[u8] = b"Hello BeeGFS!";
        let session = legacy_session();
        let mut buf = PLAIN.to_vec();
        buf.extend([0u8; AES_TAG_LEN]);

        // Test correct encryption/decryption
        session.encrypt(0, buf.as_mut_slice()).unwrap();
        session.decrypt(0, buf.as_mut_slice()).unwrap();
        assert_eq!(PLAIN, &buf[..PLAIN.len()]);

        // Test wrong iv/nonce
        session.encrypt(0, buf.as_mut_slice()).unwrap();
        session.decrypt(1, buf.as_mut_slice()).unwrap_err();

        // Test wrong tag
        session.encrypt(0, buf.as_mut_slice()).unwrap();
        let tag_pos = buf.len() - 1;
        buf[tag_pos] ^= buf[tag_pos];
        session.decrypt(0, buf.as_mut_slice()).unwrap_err();

        // Test modified cipher
        session.encrypt(0, buf.as_mut_slice()).unwrap();
        buf[0] ^= buf[0];
        session.decrypt(0, buf.as_mut_slice()).unwrap_err();
    }

    /// A message encrypted under the send key must not verify under the receive key of the same
    /// session. This is what makes reflected ciphertext fail authentication, and it is the reason
    /// the two directions must never share a key.
    #[test]
    fn directional_keys_do_not_interchange() {
        let session = Session::new(
            DirKey::new(&[1u8; 32], [0u8; 12]),
            DirKey::new(&[2u8; 32], [0u8; 12]),
        );

        let mut buf = b"Hello BeeGFS!".to_vec();
        buf.extend([0u8; AES_TAG_LEN]);

        session.encrypt(0, buf.as_mut_slice()).unwrap();
        // Same session, same counter, but the rx key differs from the tx key.
        session.decrypt(0, buf.as_mut_slice()).unwrap_err();
    }

    /// Cross-tree wire vector: the exact bytes produced by the C++ `aes256_encrypt` (OpenSSL) for
    /// the same key/nonce/counter/layout must match this implementation, otherwise Rust and the
    /// C++ servers/client cannot interoperate. Vectors generated from the C++ reference for an
    /// 8-byte prefix (0..7) + 16-byte body (8..23) + 16-byte tag, counter = 7, in both modes.
    #[test]
    fn cross_tree_vector() {
        // The prefix (0..7) is authenticated but always cleartext; in ENCRYPT mode the body (8..23)
        // is encrypted, otherwise the whole message is authenticate-only.
        let expected = if ENCRYPT {
            hex_to_bytes(
                "0001020304050607dd1f81206467f5574787b94f62e38daf61024021a12672ce6bc39fc8f055513e",
            )
        } else {
            hex_to_bytes(
                "000102030405060708090a0b0c0d0e0f1011121314151617f244c6f1264f3efc21c51522decf3493",
            )
        };

        let session = legacy_session();
        let mut buf: Vec<u8> = (0u8..24).collect();
        buf.extend([0u8; AES_TAG_LEN]);

        session.encrypt(7, buf.as_mut_slice()).unwrap();
        assert_eq!(buf, expected, "C++/Rust wire format diverged");

        // The prefix must remain in the clear regardless of mode.
        assert_eq!(&buf[..AES_MSG_CLEARTEXT_LEN], &[0, 1, 2, 3, 4, 5, 6, 7]);

        // And it must decrypt back with the same counter.
        session.decrypt(7, buf.as_mut_slice()).unwrap();
        assert_eq!(&buf[..24], (0u8..24).collect::<Vec<_>>().as_slice());
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
