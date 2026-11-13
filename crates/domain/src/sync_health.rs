//! How healthy each synced account and bank link is (docs/spec/m2-bank-data.md#errors and
//! #sync-runs): what an `errlist` code means, and when data is **stale** or **needs attention**.
//! Pure: every rule takes the time it is asked at, so tests move a fake clock.

use jiff::{SignedDuration, Timestamp};

/// Data whose `balance-date` is older than this is stale: quiet, no retry.
pub const STALE_AFTER: SignedDuration = SignedDuration::from_hours(2 * 24);
/// Data older than this, or an account missing from the responses this long, needs attention.
pub const ATTENTION_AFTER: SignedDuration = SignedDuration::from_hours(5 * 24);

/// The error code of a result for an account the bank didn't send *(agent)*.
pub const ABSENT: &str = "absent";

/// An errlist code's prefix and subcode: `con.auth` is `("con", "auth")`; a code with more
/// parts, like `con.auth.expired`, reads as its first two.
fn parts(code: &str) -> (&str, &str) {
    let mut parts = code.split('.');
    (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
}

/// Whether `code` says one bank link needs re-authorizing at the Bridge (`con.auth`).
#[must_use]
pub fn needs_reconnecting(code: &str) -> bool {
    parts(code) == ("con", "auth")
}

/// Whether `code` says the access URL was revoked or disabled (`gen.auth`): all syncing pauses.
#[must_use]
pub fn access_revoked(code: &str) -> bool {
    parts(code) == ("gen", "auth")
}

/// What the last sync said about one tracked account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub ok: bool,
    /// The errlist code, or [`ABSENT`] and the like.
    pub error_code: Option<String>,
}

/// What is known about one tracked account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// When the provider last refreshed its bank link, from the newest result that had one.
    pub balance_date: Option<Timestamp>,
    /// The newest result, if any sync has read it.
    pub latest: Option<Latest>,
    /// When the account last appeared in a response.
    pub last_seen: Option<Timestamp>,
}

/// Why an account needs attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// Its bank link needs re-authorizing at the Bridge (`con.auth`).
    Reconnect,
    /// The last sync failed for it in another way; it keeps its last data.
    Failed,
    /// Tracked, but missing from the responses for this many days.
    Missing { days: i64 },
    /// Its data is this many days old.
    OldData { days: i64 },
}

/// An account's health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ok,
    /// Quiet: its data is this many days old.
    Stale {
        days: i64,
    },
    NeedsAttention(Problem),
}

impl Health {
    /// Whether its bank link is broken: the Bridge needs a visit (**Fix at SimpleFIN**), and a
    /// balances-only account takes a manual balance in the meantime *(agent)*. A failed read
    /// (`act.*`) isn't: the bank is fine and the next sync may clear it.
    #[must_use]
    pub fn link_broken(self) -> bool {
        matches!(
            self,
            Health::NeedsAttention(
                Problem::Reconnect | Problem::Missing { .. } | Problem::OldData { .. }
            )
        )
    }
}

fn whole_days(from: Timestamp, now: Timestamp) -> i64 {
    now.duration_since(from).as_hours().max(0) / 24
}

/// How one account stands at `now`. Reconnecting comes first, then a failed read, then missing
/// too long, then old data, then stale.
#[must_use]
pub fn health(facts: &Facts, now: Timestamp) -> Health {
    if let Some(latest) = &facts.latest {
        let code = latest.error_code.as_deref();
        if code.is_some_and(needs_reconnecting) {
            return Health::NeedsAttention(Problem::Reconnect);
        }
        if !latest.ok && code != Some(ABSENT) {
            return Health::NeedsAttention(Problem::Failed);
        }
        if code == Some(ABSENT)
            && let Some(seen) = facts.last_seen
            && now.duration_since(seen) > ATTENTION_AFTER
        {
            let days = whole_days(seen, now);
            return Health::NeedsAttention(Problem::Missing { days });
        }
    }
    let Some(refreshed) = facts.balance_date else {
        return Health::Ok;
    };
    let age = now.duration_since(refreshed);
    let days = whole_days(refreshed, now);
    if age > ATTENTION_AFTER {
        Health::NeedsAttention(Problem::OldData { days })
    } else if age > STALE_AFTER {
        Health::Stale { days }
    } else {
        Health::Ok
    }
}

/// A bank link's state: the worst of its accounts' *(agent)*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankState {
    Ok,
    /// Every tracked account at it syncs a balance only.
    BalancesOnly,
    /// Quiet: its newest data is this many days old.
    Stale {
        days: i64,
    },
    NeedsAttention,
    NeedsReconnecting,
}

/// A bank link's state from its tracked accounts' health, each with whether it is balances-only.
#[must_use]
pub fn bank_state(accounts: &[(Health, bool)]) -> BankState {
    let health = |f: fn(Health) -> bool| accounts.iter().any(|&(h, _)| f(h));
    if health(|h| h == Health::NeedsAttention(Problem::Reconnect)) {
        return BankState::NeedsReconnecting;
    }
    if health(|h| matches!(h, Health::NeedsAttention(_))) {
        return BankState::NeedsAttention;
    }
    let oldest = accounts
        .iter()
        .filter_map(|&(h, _)| match h {
            Health::Stale { days } => Some(days),
            _ => None,
        })
        .max();
    if let Some(days) = oldest {
        return BankState::Stale { days };
    }
    if !accounts.is_empty() && accounts.iter().all(|&(_, balances_only)| balances_only) {
        return BankState::BalancesOnly;
    }
    BankState::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    const NOW: &str = "2026-10-28T09:00:00Z";

    fn facts(balance_date: Option<Timestamp>, latest: Option<(bool, Option<&str>)>) -> Facts {
        Facts {
            balance_date,
            latest: latest.map(|(ok, code)| Latest {
                ok,
                error_code: code.map(str::to_owned),
            }),
            last_seen: Some(at(NOW)),
        }
    }

    fn aged(age: SignedDuration) -> Facts {
        facts(Some(at(NOW) - age), Some((true, None)))
    }

    #[test]
    fn the_codes_read_by_prefix_and_subcode() {
        assert!(needs_reconnecting("con.auth"));
        assert!(needs_reconnecting("con.auth.expired"));
        assert!(!needs_reconnecting("con.failed"));
        assert!(access_revoked("gen.auth"));
        assert!(!access_revoked("gen.api"));
        assert!(!access_revoked("con.auth"));
        assert!(!needs_reconnecting(""));
    }

    #[test]
    fn data_goes_stale_after_two_days_and_needs_attention_after_five() {
        let now = at(NOW);
        let two_days = SignedDuration::from_hours(48);
        let five_days = SignedDuration::from_hours(120);
        let second = SignedDuration::from_secs(1);
        assert_eq!(health(&aged(two_days), now), Health::Ok);
        assert_eq!(
            health(&aged(two_days + second), now),
            Health::Stale { days: 2 }
        );
        assert_eq!(
            health(&aged(SignedDuration::from_hours(72)), now),
            Health::Stale { days: 3 }
        );
        assert_eq!(
            health(&aged(five_days), now),
            Health::Stale { days: 5 },
            "exactly five days is still only stale"
        );
        assert_eq!(
            health(&aged(five_days + second), now),
            Health::NeedsAttention(Problem::OldData { days: 5 })
        );
    }

    #[test]
    fn a_future_balance_date_is_fresh() {
        let now = at(NOW);
        assert_eq!(
            health(&aged(SignedDuration::from_hours(-5)), now),
            Health::Ok
        );
    }

    #[test]
    fn an_account_no_sync_has_read_is_fine() {
        assert_eq!(health(&facts(None, None), at(NOW)), Health::Ok);
    }

    #[test]
    fn con_auth_means_reconnecting_whatever_the_age() {
        let fresh = facts(Some(at(NOW)), Some((false, Some("con.auth"))));
        assert_eq!(
            health(&fresh, at(NOW)),
            Health::NeedsAttention(Problem::Reconnect)
        );
    }

    #[test]
    fn another_failed_read_needs_attention_but_isnt_a_broken_link() {
        let failed = facts(Some(at(NOW)), Some((false, Some("act.missingdata"))));
        let got = health(&failed, at(NOW));
        assert_eq!(got, Health::NeedsAttention(Problem::Failed));
        assert!(!got.link_broken());
        let other = facts(Some(at(NOW)), Some((false, Some("con.failed"))));
        assert_eq!(
            health(&other, at(NOW)),
            Health::NeedsAttention(Problem::Failed)
        );
    }

    #[test]
    fn a_missing_account_needs_attention_only_after_five_days() {
        let now = at(NOW);
        let mut gone = facts(None, Some((false, Some(ABSENT))));
        gone.last_seen = Some(now - SignedDuration::from_hours(120));
        assert_eq!(health(&gone, now), Health::Ok);
        gone.last_seen = Some(now - SignedDuration::from_hours(120) - SignedDuration::from_secs(1));
        let got = health(&gone, now);
        assert_eq!(got, Health::NeedsAttention(Problem::Missing { days: 5 }));
        assert!(got.link_broken());
    }

    #[test]
    fn a_link_is_broken_for_reconnecting_missing_and_old_data() {
        assert!(Health::NeedsAttention(Problem::Reconnect).link_broken());
        assert!(Health::NeedsAttention(Problem::OldData { days: 6 }).link_broken());
        assert!(!Health::Stale { days: 3 }.link_broken());
        assert!(!Health::Ok.link_broken());
    }

    #[test]
    fn a_bank_is_as_bad_as_its_worst_account() {
        let stale = Health::Stale { days: 4 };
        let reconnect = Health::NeedsAttention(Problem::Reconnect);
        let failed = Health::NeedsAttention(Problem::Failed);
        assert_eq!(bank_state(&[]), BankState::Ok);
        assert_eq!(bank_state(&[(Health::Ok, false)]), BankState::Ok);
        assert_eq!(
            bank_state(&[(stale, false), (Health::Ok, false)]),
            BankState::Stale { days: 4 }
        );
        assert_eq!(
            bank_state(&[(stale, false), (failed, false)]),
            BankState::NeedsAttention
        );
        assert_eq!(
            bank_state(&[(failed, false), (reconnect, true)]),
            BankState::NeedsReconnecting
        );
        assert_eq!(
            bank_state(&[(Health::Ok, true), (Health::Ok, true)]),
            BankState::BalancesOnly
        );
        assert_eq!(
            bank_state(&[(Health::Ok, true), (Health::Ok, false)]),
            BankState::Ok
        );
    }
}
