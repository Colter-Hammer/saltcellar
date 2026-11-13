//! Balances (docs/spec/m2-bank-data.md#balances): the bank's balance written as a snapshot by a
//! sync run, the opening balance a first sync derives and older rows move back, drift and its
//! flag with the suspect rows, and balance adjustments. The rules are `domain::balances`.
//!
//! Each snapshot stores the app's posted-only and with-pending balances as they were when it was
//! written (opening + rows + adjustments), so drift is read from snapshots alone. Accepting an
//! adjustment, or undoing one, adds it to (or takes it off) the stored app balances of the
//! snapshots on or after its date, as if it had been there when they were written *(agent)*.

use domain::accounts::{AccountType, Opening};
use domain::balances::{self as rules, AppBalances, SignCheck};
use domain::{Date, Money};
use jiff::Timestamp;

use crate::{Error, SqlitePool, parse_stamp, stamp};

type Tx<'a> = sqlx::Transaction<'a, sqlx::Sqlite>;

/// A bank balance a sync run read for one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BankBalance {
    pub account_id: i64,
    /// As the feed reported it, sign and all.
    pub balance: Money,
    /// When the provider last refreshed the account's bank link.
    pub balance_date: Timestamp,
    pub run_id: i64,
    /// One major unit of the household currency, in minor units (100 for two decimal places):
    /// the sign rule's near-zero edge.
    pub unit: Money,
}

/// What recording a bank balance did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceRecorded {
    /// `balance-date` didn't move forward: nothing is written and the run doesn't count.
    NotMoved,
    /// A snapshot was written (or a same-day one replaced), its balance flipped when
    /// `sign_corrected`.
    Written { sign_corrected: bool },
}

/// Records the bank's balance for one account in a run that succeeded for it, after its rows are
/// applied: a snapshot dated the UTC date of `balance-date`, written only when that moved forward
/// past the newest provider snapshot's, replacing one already there that day. The sign rule runs
/// first, once the account has an app balance to compare with. On an account with transactions and no opening balance yet (its first sync), the
/// opening balance is derived from it; the app's two balances go beside it and
/// `balance_includes_pending` may be learned. A balances-only account gets the snapshot alone.
///
/// # Errors
/// If the database fails or holds a date or sum it can't read; nothing is written then.
pub async fn record_bank_balance(
    pool: &SqlitePool,
    bank: &BankBalance,
) -> Result<BalanceRecorded, Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let id = bank.account_id;
    let found = sqlx::query!(
        r#"SELECT a.type AS kind, a.has_transactions AS "has_transactions!: bool",
             a.opening_balance_minor, a.opening_date,
             a.balance_includes_pending AS "includes_pending: bool",
             (SELECT s.provider_balance_date FROM balance_snapshots s
               WHERE s.account_id = a.id AND s.source = 'provider'
                 AND s.provider_balance_date IS NOT NULL
               ORDER BY s.provider_balance_date DESC LIMIT 1) AS "latest: String"
           FROM accounts a WHERE a.id = ?"#,
        id
    )
    .fetch_one(&mut *tx)
    .await?;
    let latest = found.latest.map(parse_stamp).transpose()?;
    if !rules::moved_forward(
        latest.map(Timestamp::as_second),
        bank.balance_date.as_second(),
    ) {
        return Ok(BalanceRecorded::NotMoved);
    }
    let as_of = domain::feed::utc_date(bank.balance_date.as_second())
        .ok_or_else(|| unreadable("a balance date", bank.balance_date.to_string()))?;
    let kind = AccountType::from_stored(&found.kind)
        .ok_or_else(|| unreadable("an account type", found.kind.clone()))?;

    let (balance, sign_corrected, app) = if found.has_transactions {
        let sums = sums(&mut tx, id).await?;
        let opening = opening(found.opening_balance_minor, found.opening_date)?;
        // On a first sync there's no app balance to compare the sign with: the rows alone are
        // a window's activity, not a balance, so the bank's is taken as sent *(agent)*.
        let (balance, corrected) = match opening {
            Some(opening) => rules::correct_sign(
                bank.balance,
                SignCheck::Rows {
                    app_posted: sums.app(opening.balance)?.posted,
                },
                bank.unit,
            ),
            None => (bank.balance, false),
        };
        let opening = match opening {
            Some(opening) => opening,
            None => derive_opening(&mut tx, id, balance, &sums, as_of).await?,
        };
        let app = sums.app(opening.balance)?;
        let learned = rules::learn_includes_pending(found.includes_pending, balance, app);
        if learned != found.includes_pending {
            sqlx::query!(
                "UPDATE accounts SET balance_includes_pending = ? WHERE id = ?",
                learned,
                id
            )
            .execute(&mut *tx)
            .await?;
        }
        (balance, corrected, Some(app))
    } else {
        let (balance, corrected) =
            rules::correct_sign(bank.balance, SignCheck::BalancesOnly { kind }, bank.unit);
        (balance, corrected, None)
    };

    let (as_of, minor) = (as_of.to_string(), balance.minor());
    let balance_date = stamp(bank.balance_date);
    let posted = app.map(|a| a.posted.minor());
    let pending = app.map(|a| a.with_pending.minor());
    sqlx::query!(
        "INSERT INTO balance_snapshots
           (account_id, as_of, balance_minor, source, provider_balance_date, sync_run_id,
            sign_corrected, app_balance_posted_minor, app_balance_pending_minor)
         VALUES (?, ?, ?, 'provider', ?, ?, ?, ?, ?)
         ON CONFLICT (account_id, as_of) DO UPDATE SET
           balance_minor = excluded.balance_minor, source = excluded.source,
           provider_balance_date = excluded.provider_balance_date,
           sync_run_id = excluded.sync_run_id, sign_corrected = excluded.sign_corrected,
           app_balance_posted_minor = excluded.app_balance_posted_minor,
           app_balance_pending_minor = excluded.app_balance_pending_minor",
        id,
        as_of,
        minor,
        balance_date,
        bank.run_id,
        sign_corrected,
        posted,
        pending
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(BalanceRecorded::Written { sign_corrected })
}

/// Derives and stores account `id`'s opening balance on its first sync, from the bank's
/// `balance` dated `as_of`.
async fn derive_opening(
    tx: &mut Tx<'_>,
    id: i64,
    balance: Money,
    sums: &Sums,
    as_of: Date,
) -> Result<Opening, Error> {
    let rows_and_adjustments = add(sums.posted, sums.adjustments)?;
    let derived = rules::first_opening(balance, rows_and_adjustments, sums.earliest, as_of)
        .ok_or_else(|| unreadable("a balance", id.to_string()))?;
    let (minor, date) = (derived.balance.minor(), derived.date.to_string());
    sqlx::query!(
        "UPDATE accounts SET opening_balance_minor = ?, opening_date = ? WHERE id = ?",
        minor,
        date,
        id
    )
    .execute(&mut **tx)
    .await?;
    Ok(derived)
}

/// What an account's rows and adjustments add up to now.
pub(crate) struct Sums {
    /// Posted rows.
    pub posted: Money,
    /// Every row, pending ones included.
    pub all: Money,
    /// Adjustments not undone.
    pub adjustments: Money,
    /// The earliest row's date.
    pub earliest: Option<Date>,
}

impl Sums {
    /// The app's two balances on top of `opening`.
    pub(crate) fn app(&self, opening: Money) -> Result<AppBalances, Error> {
        let base = add(opening, self.adjustments)?;
        Ok(AppBalances {
            posted: add(base, self.posted)?,
            with_pending: add(base, self.all)?,
        })
    }
}

pub(crate) async fn sums(tx: &mut Tx<'_>, id: i64) -> Result<Sums, Error> {
    let found = sqlx::query!(
        r#"SELECT
             (SELECT COALESCE(SUM(amount_minor), 0) FROM transactions
               WHERE account_id = ?1 AND status = 'posted') AS "posted!: i64",
             (SELECT COALESCE(SUM(amount_minor), 0) FROM transactions
               WHERE account_id = ?1) AS "all!: i64",
             (SELECT COALESCE(SUM(amount_minor), 0) FROM balance_adjustments
               WHERE account_id = ?1 AND undone_at IS NULL) AS "adjustments!: i64",
             (SELECT MIN(date) FROM transactions WHERE account_id = ?1) AS "earliest: String""#,
        id
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(Sums {
        posted: Money::from_minor(found.posted),
        all: Money::from_minor(found.all),
        adjustments: Money::from_minor(found.adjustments),
        earliest: found.earliest.map(read_date).transpose()?,
    })
}

/// Moves account `id`'s opening balance back over the posted rows dated on or before it, older
/// history that just arrived (an Actual import, a file, a back-fill): their total comes off it
/// and it is dated the day before the earliest row, so the app's balances, and drift, don't
/// change. Resolves to the opening it had, when it moved; nothing happens to an account without
/// one (a first sync derives it after its rows) or without older rows.
///
/// # Errors
/// If the database fails or holds a date or sum it can't read.
pub(crate) async fn move_opening_back(
    tx: &mut sqlx::SqliteConnection,
    id: i64,
) -> Result<Option<Opening>, Error> {
    let found = sqlx::query!(
        "SELECT opening_balance_minor, opening_date FROM accounts WHERE id = ?",
        id
    )
    .fetch_one(&mut *tx)
    .await?;
    let Some(before) = opening(found.opening_balance_minor, found.opening_date)? else {
        return Ok(None);
    };
    let date = before.date.to_string();
    let older = sqlx::query!(
        r#"SELECT COUNT(*) AS "count!: i64",
             COALESCE(SUM(amount_minor), 0) AS "total!: i64",
             MIN(date) AS "earliest: String"
           FROM transactions WHERE account_id = ? AND status = 'posted' AND date <= ?"#,
        id,
        date
    )
    .fetch_one(&mut *tx)
    .await?;
    let Some(earliest) = older.earliest.filter(|_| older.count > 0) else {
        return Ok(None);
    };
    let moved = rules::moved_back(before, Money::from_minor(older.total), read_date(earliest)?)
        .ok_or_else(|| unreadable("a balance", id.to_string()))?;
    let (minor, date) = (moved.balance.minor(), moved.date.to_string());
    sqlx::query!(
        "UPDATE accounts SET opening_balance_minor = ?, opening_date = ? WHERE id = ?",
        minor,
        date,
        id
    )
    .execute(&mut *tx)
    .await?;
    Ok(Some(before))
}

/// The latest snapshot of an account, as its detail shows bank against app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestSnapshot {
    pub as_of: Date,
    pub bank: Money,
    /// The app's balance drift is measured against (posted-only unless the bank's is known to
    /// count pending rows), as it was when the snapshot was written.
    pub app: Money,
    /// Bank less app; zero where the bank matches either app balance while that isn't known.
    pub drift: Money,
    /// The bank sent it with the other sign, and it was flipped.
    pub sign_corrected: bool,
}

/// A row a sync run added, changed or dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspectRow {
    /// `None` once the row is gone (a dropped one, or one deleted since).
    pub transaction_id: Option<i64>,
    pub change: RowChange,
    pub date: Date,
    pub amount: Money,
    /// The bank's text for it.
    pub description: String,
}

/// What a run did to a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowChange {
    Added,
    Changed,
    Dropped,
}

impl RowChange {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RowChange::Added => "added",
            RowChange::Changed => "changed",
            RowChange::Dropped => "dropped",
        }
    }

    fn parse(text: &str) -> Result<RowChange, Error> {
        match text {
            "added" => Ok(RowChange::Added),
            "changed" => Ok(RowChange::Changed),
            "dropped" => Ok(RowChange::Dropped),
            other => Err(unreadable("a row change", other.to_owned())),
        }
    }
}

/// A drift flag on an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftFlag {
    /// Bank less app.
    pub drift: Money,
    /// When drift last stepped to this value: the stepping snapshot's date.
    pub since: Date,
    /// What the runs up to and including the stepping snapshot's, after the snapshot before it,
    /// added, changed or dropped on the account *(agent: not only the stepping run's, since a
    /// run where `balance-date` didn't move may have brought the row)*.
    pub suspect_rows: Vec<SuspectRow>,
}

/// A balance adjustment, undone or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adjustment {
    pub id: i64,
    pub date: Date,
    pub amount: Money,
    pub note: Option<String>,
    /// The display name of who accepted it.
    pub accepted_by: Option<String>,
    pub accepted_at: Timestamp,
    pub undone_at: Option<Timestamp>,
}

/// An account's balances against the bank: what its detail shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Drift {
    /// The newest snapshot with the app's balance beside it; `None` before there is one, or for
    /// a balances-only account.
    pub latest: Option<LatestSnapshot>,
    /// Whether the newest snapshot of any kind had its sign flipped (balances-only accounts too).
    pub sign_corrected: bool,
    pub flag: Option<DriftFlag>,
    /// Newest first, undone ones included.
    pub adjustments: Vec<Adjustment>,
}

/// Account `id`'s drift: its newest snapshot, bank against app, the flag when drift held at the
/// same non-zero value for two snapshots in a row, and its balance adjustments.
///
/// # Errors
/// If the database fails or holds a date or sum it can't read.
pub async fn account_drift(pool: &SqlitePool, id: i64) -> Result<Drift, Error> {
    let mut conn = pool.acquire().await?;
    drift_on(&mut conn, id).await
}

async fn drift_on(conn: &mut sqlx::SqliteConnection, id: i64) -> Result<Drift, Error> {
    let includes_pending = sqlx::query_scalar!(
        r#"SELECT balance_includes_pending AS "includes: bool" FROM accounts WHERE id = ?"#,
        id
    )
    .fetch_optional(&mut *conn)
    .await?
    .flatten();
    let snapshots = sqlx::query!(
        r#"SELECT as_of, balance_minor, sync_run_id, sign_corrected AS "sign_corrected!: bool",
             app_balance_posted_minor, app_balance_pending_minor
           FROM balance_snapshots WHERE account_id = ? ORDER BY as_of"#,
        id
    )
    .fetch_all(&mut *conn)
    .await?;
    let sign_corrected = snapshots.last().is_some_and(|s| s.sign_corrected);

    // Snapshots with the app's balances beside them, oldest first.
    let mut measured = Vec::new();
    for s in &snapshots {
        let (Some(posted), Some(pending)) =
            (s.app_balance_posted_minor, s.app_balance_pending_minor)
        else {
            continue;
        };
        let app = AppBalances {
            posted: Money::from_minor(posted),
            with_pending: Money::from_minor(pending),
        };
        let bank = Money::from_minor(s.balance_minor);
        let drift = rules::drift(bank, app, includes_pending)
            .ok_or_else(|| unreadable("a balance", id.to_string()))?;
        let pending_counts = includes_pending == Some(true)
            || (includes_pending.is_none() && bank == app.with_pending);
        let against = if pending_counts {
            app.with_pending
        } else {
            app.posted
        };
        measured.push((read_date(s.as_of.clone())?, bank, against, drift, s));
    }

    let latest = measured
        .last()
        .map(|(as_of, bank, app, drift, s)| LatestSnapshot {
            as_of: *as_of,
            bank: *bank,
            app: *app,
            drift: *drift,
            sign_corrected: s.sign_corrected,
        });
    let drifts: Vec<Money> = measured.iter().map(|m| m.3).collect();
    let flag = match rules::flag(&drifts) {
        None => None,
        Some(flag) => {
            let stepped = &measured[flag.stepped_at];
            let before_run = flag
                .stepped_at
                .checked_sub(1)
                .and_then(|i| measured[i].4.sync_run_id);
            let suspect_rows = match stepped.4.sync_run_id {
                Some(run) => suspect_rows(conn, id, before_run, run).await?,
                None => Vec::new(),
            };
            Some(DriftFlag {
                drift: flag.drift,
                since: stepped.0,
                suspect_rows,
            })
        }
    };
    let adjustments = adjustments(conn, id).await?;
    Ok(Drift {
        latest,
        sign_corrected,
        flag,
        adjustments,
    })
}

/// What runs after `after` (exclusive; only `through` when `None`) up to `through` did to
/// account `id`'s rows, by date.
async fn suspect_rows(
    conn: &mut sqlx::SqliteConnection,
    id: i64,
    after: Option<i64>,
    through: i64,
) -> Result<Vec<SuspectRow>, Error> {
    let from = after.unwrap_or(through - 1);
    let rows = sqlx::query!(
        r#"SELECT transaction_id, change, date, amount_minor, description
           FROM sync_row_changes
           WHERE account_id = ? AND sync_run_id > ? AND sync_run_id <= ?
           ORDER BY date DESC, rowid"#,
        id,
        from,
        through
    )
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(SuspectRow {
                transaction_id: row.transaction_id,
                change: RowChange::parse(&row.change)?,
                date: read_date(row.date)?,
                amount: Money::from_minor(row.amount_minor),
                description: row.description,
            })
        })
        .collect()
}

async fn adjustments(conn: &mut sqlx::SqliteConnection, id: i64) -> Result<Vec<Adjustment>, Error> {
    let rows = sqlx::query!(
        r#"SELECT b.id AS "id!: i64", b.date, b.amount_minor, b.note, u.display_name AS "who?",
             b.accepted_at, b.undone_at
           FROM balance_adjustments b LEFT JOIN users u ON u.id = b.accepted_by
           WHERE b.account_id = ? ORDER BY b.accepted_at DESC, b.id DESC"#,
        id
    )
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(Adjustment {
                id: row.id,
                date: read_date(row.date)?,
                amount: Money::from_minor(row.amount_minor),
                note: row.note,
                accepted_by: row.who,
                accepted_at: parse_stamp(row.accepted_at)?,
                undone_at: row.undone_at.map(parse_stamp).transpose()?,
            })
        })
        .collect()
}

/// Why an adjustment can't be accepted or undone.
#[derive(Debug, thiserror::Error)]
pub enum AdjustmentError {
    #[error("there's no such account or adjustment")]
    NotFound,
    /// No drift flag, or one at another amount than the one offered.
    #[error("the account's drift isn't flagged at that amount")]
    NoDrift,
    #[error("the adjustment is already undone")]
    AlreadyUndone,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for AdjustmentError {
    fn from(error: sqlx::Error) -> Self {
        AdjustmentError::Db(Error::Sqlx(error))
    }
}

/// Accepts account `id`'s flagged drift, `amount`, as a balance adjustment with `note`, by login
/// `user` at `now`, dated the newest snapshot's date. Its app balance includes it from then on,
/// so drift goes to zero and the flag clears. Resolves to the adjustment's id.
///
/// # Errors
/// [`AdjustmentError::NotFound`] if there's no such account, [`AdjustmentError::NoDrift`] if its
/// drift isn't flagged at `amount`; otherwise only if the database fails.
pub async fn accept_balance_adjustment(
    pool: &SqlitePool,
    id: i64,
    amount: Money,
    note: Option<&str>,
    user: i64,
    now: Timestamp,
) -> Result<i64, AdjustmentError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM accounts WHERE id = ?) AS "exists!: bool""#,
        id
    )
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(AdjustmentError::NotFound);
    }
    // Read under the write lock, so two accepts at once can't both see the flag.
    let drift = drift_on(&mut tx, id).await?;
    let (Some(flag), Some(latest)) = (drift.flag, drift.latest) else {
        return Err(AdjustmentError::NoDrift);
    };
    if flag.drift != amount {
        return Err(AdjustmentError::NoDrift);
    }
    let (date, minor, at) = (latest.as_of.to_string(), amount.minor(), stamp(now));
    let adjustment = sqlx::query_scalar!(
        r#"INSERT INTO balance_adjustments (account_id, date, amount_minor, note, accepted_by, accepted_at)
           VALUES (?, ?, ?, ?, ?, ?) RETURNING id AS "id!: i64""#,
        id,
        date,
        minor,
        note,
        user,
        at
    )
    .fetch_one(&mut *tx)
    .await?;
    shift_snapshots(&mut tx, id, &date, minor).await?;
    tx.commit().await?;
    Ok(adjustment)
}

/// Undoes balance adjustment `adjustment` by login `user` at `now`. The record stays, marked
/// undone, so a later report can count drift that went unexplained; the app balance leaves it
/// out again, so the drift it covered comes back.
///
/// # Errors
/// [`AdjustmentError::NotFound`] if there's no such adjustment, [`AdjustmentError::AlreadyUndone`]
/// if it is; otherwise only if the database fails.
pub async fn undo_balance_adjustment(
    pool: &SqlitePool,
    adjustment: i64,
    user: i64,
    now: Timestamp,
) -> Result<i64, AdjustmentError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let found = sqlx::query!(
        r#"SELECT account_id AS "account_id!: i64", date, amount_minor, undone_at
           FROM balance_adjustments WHERE id = ?"#,
        adjustment
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AdjustmentError::NotFound)?;
    if found.undone_at.is_some() {
        return Err(AdjustmentError::AlreadyUndone);
    }
    let at = stamp(now);
    sqlx::query!(
        "UPDATE balance_adjustments SET undone_at = ?, undone_by = ? WHERE id = ?",
        at,
        user,
        adjustment
    )
    .execute(&mut *tx)
    .await?;
    let back = -found.amount_minor;
    shift_snapshots(&mut tx, found.account_id, &found.date, back).await?;
    tx.commit().await?;
    Ok(found.account_id)
}

/// Adds `minor` to the stored app balances of account `id`'s snapshots dated `date` or later.
async fn shift_snapshots(tx: &mut Tx<'_>, id: i64, date: &str, minor: i64) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE balance_snapshots
         SET app_balance_posted_minor = app_balance_posted_minor + ?3,
             app_balance_pending_minor = app_balance_pending_minor + ?3
         WHERE account_id = ?1 AND as_of >= ?2 AND app_balance_posted_minor IS NOT NULL",
        id,
        date,
        minor
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Records that run `run_id` did `change` to row `transaction_id` of account `account_id`, with
/// the row's date, amount and bank text as they are after it.
pub(crate) async fn record_change(
    tx: &mut Tx<'_>,
    run_id: i64,
    account_id: i64,
    transaction_id: i64,
    change: RowChange,
) -> Result<(), Error> {
    let change = change.as_str();
    sqlx::query!(
        "INSERT INTO sync_row_changes
           (sync_run_id, account_id, transaction_id, change, date, amount_minor, description)
         SELECT ?, ?, id, ?, date, amount_minor, raw_description FROM transactions WHERE id = ?",
        run_id,
        account_id,
        change,
        transaction_id
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn opening(balance: Option<i64>, date: Option<String>) -> Result<Option<Opening>, Error> {
    match (balance, date) {
        (Some(balance), Some(date)) => Ok(Some(Opening {
            balance: Money::from_minor(balance),
            date: read_date(date)?,
        })),
        _ => Ok(None),
    }
}

fn add(a: Money, b: Money) -> Result<Money, Error> {
    a.checked_add(b)
        .ok_or_else(|| unreadable("a balance", format!("{a} + {b}")))
}

fn read_date(text: String) -> Result<Date, Error> {
    text.parse().map_err(|_| unreadable("a date", text))
}

fn unreadable(what: &'static str, value: String) -> Error {
    Error::Unreadable { what, value }
}

#[cfg(test)]
#[path = "balances_tests.rs"]
mod tests;
