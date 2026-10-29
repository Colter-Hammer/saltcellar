//! Passwords, session tokens and the session cookie (docs/spec/foundations.md#auth).

use std::fmt::Write;

use argon2::Argon2;
use argon2::password_hash::PasswordHasher;
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
        let max_age = SESSION_LIFETIME.as_secs();
        let secure = if is_local(request) { "" } else { "; Secure" };
        let cookie = format!(
            "{COOKIE}={}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}",
            self.token
        );
        // Hex digits and fixed ASCII: always a valid header value.
        HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
    }
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
