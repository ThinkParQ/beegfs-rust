use aes_gcm::aead::{AeadInPlace, OsRng};
use aes_gcm::{self, AeadCore, Aes256Gcm, Key, KeyInit, Nonce, Tag};
use anyhow::{Context, Result, anyhow};
use bee_serde_derive::BeeSerde;

const DUMMY_KEY: [u8; 32] = *b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0";

#[derive(Debug, Default, Clone, BeeSerde, PartialEq, Eq)]
pub struct AesEncryptionInfo {
    iv: [u8; 12],
    tag: [u8; 16],
}

pub fn aes256_encrypt(buf: &mut [u8]) -> Result<AesEncryptionInfo> {
    let key = Key::<Aes256Gcm>::from_slice(&DUMMY_KEY);
    let cipher = Aes256Gcm::new(key);

    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let tag = cipher
        .encrypt_in_place_detached(&nonce, &[], buf)
        .map_err(|err| anyhow!(err))
        .context("AES256 encryption failed")?;

    Ok(AesEncryptionInfo {
        iv: nonce.into(),
        tag: tag.into(),
    })
}

pub fn aes256_decrypt(info: &AesEncryptionInfo, buf: &mut [u8]) -> Result<()> {
    let key = Key::<Aes256Gcm>::from_slice(&DUMMY_KEY);
    let cipher = Aes256Gcm::new(key);

    let nonce = Nonce::from_slice(&info.iv);
    let tag = Tag::from_slice(&info.tag);

    cipher
        .decrypt_in_place_detached(nonce, &[], buf, tag)
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

        // Test correct encryption/decryption
        let info = aes256_encrypt(buf.as_mut_slice()).unwrap();
        aes256_decrypt(&info, buf.as_mut_slice()).unwrap();
        assert_eq!(PLAIN, buf);

        // Test wrong iv/nonce
        let mut info2 = aes256_encrypt(buf.as_mut_slice()).unwrap();
        info2.iv[0] ^= info2.iv[0];
        aes256_decrypt(&info2, buf.as_mut_slice()).unwrap_err();

        // Test wrong tag
        let mut info3 = aes256_encrypt(buf.as_mut_slice()).unwrap();
        info3.tag[0] ^= info3.tag[0];
        aes256_decrypt(&info3, buf.as_mut_slice()).unwrap_err();

        // Test modified cipher
        let info4 = aes256_encrypt(buf.as_mut_slice()).unwrap();
        buf[0] ^= buf[0];
        aes256_decrypt(&info4, buf.as_mut_slice()).unwrap_err();
    }
}
