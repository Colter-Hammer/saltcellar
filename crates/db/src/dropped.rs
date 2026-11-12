//! **Recently dropped by bank** (docs/spec/m2-bank-data.md#rows-the-bank-stops-sending): pending
//! rows the household had edited, removed when the bank stopped sending them, listed and
//! restorable for 30 days; every dropped row is pruned after that.
//!
//! Restoring *(agent)*: the row comes back from its snapshot with its edits (payee, category,
//! split lines, memo, tags, exclude-from-budget) under its old id when that's free, kept by the
//! household (`kept_by_household`) so sync doesn't drop it again; its bank id stays, so the bank
//! sending it again (or posting it) still finds it. A payee, category or tag deleted since is left
//! off. The transfer it was a leg of isn't in the snapshot and doesn't come back.

use domain::{Date, Money};
use jiff::{SignedDuration, Timestamp};

use crate::{Error, SqlitePool, parse_stamp, stamp};

/// How long a dropped row can be restored, and is kept.
const KEPT_FOR: SignedDuration = SignedDuration::from_hours(30 * 24);

/// A dropped row as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedRow {
    /// The dropped row's id (not the transaction's).
    pub id: i64,
    pub account_id: i64,
    pub account: String,
    pub date: Date,
    pub amount: Money,
    pub payee: Option<String>,
    pub raw_description: String,
    pub category: Option<String>,
    pub split: bool,
    pub memo: Option<String>,
    pub dropped_at: Timestamp,
}

/// The rows listed under Recently dropped by bank at `now`: edited, not restored, dropped in the
/// last 30 days; the latest dropped first.
///
/// # Errors
/// If the database fails, or holds a date or a timestamp it can't read.
pub async fn dropped_rows(pool: &SqlitePool, now: Timestamp) -> Result<Vec<DroppedRow>, Error> {
    let since = stamp(now - KEPT_FOR);
    let found = sqlx::query!(
        r#"SELECT d.id AS "id!: i64", d.account_id, a.name AS account, d.dropped_at,
             d.snapshot ->> '$.transaction.date' AS "date!: String",
             d.snapshot ->> '$.transaction.amount_minor' AS "amount_minor!: i64",
             d.snapshot ->> '$.transaction.raw_description' AS "raw_description!: String",
             d.snapshot ->> '$.transaction.memo' AS "memo?: String",
             p.name AS "payee?", c.name AS "category?",
             json_array_length(d.snapshot, '$.splits') > 0 AS "split!: bool"
           FROM dropped_transactions d
           JOIN accounts a ON a.id = d.account_id
           LEFT JOIN payees p ON p.id = d.snapshot ->> '$.transaction.payee_id'
           LEFT JOIN categories c ON c.id = d.snapshot ->> '$.transaction.category_id'
           WHERE d.user_edited = 1 AND d.restored_at IS NULL AND d.dropped_at >= ?
           ORDER BY d.dropped_at DESC, d.id DESC"#,
        since
    )
    .fetch_all(pool)
    .await?;
    found
        .into_iter()
        .map(|row| {
            Ok(DroppedRow {
                id: row.id,
                account_id: row.account_id,
                account: row.account,
                date: row.date.parse().map_err(|_| Error::Unreadable {
                    what: "a date",
                    value: row.date.clone(),
                })?,
                amount: Money::from_minor(row.amount_minor),
                payee: row.payee,
                raw_description: row.raw_description,
                category: row.category,
                split: row.split,
                memo: row.memo,
                dropped_at: parse_stamp(row.dropped_at)?,
            })
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    /// No such dropped row listed: never edited, restored already, or older than 30 days.
    #[error("there's no such dropped row")]
    NotFound,
    /// The bank has sent a row with its id again since; it's in the list already.
    #[error("the bank sent it again")]
    BackFromBank,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for RestoreError {
    fn from(error: sqlx::Error) -> Self {
        RestoreError::Db(Error::Sqlx(error))
    }
}

/// Restores dropped row `id` for login `user_id` at `now`; resolves to the transaction's id.
///
/// # Errors
/// [`RestoreError::NotFound`], [`RestoreError::BackFromBank`], or if the database fails;
/// nothing is written then.
pub async fn restore_dropped(
    pool: &SqlitePool,
    id: i64,
    user_id: i64,
    now: Timestamp,
) -> Result<i64, RestoreError> {
    let at = stamp(now);
    let since = stamp(now - KEPT_FOR);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let found = sqlx::query!(
        r#"SELECT EXISTS (SELECT 1 FROM transactions t
             WHERE t.account_id = d.account_id
               AND t.external_id = d.snapshot ->> '$.transaction.external_id') AS "back!: bool"
           FROM dropped_transactions d
           WHERE d.id = ? AND d.user_edited = 1 AND d.restored_at IS NULL AND d.dropped_at >= ?"#,
        id,
        since
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RestoreError::NotFound)?;
    if found.back {
        return Err(RestoreError::BackFromBank);
    }
    let restored = sqlx::query_scalar!(
        r#"INSERT INTO transactions
             (id, account_id, date, amount_minor, status, payee_id, raw_description, bank_category,
              memo, category_id, exclude_from_budget, categorized_by, categorization_reason,
              source, external_id, dedupe_key, last_seen_at, kept_by_household, created_at,
              updated_at, updated_by)
           SELECT
             CASE WHEN EXISTS (SELECT 1 FROM transactions o WHERE o.id = s.t ->> '$.id')
                  THEN NULL ELSE s.t ->> '$.id' END,
             s.account_id, s.t ->> '$.date', s.t ->> '$.amount_minor', s.t ->> '$.status',
             (SELECT p.id FROM payees p WHERE p.id = s.t ->> '$.payee_id'),
             s.t ->> '$.raw_description', s.t ->> '$.bank_category', s.t ->> '$.memo',
             (SELECT c.id FROM categories c WHERE c.id = s.t ->> '$.category_id'),
             s.t ->> '$.exclude_from_budget', s.t ->> '$.categorized_by',
             s.t ->> '$.categorization_reason', s.t ->> '$.source', s.t ->> '$.external_id',
             s.t ->> '$.dedupe_key', s.t ->> '$.last_seen_at', 1, s.t ->> '$.created_at', ?, ?
           FROM (SELECT account_id, snapshot -> '$.transaction' AS t
                 FROM dropped_transactions WHERE id = ?) s
           RETURNING id AS "id!: i64""#,
        at,
        user_id,
        id
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO transaction_splits (transaction_id, category_id, amount_minor, memo)
         SELECT ?, l.value ->> '$.category_id', l.value ->> '$.amount_minor',
                l.value ->> '$.memo'
         FROM dropped_transactions d, json_each(d.snapshot, '$.splits') l
         WHERE d.id = ? AND l.value ->> '$.category_id' IN (SELECT id FROM categories)",
        restored,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO transaction_tags (transaction_id, tag_id)
         SELECT ?, g.value
         FROM dropped_transactions d, json_each(d.snapshot, '$.tags') g
         WHERE d.id = ? AND g.value IN (SELECT id FROM tags)",
        restored,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE dropped_transactions SET restored_at = ? WHERE id = ?",
        at,
        id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(restored)
}

/// Deletes the dropped rows older than 30 days at `now`, restored or not.
pub(crate) async fn prune(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    now: Timestamp,
) -> Result<(), Error> {
    let since = stamp(now - KEPT_FOR);
    sqlx::query!(
        "DELETE FROM dropped_transactions WHERE dropped_at < ?",
        since
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}
