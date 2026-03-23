use aes_gcm::aead::{AeadInPlace, OsRng};
use aes_gcm::{self, AeadCore, Aes256Gcm, Key, KeyInit, Nonce, Tag};
use anyhow::{Context, Result, anyhow};

const DUMMY_KEY: [u8; 32] = *b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0";
const AES_TAG_LEN: usize = 16;
const ENCRYPT: bool = true;
pub type AesIv = [u8; 12];

pub fn aes256_encrypt(buf: &mut [u8]) -> Result<AesIv> {
    let clear_len = buf.len() - AES_TAG_LEN;

    let key = Key::<Aes256Gcm>::from_slice(&DUMMY_KEY);
    let cipher = Aes256Gcm::new(key);

    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let tag = if ENCRYPT {
        cipher.encrypt_in_place_detached(&nonce, &[], &mut buf[..clear_len])
    } else {
        cipher.encrypt_in_place_detached(&nonce, &buf[..clear_len], &mut [])
    }
    .map_err(|err| anyhow!(err))
    .context("AES256 encryption failed")?;

    buf[clear_len..clear_len + AES_TAG_LEN].clone_from_slice(&tag);

    Ok(nonce.into())
}

pub fn aes256_decrypt(iv: &AesIv, buf: &mut [u8]) -> Result<()> {
    let clear_len = buf.len() - AES_TAG_LEN;

    let key = Key::<Aes256Gcm>::from_slice(&DUMMY_KEY);
    let cipher = Aes256Gcm::new(key);

    let nonce = Nonce::from_slice(iv);
    let tag = Tag::clone_from_slice(&buf[clear_len..]);

    if ENCRYPT {
        cipher.decrypt_in_place_detached(nonce, &[], &mut buf[..clear_len], &tag)
    } else {
        cipher.decrypt_in_place_detached(nonce, &buf[..clear_len], &mut [], &tag)
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
        let iv = aes256_encrypt(buf.as_mut_slice()).unwrap();
        aes256_decrypt(&iv, buf.as_mut_slice()).unwrap();
        assert_eq!(PLAIN, &buf[..PLAIN.len()]);

        // Test wrong iv/nonce
        let mut iv = aes256_encrypt(buf.as_mut_slice()).unwrap();
        iv[0] ^= iv[0];
        aes256_decrypt(&iv, buf.as_mut_slice()).unwrap_err();

        // Test wrong tag
        let iv = aes256_encrypt(buf.as_mut_slice()).unwrap();
        let tag_pos = buf.len() - 1;
        buf[tag_pos] ^= buf[tag_pos];
        aes256_decrypt(&iv, buf.as_mut_slice()).unwrap_err();

        // Test modified cipher
        let iv = aes256_encrypt(buf.as_mut_slice()).unwrap();
        buf[0] ^= buf[0];
        aes256_decrypt(&iv, buf.as_mut_slice()).unwrap_err();
    }
}
