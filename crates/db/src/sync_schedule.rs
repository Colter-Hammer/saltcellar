//! What the sync schedule reads and keeps (docs/spec/m2-bank-data.md#sync-runs): the sync
//! settings, the requests runs spent, the run history with each account's result, pruning, and
//! the kept raw responses. The raw responses arrive sealed; this module never sees them open.

use jiff::{SignedDuration, Timestamp};

use crate::{Error, SqlitePool, SyncRun, SyncTrigger, parse_stamp, stamp};

const TIME_KEY: &str = "sync.time";
const KEEP_RAW_KEY: &str = "sync.keep_raw_responses";

/// The household's sync settings, as stored. Unset ones are `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSettings {
    /// `sync.time`: `HH:MM` in the household timezone.
    pub time: Option<String>,
    /// `sync.keep_raw_responses`.
    pub keep_raw_responses: bool,
}

/// The sync settings. A value that isn't the JSON expected reads as unset *(agent)*.
///
/// # Errors
/// If the database fails.
pub async fn sync_settings(pool: &SqlitePool) -> Result<SyncSettings, Error> {
    let rows = sqlx::query!(
        "SELECT key, value FROM settings WHERE key IN (?, ?)",
        TIME_KEY,
        KEEP_RAW_KEY
    )
    .fetch_all(pool)
    .await?;
    let mut settings = SyncSettings::default();
    for row in rows {
        let value: Option<serde_json::Value> = serde_json::from_str(&row.value).ok();
        match row.key.as_str() {
            TIME_KEY => {
                settings.time = value.and_then(|v| v.as_str().map(str::to_owned));
            }
            _ => settings.keep_raw_responses = value.and_then(|v| v.as_bool()).unwrap_or(false),
        }
    }
    Ok(settings)
}

/// Stores the sync settings. Turning raw responses off deletes the kept ones *(agent)*.
///
/// # Errors
/// If the database fails.
pub async fn set_sync_settings(pool: &SqlitePool, time: &str, keep_raw: bool) -> Result<(), Error> {
    let time = serde_json::Value::from(time).to_string();
    let keep = serde_json::Value::from(keep_raw).to_string();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    for (key, value) in [(TIME_KEY, &time), (KEEP_RAW_KEY, &keep)] {
        sqlx::query!(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            key,
            value
        )
        .execute(&mut *tx)
        .await?;
    }
    if !keep_raw {
        sqlx::query!("DELETE FROM sync_raw_responses")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// `(started, requests)` of every run of the household's connection started after `since`.
///
/// # Errors
/// If the database fails or holds a time it can't read.
pub async fn requests_since(
    pool: &SqlitePool,
    since: Timestamp,
) -> Result<Vec<(Timestamp, i64)>, Error> {
    let since = stamp(since);
    let rows = sqlx::query!(
        r#"SELECT started_at, requests FROM sync_runs WHERE started_at > ? ORDER BY started_at"#,
        since
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| Ok((parse_stamp(row.started_at)?, row.requests)))
        .collect()
}

/// When the newest finished run that Sync now cools down after ended: any run but a back-fill
/// *(agent: tracking an account doesn't lock Sync now)*.
///
/// # Errors
/// If the database fails or holds a time it can't read.
pub async fn last_finished_sync(pool: &SqlitePool) -> Result<Option<Timestamp>, Error> {
    let at = sqlx::query_scalar!(
        r#"SELECT MAX(finished_at) AS "at: String" FROM sync_runs WHERE trigger <> 'backfill'"#
    )
    .fetch_one(pool)
    .await?;
    at.map(parse_stamp).transpose()
}

/// When the newest run that ended without a run-level error finished. Any trigger counts
/// *(agent: a fresh Sync now makes a catch-up pointless)*.
///
/// # Errors
/// If the database fails or holds a time it can't read.
pub async fn last_successful_sync(pool: &SqlitePool) -> Result<Option<Timestamp>, Error> {
    let at = sqlx::query_scalar!(
        r#"SELECT MAX(finished_at) AS "at: String" FROM sync_runs WHERE error_code IS NULL"#
    )
    .fetch_one(pool)
    .await?;
    at.map(parse_stamp).transpose()
}

/// When the newest scheduled run started.
///
/// # Errors
/// If the database fails or holds a time it can't read.
pub async fn last_scheduled_sync(pool: &SqlitePool) -> Result<Option<Timestamp>, Error> {
    let at = sqlx::query_scalar!(
        r#"SELECT MAX(started_at) AS "at: String" FROM sync_runs WHERE trigger = 'scheduled'"#
    )
    .fetch_one(pool)
    .await?;
    at.map(parse_stamp).transpose()
}

/// How one account fared in a run, for the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunAccount {
    pub account_id: i64,
    pub name: String,
    pub ok: bool,
    pub error_code: Option<String>,
    pub error: Option<String>,
    pub balance_date: Option<Timestamp>,
}

/// A run with its accounts' results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunWithAccounts {
    pub run: SyncRun,
    /// By account name.
    pub accounts: Vec<RunAccount>,
}

/// The household's connection's runs, newest first.
///
/// # Errors
/// If the database fails or holds a run it can't read.
pub async fn sync_history(pool: &SqlitePool) -> Result<Vec<RunWithAccounts>, Error> {
    let runs = sqlx::query!(
        r#"SELECT id AS "id!: i64", trigger, started_at, finished_at, requests, added, updated,
             error_code, error
           FROM sync_runs
           WHERE connection_id = (SELECT id FROM connections ORDER BY id LIMIT 1)
           ORDER BY id DESC"#
    )
    .fetch_all(pool)
    .await?;
    let results = sqlx::query!(
        r#"SELECT r.sync_run_id AS "run_id!: i64", r.account_id AS "account_id!: i64", a.name,
             r.ok AS "ok!: bool", r.error_code, r.error, r.provider_balance_date
           FROM sync_account_results r JOIN accounts a ON a.id = r.account_id
           ORDER BY r.sync_run_id, a.name, a.id"#
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(runs.len());
    for row in runs {
        let accounts = results
            .iter()
            .filter(|r| r.run_id == row.id)
            .map(|r| {
                Ok(RunAccount {
                    account_id: r.account_id,
                    name: r.name.clone(),
                    ok: r.ok,
                    error_code: r.error_code.clone(),
                    error: r.error.clone(),
                    balance_date: r.provider_balance_date.clone().map(parse_stamp).transpose()?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        out.push(RunWithAccounts {
            run: SyncRun {
                id: row.id,
                trigger: SyncTrigger::parse(&row.trigger)?,
                started_at: parse_stamp(row.started_at)?,
                finished_at: row.finished_at.map(parse_stamp).transpose()?,
                requests: row.requests,
                added: row.added,
                updated: row.updated,
                error_code: row.error_code,
                error: row.error,
            },
            accounts,
        });
    }
    Ok(out)
}

/// Deletes runs started more than `runs_kept` before `now` (their results and raw responses go
/// with them), and raw responses received more than `raw_kept` before.
///
/// # Errors
/// If the database fails.
pub async fn prune_sync_history(
    pool: &SqlitePool,
    now: Timestamp,
    runs_kept: SignedDuration,
    raw_kept: SignedDuration,
) -> Result<(), Error> {
    let runs_before = stamp(now - runs_kept);
    let raw_before = stamp(now - raw_kept);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query!("DELETE FROM sync_runs WHERE started_at < ?", runs_before)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        "DELETE FROM sync_raw_responses WHERE received_at < ?",
        raw_before
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Keeps response number `seq` of run `run_id`, already sealed.
///
/// # Errors
/// If the database fails.
pub async fn keep_raw_response(
    pool: &SqlitePool,
    run_id: i64,
    seq: i64,
    received: Timestamp,
    sealed: &[u8],
) -> Result<(), Error> {
    let at = stamp(received);
    sqlx::query!(
        "INSERT INTO sync_raw_responses (sync_run_id, seq, received_at, body) VALUES (?, ?, ?, ?)",
        run_id,
        seq,
        at,
        sealed
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "sync_schedule_tests.rs"]
mod tests;
