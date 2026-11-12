//! Sync runs (docs/spec/m2-bank-data.md#sync-runs): the run and its per-account results, and
//! which feed accounts it serves. What a run writes into an account's rows is `synced_rows`.

use jiff::Timestamp;

use crate::{Error, SqlitePool, parse_stamp, stamp};

/// What started a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncTrigger {
    Scheduled,
    /// Sync now.
    Manual,
    Retry,
    CatchUp,
    /// Tracking an account.
    Backfill,
}

impl SyncTrigger {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SyncTrigger::Scheduled => "scheduled",
            SyncTrigger::Manual => "manual",
            SyncTrigger::Retry => "retry",
            SyncTrigger::CatchUp => "catch_up",
            SyncTrigger::Backfill => "backfill",
        }
    }

    pub(crate) fn parse(text: &str) -> Result<SyncTrigger, Error> {
        match text {
            "scheduled" => Ok(SyncTrigger::Scheduled),
            "manual" => Ok(SyncTrigger::Manual),
            "retry" => Ok(SyncTrigger::Retry),
            "catch_up" => Ok(SyncTrigger::CatchUp),
            "backfill" => Ok(SyncTrigger::Backfill),
            other => Err(Error::Unreadable {
                what: "a sync trigger",
                value: other.to_owned(),
            }),
        }
    }
}

/// A tracked feed account of the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedLink {
    pub account_id: i64,
    /// The provider's account id.
    pub external_id: String,
    /// Only its balance is synced; its rows are ignored.
    pub balances_only: bool,
    /// Its 90-day back-fill is done.
    pub backfilled: bool,
}

/// Every tracked feed account of connection `connection_id`, in the order first seen.
///
/// # Errors
/// If the database fails.
pub async fn tracked_feed_accounts(
    pool: &SqlitePool,
    connection_id: i64,
) -> Result<Vec<FeedLink>, Error> {
    let rows = sqlx::query!(
        r#"SELECT a.id AS "account_id!: i64", ca.external_account_id,
             a.has_transactions AS "has_transactions!: bool",
             ca.backfilled_at IS NOT NULL AS "backfilled!: bool"
           FROM connection_accounts ca JOIN accounts a ON a.id = ca.account_id
           WHERE ca.connection_id = ? AND ca.decision = 'tracked'
           ORDER BY ca.first_seen_at, ca.rowid"#,
        connection_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| FeedLink {
            account_id: row.account_id,
            external_id: row.external_account_id,
            balances_only: !row.has_transactions,
            backfilled: row.backfilled,
        })
        .collect())
}

/// Marks the feed accounts `external_ids` of connection `connection_id` back-filled.
///
/// # Errors
/// If the database fails.
pub async fn mark_backfilled(
    pool: &SqlitePool,
    connection_id: i64,
    external_ids: &[String],
    now: Timestamp,
) -> Result<(), Error> {
    let at = stamp(now);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    for id in external_ids {
        sqlx::query!(
            "UPDATE connection_accounts SET backfilled_at = ?
             WHERE connection_id = ? AND external_account_id = ?",
            at,
            connection_id,
            id
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Starts a run on connection `connection_id`; resolves to its id.
///
/// # Errors
/// If the database fails.
pub async fn start_sync_run(
    pool: &SqlitePool,
    connection_id: i64,
    trigger: SyncTrigger,
    now: Timestamp,
) -> Result<i64, Error> {
    let trigger = trigger.as_str();
    let at = stamp(now);
    let id = sqlx::query_scalar!(
        r#"INSERT INTO sync_runs (connection_id, trigger, started_at) VALUES (?, ?, ?)
           RETURNING id AS "id!: i64""#,
        connection_id,
        trigger,
        at
    )
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// How one account fared in a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountResult {
    pub account_id: i64,
    /// False: it keeps its last data.
    pub ok: bool,
    pub error_code: Option<String>,
    /// Sanitized.
    pub error: Option<String>,
    /// When the provider last refreshed its bank link.
    pub balance_date: Option<Timestamp>,
}

/// How a run ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunEnd {
    /// Bridge requests made.
    pub requests: u32,
    pub added: u64,
    pub updated: u64,
    /// Why the whole run failed, if it did.
    pub error_code: Option<String>,
    /// Sanitized.
    pub error: Option<String>,
}

/// Finishes run `run_id` with its per-account `results`; the connection's last sync time moves
/// on, and its last error is the run's (cleared by a run without one).
///
/// # Errors
/// If the database fails.
pub async fn finish_sync_run(
    pool: &SqlitePool,
    run_id: i64,
    end: &RunEnd,
    results: &[AccountResult],
    now: Timestamp,
) -> Result<(), Error> {
    let at = stamp(now);
    let added = i64::try_from(end.added).unwrap_or(i64::MAX);
    let updated = i64::try_from(end.updated).unwrap_or(i64::MAX);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    for result in results {
        let balance_date = result.balance_date.map(stamp);
        sqlx::query!(
            "INSERT INTO sync_account_results
               (sync_run_id, account_id, ok, error_code, error, provider_balance_date)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (sync_run_id, account_id) DO UPDATE SET
               ok = excluded.ok, error_code = excluded.error_code, error = excluded.error,
               provider_balance_date = excluded.provider_balance_date",
            run_id,
            result.account_id,
            result.ok,
            result.error_code,
            result.error,
            balance_date
        )
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query!(
        "UPDATE sync_runs
         SET finished_at = ?, requests = ?, added = ?, updated = ?, error_code = ?, error = ?
         WHERE id = ?",
        at,
        end.requests,
        added,
        updated,
        end.error_code,
        end.error,
        run_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE connections SET last_sync_at = ?, last_error = ?
         WHERE id = (SELECT connection_id FROM sync_runs WHERE id = ?)",
        at,
        end.error,
        run_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// A run, as the history shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRun {
    pub id: i64,
    pub trigger: SyncTrigger,
    pub started_at: Timestamp,
    /// `None` while it runs.
    pub finished_at: Option<Timestamp>,
    pub requests: i64,
    pub added: i64,
    pub updated: i64,
    pub error_code: Option<String>,
    pub error: Option<String>,
}

/// The household's connection's newest run, if it has had one.
///
/// # Errors
/// If the database fails or holds a run it can't read.
pub async fn latest_sync_run(pool: &SqlitePool) -> Result<Option<SyncRun>, Error> {
    let row = sqlx::query!(
        r#"SELECT id AS "id!: i64", trigger, started_at, finished_at, requests, added, updated,
             error_code, error
           FROM sync_runs
           WHERE connection_id = (SELECT id FROM connections ORDER BY id LIMIT 1)
           ORDER BY id DESC LIMIT 1"#
    )
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        Ok(SyncRun {
            id: row.id,
            trigger: SyncTrigger::parse(&row.trigger)?,
            started_at: parse_stamp(row.started_at)?,
            finished_at: row.finished_at.map(parse_stamp).transpose()?,
            requests: row.requests,
            added: row.added,
            updated: row.updated,
            error_code: row.error_code,
            error: row.error,
        })
    })
    .transpose()
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
