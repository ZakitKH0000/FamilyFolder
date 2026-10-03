//! Шифрование того, что кладётся в облако. Ключ знают только устройства семьи.

use anyhow::{Result, anyhow};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::util::random_bytes;

/// nonce (24 байта) || шифртекст с меткой. `ad` привязывает данные к их месту в облаке.
pub fn seal(key: &[u8; 32], ad: &str, plain: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = random_bytes::<24>();
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad: ad.as_bytes() })
        .expect("encryption cannot fail");
    let mut out = Vec::with_capacity(24 + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

pub fn open(key: &[u8; 32], ad: &str, data: &[u8]) -> Result<Vec<u8>> {
    anyhow::ensure!(data.len() >= 24 + 16, crate::t!("err.cloud_corrupt"));
    let cipher = XChaCha20Poly1305::new(key.into());
    let (nonce, ct) = data.split_at(24);
    cipher
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: ad.as_bytes() })
        .map_err(|_| anyhow!(crate::t!("err.cloud_decrypt")))
}

/// Имя папки содержимого в облаке: не раскрывает сумму файла.
pub fn content_key(key: &[u8; 32], hash: &str) -> String {
    blake3::keyed_hash(key, hash.as_bytes()).to_hex()[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_tamper() {
        let key = [7u8; 32];
        let sealed = seal(&key, "data/x/0", b"hello");
        assert_eq!(open(&key, "data/x/0", &sealed).unwrap(), b"hello");
        assert!(open(&key, "data/x/1", &sealed).is_err());
        assert!(open(&[8u8; 32], "data/x/0", &sealed).is_err());
    }
}
