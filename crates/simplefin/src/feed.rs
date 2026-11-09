//! Reading the feed (docs/spec/m2-bank-data.md#the-request): `GET <access URL>/accounts` with
//! `version=2`, `pending=1` and a `start-date`, Basic auth from the access URL's login.
//!
//! Only the accounts are read here, for the new accounts prompt; the back-fill reads their
//! balances and rows *(agent)*.

use std::collections::BTreeMap;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;

use crate::AccessUrl;

/// How long a request to the Bridge may take *(agent)*.
const TIMEOUT: Duration = Duration::from_secs(60);
/// The most of an answer read: 90 days of every account's rows is well under this *(agent)*.
const MAX_ANSWER: u64 = 32 * 1024 * 1024;

/// One account the connection reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedAccount {
    /// The provider's account id (`ACT-…`), opaque.
    pub id: String,
    /// The name the bank gives it.
    pub name: String,
    /// The name of its bank link (its `conn_id`'s connection), if the feed names it.
    pub institution: Option<String>,
    /// ISO 4217, as the feed spells it.
    pub currency: String,
}

/// Why reading the feed failed. None of these carry the access URL or the library's own message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// No answer: no network, a failed TLS handshake, a timeout.
    #[error("the SimpleFIN server couldn't be reached")]
    Unreachable,
    /// 403: the access URL was revoked.
    #[error("SimpleFIN turned the access URL down")]
    Revoked,
    /// 402: the subscription is unpaid.
    #[error("the SimpleFIN subscription is unpaid")]
    Unpaid,
    /// Any other status but 200.
    #[error("the SimpleFIN server answered {0}")]
    Refused(u16),
    /// A 200 that isn't the feed, or an access URL that isn't one.
    #[error("the SimpleFIN server's answer isn't a feed")]
    BadAnswer,
}

#[derive(Deserialize)]
struct Body {
    #[serde(default)]
    connections: Vec<Connection>,
    accounts: Vec<Account>,
}

#[derive(Deserialize)]
struct Connection {
    conn_id: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct Account {
    id: String,
    name: String,
    currency: String,
    conn_id: Option<String>,
}

/// The access URL split into the address without its login, and `user:pass`.
fn split(access: &AccessUrl) -> Option<(String, &str)> {
    let (scheme, rest) = access.expose().split_once("://")?;
    let (login, address) = rest.split_once('@')?;
    if login.contains('/') || !login.contains(':') {
        return None;
    }
    Some((
        format!("{scheme}://{}", address.trim_end_matches('/')),
        login,
    ))
}

/// Every account the connection reports, from rows transacted at or after `start_date` (epoch
/// seconds). Blocks until the answer; call it from `spawn_blocking`.
///
/// # Errors
/// [`FetchError`]: the Bridge can't be reached, refuses, or answers something that isn't a feed.
pub fn fetch_accounts(access: &AccessUrl, start_date: i64) -> Result<Vec<FeedAccount>, FetchError> {
    let (base, login) = split(access).ok_or(FetchError::BadAnswer)?;
    let url = format!("{base}/accounts?version=2&pending=1&start-date={start_date}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .user_agent(concat!("saltcellar/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .header(
            "Authorization",
            &format!("Basic {}", STANDARD.encode(login)),
        )
        .call()
        .map_err(|_| FetchError::Unreachable)?;
    match response.status().as_u16() {
        200 => {}
        403 => return Err(FetchError::Revoked),
        402 => return Err(FetchError::Unpaid),
        status => return Err(FetchError::Refused(status)),
    }
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER)
        .read_to_string()
        .map_err(|_| FetchError::BadAnswer)?;
    read(&text).ok_or(FetchError::BadAnswer)
}

/// The accounts in a v2 feed's text; `None` if it isn't one.
fn read(text: &str) -> Option<Vec<FeedAccount>> {
    let body: Body = serde_json::from_str(text).ok()?;
    let banks: BTreeMap<String, Option<String>> = body
        .connections
        .into_iter()
        .map(|connection| (connection.conn_id, connection.name))
        .collect();
    Some(
        body.accounts
            .into_iter()
            .map(|account| FeedAccount {
                institution: account
                    .conn_id
                    .and_then(|id| banks.get(&id).cloned().flatten())
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty()),
                id: account.id,
                name: account.name.trim().to_owned(),
                currency: account.currency.trim().to_ascii_uppercase(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_comes_out_of_the_access_url() {
        let access = AccessUrl::new("https://u1:p4ss@bridge.example.test/simplefin/".to_owned());
        assert_eq!(
            split(&access),
            Some((
                "https://bridge.example.test/simplefin".to_owned(),
                "u1:p4ss"
            ))
        );
        for bad in ["https://bridge.example.test/x", "nope", "https://u@host/x"] {
            assert_eq!(split(&AccessUrl::new(bad.to_owned())), None, "{bad}");
        }
    }

    #[test]
    fn a_feed_names_each_account_s_bank() {
        let text = r#"{"errlist":[],"connections":[{"conn_id":"CON-1","name":" Demo Bank "}],
          "accounts":[{"id":"ACT-1","conn_id":"CON-1","name":"Demo Checking","currency":"usd","balance":"1.00"},
                      {"id":"ACT-2","conn_id":"CON-9","name":"Demo Card","currency":"USD"}]}"#;
        assert_eq!(
            read(text).unwrap(),
            [
                FeedAccount {
                    id: "ACT-1".to_owned(),
                    name: "Demo Checking".to_owned(),
                    institution: Some("Demo Bank".to_owned()),
                    currency: "USD".to_owned(),
                },
                FeedAccount {
                    id: "ACT-2".to_owned(),
                    name: "Demo Card".to_owned(),
                    institution: None,
                    currency: "USD".to_owned(),
                },
            ]
        );
        assert_eq!(read("{}"), None);
        assert_eq!(read("Forbidden"), None);
    }
}
