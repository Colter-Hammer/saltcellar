//! `BUDGET_SECRET_KEY` and the access URL sealed with it.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload, array::Array};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::AccessUrl;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
/// The first byte of every sealed value, so a later format can be told from this one *(agent)*.
const VERSION: u8 = 1;
/// Bound into every seal, so a value sealed for another purpose (kept raw responses) won't open
/// as an access URL *(agent)*.
const CONTEXT: &[u8] = b"saltcellar connection access url";

/// The 32 bytes `BUDGET_SECRET_KEY` holds. Never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretKey([u8; KEY_LEN]);

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

/// A `BUDGET_SECRET_KEY` that isn't 32 bytes of base64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("isn't 32 random bytes in base64 (make one with `openssl rand -base64 32`)")]
pub struct KeyError;

impl SecretKey {
    /// Reads the key from its base64 text.
    ///
    /// # Errors
    /// If the text isn't base64 of exactly 32 bytes.
    pub fn from_base64(text: &str) -> Result<SecretKey, KeyError> {
        let bytes = STANDARD.decode(text.trim()).map_err(|_| KeyError)?;
        let bytes: [u8; KEY_LEN] = bytes.try_into().map_err(|_| KeyError)?;
        Ok(SecretKey(bytes))
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(&Array::from(self.0))
    }
}

/// Why a sealed value didn't open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// The key is not the one it was sealed with, or the value was changed.
    #[error("the sealed value doesn't open with this key")]
    WrongKey,
    /// Not a value this version sealed.
    #[error("the sealed value isn't in a format this version reads")]
    Unreadable,
}

/// The OS had no random bytes for a nonce, so nothing was sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("can't seal the secret: no random bytes from the OS")]
pub struct SealError;

/// Seals the access URL under `key`: a version byte, a fresh random 24-byte nonce, then the
/// XChaCha20-Poly1305 ciphertext with its tag.
///
/// # Errors
/// [`SealError`] if the OS has no random bytes for the nonce.
pub fn seal(key: &SecretKey, url: &AccessUrl) -> Result<Vec<u8>, SealError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| SealError)?;
    let sealed = key
        .cipher()
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: url.expose().as_bytes(),
                aad: CONTEXT,
            },
        )
        .map_err(|_| SealError)?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + sealed.len());
    out.push(VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Opens a value [`seal`] made.
///
/// # Errors
/// [`OpenError::WrongKey`] if `key` isn't the one it was sealed with, [`OpenError::Unreadable`]
/// if it isn't a sealed value at all.
pub fn open(key: &SecretKey, sealed: &[u8]) -> Result<AccessUrl, OpenError> {
    let [VERSION, rest @ ..] = sealed else {
        return Err(OpenError::Unreadable);
    };
    let (nonce, ciphertext) = rest
        .split_at_checked(NONCE_LEN)
        .ok_or(OpenError::Unreadable)?;
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| OpenError::Unreadable)?;
    let plain = key
        .cipher()
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: ciphertext,
                aad: CONTEXT,
            },
        )
        .map_err(|_| OpenError::WrongKey)?;
    let text = String::from_utf8(plain).map_err(|_| OpenError::Unreadable)?;
    Ok(AccessUrl::new(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> SecretKey {
        SecretKey([byte; KEY_LEN])
    }

    const URL: &str = "https://demo-user:demo-pass@bridge.example.test/simplefin";

    #[test]
    fn a_sealed_url_opens_with_the_same_key() {
        let sealed = seal(&key(1), &AccessUrl::new(URL.to_owned())).unwrap();
        assert_eq!(open(&key(1), &sealed).unwrap().expose(), URL);
    }

    #[test]
    fn the_sealed_bytes_do_not_hold_the_url() {
        let sealed = seal(&key(1), &AccessUrl::new(URL.to_owned())).unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains("demo-pass"));
        assert!(!sealed.windows(9).any(|w| w == b"demo-pass"));
    }

    #[test]
    fn each_seal_uses_a_new_nonce() {
        let url = AccessUrl::new(URL.to_owned());
        assert_ne!(seal(&key(1), &url).unwrap(), seal(&key(1), &url).unwrap());
    }

    #[test]
    fn another_key_does_not_open_it() {
        let sealed = seal(&key(1), &AccessUrl::new(URL.to_owned())).unwrap();
        assert_eq!(open(&key(2), &sealed).unwrap_err(), OpenError::WrongKey);
    }

    #[test]
    fn a_changed_value_does_not_open() {
        let mut sealed = seal(&key(1), &AccessUrl::new(URL.to_owned())).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert_eq!(open(&key(1), &sealed).unwrap_err(), OpenError::WrongKey);
    }

    #[test]
    fn garbage_is_unreadable() {
        assert_eq!(open(&key(1), b"").unwrap_err(), OpenError::Unreadable);
        assert_eq!(
            open(&key(1), &[1, 2, 3]).unwrap_err(),
            OpenError::Unreadable
        );
        assert_eq!(open(&key(1), &[9; 60]).unwrap_err(), OpenError::Unreadable);
    }

    #[test]
    fn the_key_reads_32_bytes_of_base64_and_prints_nothing() {
        let text = STANDARD.encode([7u8; 32]);
        let parsed = SecretKey::from_base64(&format!("{text}\n")).unwrap();
        assert_eq!(parsed, key(7));
        assert_eq!(format!("{parsed:?}"), "SecretKey(<redacted>)");
        assert!(!format!("{parsed:?}").contains(&text));
        assert_eq!(SecretKey::from_base64("c2hvcnQ=").unwrap_err(), KeyError);
        assert_eq!(SecretKey::from_base64("not base64!").unwrap_err(), KeyError);
        assert_eq!(SecretKey::from_base64("").unwrap_err(), KeyError);
    }
}
