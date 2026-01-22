use aes_gcm::aead::AeadInPlace;
use aes_gcm::{self, Aes256Gcm, Key, KeyInit, Tag};
use anyhow::{Context, Result, anyhow};

const DUMMY_KEY: [u8; 32] = *b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0";
const DUMMY_NONCE: [u8; 12] = *b"BeeGFSNonce\0";

const AES_TAG_LEN: usize = 16;
/// Length of the cleartext prefix (magic + `msg_len`) at the start of a serialized message. These
/// bytes are authenticated as additional data but never encrypted: the receiver must read `msg_len`
/// to frame the message before it can decrypt. Mirrors `AES_MSG_CLEARTEXT_LEN` in the C++ codebase
/// and must equal [`crate::bee_msg::Header::ENCRYPTION_INFO_LEN`].
const AES_MSG_CLEARTEXT_LEN: usize = 8;
const ENCRYPT: bool = true;

fn build_nonce(counter: u64, mut nonce: [u8; 12]) -> [u8; 12] {
    for i in 0..8 {
        nonce[11 - i] ^= (counter >> (8 * i)) as u8;
    }

    nonce
}

/// Encrypts/authenticates a fully serialized message in place, using the nonce derived from
/// `counter`.
///
/// `buf` must be the **whole** serialized message starting at the cleartext prefix (magic +
/// length), with the last [`AES_TAG_LEN`] bytes reserved for the GCM tag. The
/// [`AES_MSG_CLEARTEXT_LEN`]-byte prefix is authenticated as additional data but left in the clear
/// (the receiver needs `msg_len` to frame the message before it can decrypt); when [`ENCRYPT`] is
/// enabled the remaining body is encrypted, otherwise the whole message is authenticate-only. The
/// tag is written into the final [`AES_TAG_LEN`] bytes. Must stay in sync with the C++
/// `aes256_encrypt`.
pub fn aes256_encrypt(counter: u64, buf: &mut [u8]) -> Result<()> {
    anyhow::ensure!(
        buf.len() >= AES_MSG_CLEARTEXT_LEN + AES_TAG_LEN,
        "Message too short to encrypt: {} bytes",
        buf.len()
    );

    let key = Key::<Aes256Gcm>::from_slice(&DUMMY_KEY);
    let cipher = Aes256Gcm::new(key);
    let nonce = build_nonce(counter, DUMMY_NONCE).into();

    let clear_len = buf.len() - AES_TAG_LEN;
    let (msg, tag_slot) = buf.split_at_mut(clear_len);

    let tag = if ENCRYPT {
        // Authenticate the cleartext prefix as additional data; encrypt the body after it.
        let (prefix, body) = msg.split_at_mut(AES_MSG_CLEARTEXT_LEN);
        cipher.encrypt_in_place_detached(&nonce, prefix, body)
    } else {
        // Authenticate-only: the entire message (minus tag) is additional data, nothing encrypted.
        cipher.encrypt_in_place_detached(&nonce, msg, &mut [])
    }
    .map_err(|err| anyhow!(err))
    .context("AES256 encryption failed")?;

    tag_slot.clone_from_slice(&tag);

    Ok(())
}

/// Decrypts and verifies a serialized message in place (inverse of [`aes256_encrypt`]).
pub fn aes256_decrypt(counter: u64, buf: &mut [u8]) -> Result<()> {
    anyhow::ensure!(
        buf.len() >= AES_MSG_CLEARTEXT_LEN + AES_TAG_LEN,
        "Message too short to decrypt: {} bytes",
        buf.len()
    );

    let key = Key::<Aes256Gcm>::from_slice(&DUMMY_KEY);
    let cipher = Aes256Gcm::new(key);
    let nonce = build_nonce(counter, DUMMY_NONCE).into();

    let clear_len = buf.len() - AES_TAG_LEN;
    let (msg, tag_slot) = buf.split_at_mut(clear_len);
    let tag = Tag::clone_from_slice(tag_slot);

    if ENCRYPT {
        // Cleartext prefix is additional data; the body after it is the ciphertext.
        let (prefix, body) = msg.split_at_mut(AES_MSG_CLEARTEXT_LEN);
        cipher.decrypt_in_place_detached(&nonce, prefix, body, &tag)
    } else {
        // Authenticate-only: the entire message (minus tag) is additional data.
        cipher.decrypt_in_place_detached(&nonce, msg, &mut [], &tag)
    }
    .map_err(|err| anyhow!(err))
    .context("AES256 decryption failed")?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn encrypt_decrypt() {
        const PLAIN: &[u8] = b"Hello BeeGFS!";
        let mut buf = PLAIN.to_vec();
        buf.extend([0u8; AES_TAG_LEN]);

        // Test correct encryption/decryption
        aes256_encrypt(0, buf.as_mut_slice()).unwrap();
        aes256_decrypt(0, buf.as_mut_slice()).unwrap();
        assert_eq!(PLAIN, &buf[..PLAIN.len()]);

        // Test wrong iv/nonce
        aes256_encrypt(0, buf.as_mut_slice()).unwrap();
        aes256_decrypt(1, buf.as_mut_slice()).unwrap_err();

        // Test wrong tag
        aes256_encrypt(0, buf.as_mut_slice()).unwrap();
        let tag_pos = buf.len() - 1;
        buf[tag_pos] ^= buf[tag_pos];
        aes256_decrypt(0, buf.as_mut_slice()).unwrap_err();

        // Test modified cipher
        aes256_encrypt(0, buf.as_mut_slice()).unwrap();
        buf[0] ^= buf[0];
        aes256_decrypt(0, buf.as_mut_slice()).unwrap_err();
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

        let mut buf: Vec<u8> = (0u8..24).collect();
        buf.extend([0u8; AES_TAG_LEN]);

        aes256_encrypt(7, buf.as_mut_slice()).unwrap();
        assert_eq!(buf, expected, "C++/Rust wire format diverged");

        // The prefix must remain in the clear regardless of mode.
        assert_eq!(&buf[..AES_MSG_CLEARTEXT_LEN], &[0, 1, 2, 3, 4, 5, 6, 7]);

        // And it must decrypt back with the same counter.
        aes256_decrypt(7, buf.as_mut_slice()).unwrap();
        assert_eq!(&buf[..24], (0u8..24).collect::<Vec<_>>().as_slice());
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
