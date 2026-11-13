//! How the synced accounts and their bank links are doing (docs/spec/m2-bank-data.md#errors and
//! #sync-runs): per account, `ok`, stale or needing attention; per bank link, the same with
//! **needs reconnecting**. The rules are `domain::sync_health`'s; this module reads the facts and
//! words them *(agent: the server's messages are English)*.

use domain::sync_health::{self as rules, BankState, Health, Problem};
use jiff::Timestamp;
use serde::Serialize;
use utoipa::ToSchema;

use crate::{ApiError, AppState};

/// Where an account's data stands.
#[derive(Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountSyncState {
    Ok,
    /// Quiet: the bank's data is over 2 days old.
    Stale,
    /// Its bank link needs re-authorizing at the SimpleFIN Bridge.
    NeedsReconnecting,
    /// The last sync failed for it, its data is over 5 days old, or the bank stopped sending it.
    NeedsAttention,
}

/// An account's sync health.
#[derive(Serialize, ToSchema)]
pub struct AccountSync {
    state: AccountSyncState,
    /// One sentence for the household ("Data is 3 days old."); null when all is well.
    #[schema(required = true)]
    message: Option<String>,
    /// How old its data is, in whole days, when that is the problem.
    #[schema(required = true)]
    days: Option<i64>,
    /// RFC 3339: when the provider last refreshed its bank link; null before any sync read it.
    #[schema(required = true)]
    last_refreshed_at: Option<String>,
    /// **Fix at SimpleFIN** then **Done, sync now** can help: its bank link is broken.
    fix: bool,
}

/// A bank link's state.
#[derive(Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BankLinkState {
    Ok,
    /// Every tracked account at it syncs a balance only.
    BalancesOnly,
    /// Quiet: its data is over 2 days old.
    Stale,
    NeedsAttention,
    NeedsReconnecting,
}

/// One bank link of the connection, for Settings › Bank connection & sync.
#[derive(Serialize, ToSchema)]
pub struct Bank {
    /// The bank's name as the feed gives it.
    name: String,
    /// How many tracked accounts it has.
    accounts: u32,
    state: BankLinkState,
    /// "Bank hasn't updated in 4 days" and the like; null when all is well.
    #[schema(required = true)]
    message: Option<String>,
    /// How old its data is, in whole days, when stale.
    #[schema(required = true)]
    days: Option<i64>,
    /// RFC 3339: the newest refresh of any of its accounts; null before any sync read one.
    #[schema(required = true)]
    last_refreshed_at: Option<String>,
    /// **Fix at SimpleFIN ↗** and **Done, sync now** apply.
    fix: bool,
}

struct Entry {
    health: db::FeedAccountHealth,
    state: Health,
}

/// The health of every tracked feed account, read now.
pub(crate) struct Snapshot {
    entries: Vec<Entry>,
    /// The connection is paused: nothing syncs until a new setup token is pasted.
    paused: bool,
}

impl Snapshot {
    pub(crate) async fn load(state: &AppState) -> Result<Snapshot, ApiError> {
        let now = state.now();
        let paused = db::connection(&state.pool)
            .await?
            .is_some_and(|c| c.status != db::ConnectionStatus::Ok);
        let entries = db::feed_account_health(&state.pool)
            .await?
            .into_iter()
            .map(|health| Entry {
                state: rules::health(&health.facts, now),
                health,
            })
            .collect();
        Ok(Snapshot { entries, paused })
    }

    fn entry(&self, account_id: i64) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| entry.health.account_id == account_id)
    }

    /// The account's health, if it is a tracked feed account.
    pub(crate) fn account(&self, account_id: i64) -> Option<AccountSync> {
        self.entry(account_id).map(account_sync)
    }

    /// Whether a balances-only account takes a manual balance although it has a connection: its
    /// bank link is broken, or the connection is paused *(agent)*.
    pub(crate) fn takes_manual_balance(&self, account_id: i64) -> bool {
        self.entry(account_id).is_some_and(|entry| {
            entry.health.balances_only && (self.paused || entry.state.link_broken())
        })
    }

    /// One row per bank link, in the order its accounts were first seen.
    pub(crate) fn banks(&self) -> Vec<Bank> {
        let mut groups: Vec<(Option<&str>, Vec<&Entry>)> = Vec::new();
        for entry in &self.entries {
            let key = entry.health.bank_link_id.as_deref();
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, members)) => members.push(entry),
                None => groups.push((key, vec![entry])),
            }
        }
        groups
            .into_iter()
            .map(|(_, members)| bank(&members))
            .collect()
    }
}

fn days_old(days: i64) -> String {
    if days == 1 {
        "1 day".to_owned()
    } else {
        format!("{days} days")
    }
}

fn account_sync(entry: &Entry) -> AccountSync {
    let (state, days, message) =
        match entry.state {
            Health::Ok => (AccountSyncState::Ok, None, None),
            Health::Stale { days } => (
                AccountSyncState::Stale,
                Some(days),
                Some(format!("Data is {} old.", days_old(days))),
            ),
            Health::NeedsAttention(Problem::Reconnect) => (
                AccountSyncState::NeedsReconnecting,
                None,
                Some(entry.health.message.clone().unwrap_or_else(|| {
                    "Its bank needs to be reconnected at SimpleFIN.".to_owned()
                })),
            ),
            Health::NeedsAttention(Problem::Failed) => (
                AccountSyncState::NeedsAttention,
                None,
                Some(entry.health.message.clone().unwrap_or_else(|| {
                    "The last sync couldn't read this account. It keeps its last data.".to_owned()
                })),
            ),
            Health::NeedsAttention(Problem::Missing { days }) => (
                AccountSyncState::NeedsAttention,
                Some(days),
                Some(format!(
                    "The bank hasn't sent this account in {}.",
                    days_old(days)
                )),
            ),
            Health::NeedsAttention(Problem::OldData { days }) => (
                AccountSyncState::NeedsAttention,
                Some(days),
                Some(format!("Data is {} old.", days_old(days))),
            ),
        };
    AccountSync {
        state,
        message,
        days,
        last_refreshed_at: entry.health.facts.balance_date.map(|at| at.to_string()),
        fix: entry.state.link_broken(),
    }
}

fn bank(members: &[&Entry]) -> Bank {
    let pairs: Vec<(Health, bool)> = members
        .iter()
        .map(|entry| (entry.state, entry.health.balances_only))
        .collect();
    let name = members
        .iter()
        .find_map(|entry| entry.health.bank_name.clone())
        .unwrap_or_else(|| "Bank".to_owned());
    let refreshed: Option<Timestamp> = members
        .iter()
        .filter_map(|entry| entry.health.facts.balance_date)
        .max();
    let state = rules::bank_state(&pairs);
    let (state, days, message) = match state {
        BankState::Ok => (BankLinkState::Ok, None, None),
        BankState::BalancesOnly => (BankLinkState::BalancesOnly, None, None),
        BankState::Stale { days } => (
            BankLinkState::Stale,
            Some(days),
            Some(format!("Bank hasn't updated in {}.", days_old(days))),
        ),
        BankState::NeedsAttention => (BankLinkState::NeedsAttention, None, problem_line(members)),
        BankState::NeedsReconnecting => (
            BankLinkState::NeedsReconnecting,
            None,
            problem_line(members),
        ),
    };
    Bank {
        name,
        accounts: u32::try_from(members.len()).unwrap_or(u32::MAX),
        state,
        message,
        days,
        last_refreshed_at: refreshed.map(|at| at.to_string()),
        fix: members.iter().any(|entry| entry.state.link_broken()),
    }
}

/// The first account's problem at a bank that has one, in its own words.
fn problem_line(members: &[&Entry]) -> Option<String> {
    let worst = members
        .iter()
        .find(|entry| entry.state == Health::NeedsAttention(Problem::Reconnect))
        .or_else(|| {
            members
                .iter()
                .find(|entry| matches!(entry.state, Health::NeedsAttention(_)))
        })?;
    account_sync(worst).message
}
