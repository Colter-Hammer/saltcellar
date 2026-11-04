//! Claiming a setup token: the token is base64 of a claim URL (or is the claim URL itself); one
//! `POST` to it with an empty body answers the access URL.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};

/// How long the claim waits for the Bridge *(agent)*.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The most of the answer read; an access URL is a few hundred bytes *(agent)*.
const MAX_ANSWER: u64 = 4096;

/// Where claim URLs may point. In production, any `https://` URL. The dev setting
/// `BUDGET_SIMPLEFIN_CLAIM_HOSTS` narrows it to the listed `host[:port]`s, and then allows
/// `http://` to them, so a fake Bridge on localhost works *(agent)*.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimPolicy {
    hosts: Vec<String>,
}

impl ClaimPolicy {
    /// Only these `host[:port]`s, lowercased. An empty list is the production rule.
    #[must_use]
    pub fn only_hosts(hosts: impl IntoIterator<Item = String>) -> ClaimPolicy {
        ClaimPolicy {
            hosts: hosts
                .into_iter()
                .map(|host| host.trim().to_ascii_lowercase())
                .filter(|host| !host.is_empty())
                .collect(),
        }
    }

    /// The listed hosts, empty in production.
    #[must_use]
    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }

    /// Whether `url` (a claim URL, or the access URL the Bridge answered) may be talked to.
    fn allows(&self, url: &str) -> bool {
        let Some((scheme, rest)) = url.split_once("://") else {
            return false;
        };
        let Some(authority) = rest.split(['/', '?', '#']).next() else {
            return false;
        };
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        if host.is_empty() || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return false;
        }
        if self.hosts.is_empty() {
            return scheme == "https";
        }
        (scheme == "https" || scheme == "http")
            && self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host))
    }
}

/// Why a pasted setup token is refused, before any request. Never echoes the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("That doesn't look like a SimpleFIN setup token.")]
    NotAToken,
    #[error("A setup token has to point at an https:// address.")]
    NotHttps,
    #[error("This setup token points at a server this app isn't set to use.")]
    HostNotAllowed,
}

/// A claim URL that passed the policy. Its `Debug` hides the address, which carries the one-time
/// token.
#[derive(Clone, PartialEq, Eq)]
pub struct ClaimUrl(String);

impl std::fmt::Debug for ClaimUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClaimUrl(<redacted>)")
    }
}

impl ClaimUrl {
    /// Reads what was pasted: base64 of a claim URL, or a claim URL.
    ///
    /// # Errors
    /// If it is neither, or the URL isn't one `policy` allows.
    pub fn parse(pasted: &str, policy: &ClaimPolicy) -> Result<ClaimUrl, TokenError> {
        let pasted = pasted.trim();
        let url = if pasted.starts_with("https://") || pasted.starts_with("http://") {
            pasted.to_owned()
        } else {
            decode(pasted).ok_or(TokenError::NotAToken)?
        };
        if !url.contains("://") || url.contains('@') {
            return Err(TokenError::NotAToken);
        }
        if policy.allows(&url) {
            Ok(ClaimUrl(url))
        } else if policy.hosts.is_empty() {
            Err(TokenError::NotHttps)
        } else {
            Err(TokenError::HostNotAllowed)
        }
    }
}

/// Base64 of text, padded or not, standard alphabet or URL-safe.
fn decode(pasted: &str) -> Option<String> {
    let bare = pasted.trim_end_matches('=');
    let bytes = STANDARD_NO_PAD
        .decode(bare)
        .or_else(|_| URL_SAFE_NO_PAD.decode(bare))
        .ok()?;
    String::from_utf8(bytes).ok()
}

/// The SimpleFIN access URL, `https://user:pass@host/path`: a credential. `Debug` and `Display`
/// print a placeholder; [`AccessUrl::expose`] is the only way to the text.
#[derive(Clone, PartialEq, Eq)]
pub struct AccessUrl(String);

impl AccessUrl {
    /// Wraps an access URL's text.
    #[must_use]
    pub fn new(text: String) -> AccessUrl {
        AccessUrl(text)
    }

    /// The URL itself, for the request that needs it and for sealing it. Never for a log, an
    /// error or a response.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for AccessUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccessUrl(<redacted>)")
    }
}

impl std::fmt::Display for AccessUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<access url redacted>")
    }
}

/// Why claiming failed. None of these carry the claim URL, the access URL or the library's own
/// message, which can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClaimError {
    /// No answer: no network, a failed TLS handshake, a timeout.
    #[error("the SimpleFIN server couldn't be reached")]
    Unreachable,
    /// The server answered with this status (403: a token already claimed, or not one).
    #[error("the SimpleFIN server answered {0}")]
    Refused(u16),
    /// A 200 that isn't an access URL.
    #[error("the SimpleFIN server's answer isn't an access URL")]
    BadAnswer,
}

/// Claims `url`: one `POST` with an empty body, redirects not followed. Blocks until the answer;
/// call it from `spawn_blocking`.
///
/// # Errors
/// [`ClaimError`]: the server can't be reached, refuses, or answers with something else.
pub fn claim(url: &ClaimUrl, policy: &ClaimPolicy) -> Result<AccessUrl, ClaimError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .user_agent(concat!("saltcellar/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut response = agent
        .post(&url.0)
        .send_empty()
        .map_err(|_| ClaimError::Unreachable)?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(ClaimError::Refused(status));
    }
    let answer = response
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER)
        .read_to_string()
        .map_err(|_| ClaimError::BadAnswer)?;
    let answer = answer.trim();
    // `user:pass@` must be there, and the server must be one the policy allows.
    let has_login = answer
        .split_once("://")
        .and_then(|(_, rest)| rest.split('/').next())
        .is_some_and(|authority| authority.contains(':') && authority.contains('@'));
    if has_login && policy.allows(answer) {
        Ok(AccessUrl(answer.to_owned()))
    } else {
        Err(ClaimError::BadAnswer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production() -> ClaimPolicy {
        ClaimPolicy::default()
    }

    fn b64(text: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    #[test]
    fn a_token_is_base64_of_an_https_claim_url() {
        let token = b64("https://bridge.example.test/simplefin/claim/abc123");
        assert!(ClaimUrl::parse(&token, &production()).is_ok());
        assert!(ClaimUrl::parse(&format!("  {token}\n"), &production()).is_ok());
        assert!(ClaimUrl::parse(token.trim_end_matches('='), &production()).is_ok());
    }

    #[test]
    fn the_claim_url_itself_is_accepted() {
        assert!(
            ClaimUrl::parse(
                "https://bridge.example.test/simplefin/claim/abc",
                &production()
            )
            .is_ok()
        );
    }

    #[test]
    fn what_is_not_a_token_is_refused() {
        for pasted in [
            "",
            "hello there",
            "!!!!",
            &b64("not a url"),
            &b64("https://a@b/c"),
        ] {
            assert_eq!(
                ClaimUrl::parse(pasted, &production()).unwrap_err(),
                TokenError::NotAToken,
                "{pasted:?}"
            );
        }
    }

    #[test]
    fn production_wants_https() {
        for pasted in [
            "http://bridge.example.test/claim/abc".to_owned(),
            b64("http://bridge.example.test/claim/abc"),
            b64("ftp://bridge.example.test/claim/abc"),
        ] {
            assert_eq!(
                ClaimUrl::parse(&pasted, &production()).unwrap_err(),
                TokenError::NotHttps
            );
        }
    }

    #[test]
    fn the_dev_setting_allows_only_its_hosts_and_plain_http_to_them() {
        let dev = ClaimPolicy::only_hosts(["127.0.0.1:8190".to_owned()]);
        assert!(ClaimUrl::parse("http://127.0.0.1:8190/claim/abc", &dev).is_ok());
        assert!(ClaimUrl::parse(&b64("http://127.0.0.1:8190/claim/abc"), &dev).is_ok());
        assert_eq!(
            ClaimUrl::parse("https://bridge.example.test/claim/abc", &dev).unwrap_err(),
            TokenError::HostNotAllowed
        );
        assert_eq!(
            ClaimUrl::parse("http://127.0.0.1:9999/claim/abc", &dev).unwrap_err(),
            TokenError::HostNotAllowed
        );
    }

    #[test]
    fn the_secrets_print_as_placeholders() {
        let claim =
            ClaimUrl::parse("https://bridge.example.test/claim/tok-1", &production()).unwrap();
        assert!(!format!("{claim:?}").contains("tok-1"));
        let access = AccessUrl::new("https://u:p4ss@bridge.example.test/x".to_owned());
        assert!(!format!("{access:?} {access}").contains("p4ss"));
        assert_eq!(access.expose(), "https://u:p4ss@bridge.example.test/x");
    }
}
