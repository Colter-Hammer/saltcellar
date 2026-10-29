//! Passwords, session tokens and the session cookie (docs/spec/foundations.md#auth).

use std::fmt::Write;
use std::sync::LazyLock;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use axum::http::{HeaderMap, HeaderValue, header};
use domain::login::{Password, SESSION_LIFETIME};
use sha2::{Digest, Sha256};

/// The session cookie's name.
pub const COOKIE: &str = "session";

/// The argon2id hash of `password` with a fresh random salt, in PHC string form
/// (`$argon2id$v=19$…`), with argon2's default cost. Slow on purpose: call it off the async threads.
///
/// # Errors
/// If the OS has no random bytes for the salt.
pub fn hash_password(password: &Password) -> Result<String, argon2::password_hash::Error> {
    let hash = Argon2::default().hash_password(password.expose().as_bytes())?;
    Ok(hash.to_string())
}

/// Whether `typed` is the password `hash` was made from. With no hash (no such login), it checks
/// against a decoy instead, so an unknown username takes as long to refuse as a wrong password.
/// Slow on purpose: call it off the async threads.
pub fn verify_password(hash: Option<&str>, typed: &str) -> bool {
    static DECOY: LazyLock<Option<String>> = LazyLock::new(|| {
        Argon2::default()
            .hash_password(b"a decoy for logins that don't exist")
            .ok()
            .map(|hash| hash.to_string())
    });
    let Some(hash) = hash else {
        if let Some(decoy) = DECOY.as_deref() {
            let _ = Argon2::default().verify_password(typed.as_bytes(), decoy);
        }
        return false;
    };
    Argon2::default()
        .verify_password(typed.as_bytes(), hash)
        .is_ok()
}

/// A new session's token, which only the cookie holds, and its hash, which only the database holds.
pub struct NewSession {
    pub token: String,
    pub token_hash: String,
}

impl NewSession {
    /// A token of 32 random bytes.
    ///
    /// # Errors
    /// If the OS has no random bytes.
    pub fn generate() -> Result<NewSession, getrandom::Error> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)?;
        let token = hex(&bytes);
        let token_hash = token_hash(&token);
        Ok(NewSession { token, token_hash })
    }

    /// The `Set-Cookie` value that hands the token to the browser: `HttpOnly`, `SameSite=Lax`, and
    /// `Secure` unless the request came to this machine itself (`localhost`, or a loopback address
    /// *(agent)*), so local dev works over plain HTTP.
    pub fn cookie(&self, request: &HeaderMap) -> HeaderValue {
        cookie(&self.token, request)
    }
}

/// The `Set-Cookie` value that hands `token` to the browser for [`SESSION_LIFETIME`]: see
/// [`NewSession::cookie`]. Sent again on each use, so the cookie slides with the session.
pub fn cookie(token: &str, request: &HeaderMap) -> HeaderValue {
    set_cookie(token, SESSION_LIFETIME.as_secs(), request)
}

/// The `Set-Cookie` value that has the browser drop the session cookie.
pub fn cleared_cookie(request: &HeaderMap) -> HeaderValue {
    set_cookie("", 0, request)
}

fn set_cookie(token: &str, max_age: i64, request: &HeaderMap) -> HeaderValue {
    let secure = if is_local(request) { "" } else { "; Secure" };
    let cookie =
        format!("{COOKIE}={token}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}");
    // The token comes from a valid Cookie header or is hex: always a valid header value.
    HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// The session token from the request's `Cookie` headers, if it sent one.
pub fn token(request: &HeaderMap) -> Option<&str> {
    request
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, value)| *name == COOKIE && !value.is_empty())
        .map(|(_, value)| value)
}

/// What the database stores for a token: its SHA-256, in hex.
pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Whether the request's `Host` is this machine itself.
fn is_local(request: &HeaderMap) -> bool {
    let Some(host) = request
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let name = if host.starts_with('[') {
        host.split_inclusive(']').next().unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .any(|local| name.eq_ignore_ascii_case(local))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(value: &str) -> HeaderMap {
        HeaderMap::from_iter([(header::HOST, HeaderValue::from_str(value).unwrap())])
    }

    fn session() -> NewSession {
        NewSession {
            token: "ab12".to_owned(),
            token_hash: token_hash("ab12"),
        }
    }

    #[test]
    fn hashes_with_argon2id_and_a_fresh_salt() {
        let password = Password::new("correct horse battery", "correct horse battery").unwrap();
        let first = hash_password(&password).unwrap();
        let second = hash_password(&password).unwrap();
        assert!(first.starts_with("$argon2id$v=19$"), "{first}");
        assert!(!first.contains("correct horse"));
        assert_ne!(first, second);
    }

    #[test]
    fn verifies_the_password_the_hash_was_made_from() {
        let password = Password::new("correct horse battery", "correct horse battery").unwrap();
        let hash = hash_password(&password).unwrap();
        assert!(verify_password(Some(&hash), "correct horse battery"));
        assert!(!verify_password(Some(&hash), "correct horse batter"));
        assert!(!verify_password(Some(&hash), ""));
        assert!(!verify_password(
            Some("not a hash"),
            "correct horse battery"
        ));
        assert!(!verify_password(None, "correct horse battery"));
    }

    #[test]
    fn reads_the_session_token_from_among_the_cookies() {
        let cookies = |values: &[&str]| {
            values
                .iter()
                .map(|v| (header::COOKIE, HeaderValue::from_str(v).unwrap()))
                .collect::<HeaderMap>()
        };
        assert_eq!(token(&cookies(&["session=ab12"])), Some("ab12"));
        assert_eq!(
            token(&cookies(&["theme=dark; session=ab12; lang=en"])),
            Some("ab12")
        );
        assert_eq!(
            token(&cookies(&["theme=dark", "session=ab12"])),
            Some("ab12")
        );
        assert_eq!(token(&cookies(&["mysession=ab12"])), None);
        assert_eq!(token(&cookies(&["session="])), None);
        assert_eq!(token(&HeaderMap::new()), None);
    }

    #[test]
    fn the_cleared_cookie_expires_at_once() {
        assert_eq!(
            cleared_cookie(&host("localhost")),
            "session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"
        );
        assert!(
            cleared_cookie(&host("budget.example.test"))
                .to_str()
                .unwrap()
                .ends_with("; Secure")
        );
    }

    #[test]
    fn tokens_are_random_and_only_their_hash_is_stored() {
        let one = NewSession::generate().unwrap();
        let two = NewSession::generate().unwrap();
        assert_eq!(one.token.len(), 64);
        assert_ne!(one.token, two.token);
        assert_eq!(one.token_hash, token_hash(&one.token));
        assert_ne!(one.token_hash, one.token);
        assert_eq!(
            token_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_cookie_is_http_only_lax_and_lasts_thirty_days() {
        let cookie = session().cookie(&host("budget.example.test"));
        assert_eq!(
            cookie,
            "session=ab12; Path=/; Max-Age=2592000; HttpOnly; SameSite=Lax; Secure"
        );
    }

    #[test]
    fn the_cookie_is_secure_except_on_this_machine() {
        for local in [
            "localhost",
            "localhost:8080",
            "LocalHost:5173",
            "127.0.0.1:8080",
            "[::1]:8080",
            "[::1]",
        ] {
            let cookie = session().cookie(&host(local));
            assert!(!cookie.to_str().unwrap().contains("Secure"), "{local}");
        }
        for remote in [
            "budget.example.test",
            "localhost.example.test",
            "192.0.2.10:8080",
            "[2001:db8::1]:8080",
        ] {
            let cookie = session().cookie(&host(remote));
            assert!(cookie.to_str().unwrap().ends_with("; Secure"), "{remote}");
        }
        assert!(
            session()
                .cookie(&HeaderMap::new())
                .to_str()
                .unwrap()
                .ends_with("; Secure")
        );
    }
}
