//! What a sync run writes into one account's rows (docs/spec/m2-bank-data.md#matching-a-synced-row
//! and #rows-the-bank-stops-sending): each row the feed sent matched to a stored one (or not) by
//! `domain::matching`, then the rows the bank stopped sending: pending ones counted and removed at
//! two misses, posted ones flagged `not_seen_at_bank`. Also deleting a synced row
//! (#deleting-a-synced-row), whose id is kept so the window never brings it back.
//!
//! The household's fields (payee, category, splits, memo, tags, exclude-from-budget) are never
//! touched once a row exists.

use std::collections::BTreeSet;

use domain::Date;
use domain::feed::SyncedRow;
use domain::matching::{Match, StoredRow, match_rows};
use jiff::{SignedDuration, Timestamp};

use crate::{Error, SqlitePool, stamp};

/// What applying one account's rows did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    pub added: u64,
    /// Rows whose sync-owned fields or bank id changed.
    pub updated: u64,
}

/// Runs in a row a pending row may be missing before it is removed.
const PENDING_MISSES: i64 = 2;
/// Days after the window's start before a missing posted row counts *(agent)*: rows transacted
/// before the window that posted inside it are cut by the Bridge.
const WINDOW_MARGIN_DAYS: i32 = 5;
/// How long a posted row may go unseen before it is flagged.
const NOT_SEEN_FOR: SignedDuration = SignedDuration::from_hours(7 * 24);

/// Applies the rows the feed sent for account `account_id` in a run that succeeded for it, seen
/// at `now`, in one transaction. `rows` is everything this run's response(s) held for the
/// account, and `window_start` the UTC date the earliest request started on.
///
/// Each row: an id the household deleted is skipped; otherwise it is matched
/// (`domain::matching`) and a matched stored row takes the feed's id, amount, date, status and
/// raw description, becoming the provider's; an unmatched one is inserted with a payee found by
/// alias or exact name, or made (docs/spec/m2-bank-data.md#payees-for-synced-rows-agent). A
/// split row whose amount changes is flagged `amount_changed_split`, never adjusted.
///
/// Then the rows the response didn't hold: a pending row's miss count goes up, and at two it is
/// removed, kept in `dropped_transactions` (`user_edited` when the household had touched it); a
/// posted provider row dated at least 5 days into the window and unseen for 7 days is flagged
/// `not_seen_at_bank`, its `last_seen_at` kept. A flagged row that comes back loses the flag.
///
/// # Errors
/// If the database fails; nothing is written then.
pub async fn apply_synced_rows(
    pool: &SqlitePool,
    account_id: i64,
    rows: &[SyncedRow],
    window_start: Date,
    now: Timestamp,
) -> Result<Applied, Error> {
    let at = stamp(now);
    let mut applied = Applied::default();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let deleted: BTreeSet<String> = sqlx::query_scalar!(
        "SELECT external_id FROM deleted_bank_rows WHERE account_id = ?",
        account_id
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
    let rows: Vec<SyncedRow> = rows
        .iter()
        .filter(|row| !deleted.contains(&row.external_id))
        .cloned()
        .collect();
    let ids = serde_json::Value::from(
        rows.iter()
            .map(|row| row.external_id.as_str())
            .collect::<Vec<_>>(),
    )
    .to_string();

    let stored = candidates(&mut tx, account_id, &rows, &ids).await?;
    for (row, found) in rows.iter().zip(match_rows(&rows, &stored)) {
        match found {
            Match::SameId(id) => {
                if take_over(&mut tx, id, account_id, row, &at).await? {
                    applied.updated += 1;
                }
            }
            Match::Posts(id) | Match::Reissued(id) | Match::Adopts(id) => {
                take_over(&mut tx, id, account_id, row, &at).await?;
                applied.updated += 1;
            }
            Match::PossibleDuplicate(of) => {
                insert(&mut tx, account_id, row, Some(of), &at).await?;
                applied.added += 1;
            }
            Match::New => {
                insert(&mut tx, account_id, row, None, &at).await?;
                applied.added += 1;
            }
        }
    }

    missed_pending(&mut tx, account_id, &ids, &at).await?;
    let from = window_start
        .plus_days(WINDOW_MARGIN_DAYS)
        .unwrap_or(window_start)
        .to_string();
    let unseen_since = stamp(now - NOT_SEEN_FOR);
    sqlx::query!(
        "UPDATE transactions SET sync_flag = 'not_seen_at_bank'
         WHERE account_id = ? AND source = 'provider' AND status = 'posted'
           AND external_id IS NOT NULL
           AND external_id NOT IN (SELECT value FROM json_each(?))
           AND date >= ? AND last_seen_at < ? AND sync_flag IS NULL",
        account_id,
        ids,
        from,
        unseen_since
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(applied)
}

/// The account's stored rows that `rows` could match: those carrying one of their ids (`ids`, a
/// JSON array), and every row dated from a week before the earliest of them.
async fn candidates(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: i64,
    rows: &[SyncedRow],
    ids: &str,
) -> Result<Vec<StoredRow>, Error> {
    let Some(earliest) = rows.iter().map(|row| row.date).min() else {
        return Ok(Vec::new());
    };
    let from = domain::matching::earliest_candidate(earliest)
        .unwrap_or(earliest)
        .to_string();
    let found = sqlx::query!(
        r#"SELECT id AS "id!: i64", external_id, date, amount_minor, status, raw_description,
             source
           FROM transactions
           WHERE account_id = ?
             AND (date >= ? OR external_id IN (SELECT value FROM json_each(?)))"#,
        account_id,
        from,
        ids
    )
    .fetch_all(&mut **tx)
    .await?;
    found
        .into_iter()
        .map(|row| {
            Ok(StoredRow {
                id: row.id,
                external_id: row.external_id,
                date: row.date.parse().map_err(|_| Error::Unreadable {
                    what: "a transaction date",
                    value: row.date.clone(),
                })?,
                amount: domain::Money::from_minor(row.amount_minor),
                pending: row.status == "pending",
                raw_description: row.raw_description,
                imported: row.source == "import",
            })
        })
        .collect()
}

/// Gives stored row `id` the feed row's id and sync-owned fields; it becomes the provider's. A
/// `not_seen_at_bank` flag goes, since the bank sent it. Resolves to whether any of the
/// sync-owned fields changed.
async fn take_over(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: i64,
    account_id: i64,
    row: &SyncedRow,
    at: &str,
) -> Result<bool, Error> {
    let date = row.date.to_string();
    let amount = row.amount.minor();
    let status = if row.pending { "pending" } else { "posted" };
    let key = domain::import::dedupe_key(account_id, row.date, row.amount, &row.raw_description);
    let stored = sqlx::query!(
        r#"SELECT date, amount_minor, status, raw_description,
             EXISTS (SELECT 1 FROM transaction_splits s WHERE s.transaction_id = t.id)
               AS "split!: bool"
           FROM transactions t WHERE id = ?"#,
        id
    )
    .fetch_one(&mut **tx)
    .await?;
    let changed = stored.date != date
        || stored.amount_minor != amount
        || stored.status != status
        || stored.raw_description != row.raw_description;
    let split_off = stored.split && stored.amount_minor != amount;
    sqlx::query!(
        "UPDATE transactions
         SET external_id = ?, source = 'provider', date = ?, amount_minor = ?, status = ?,
             raw_description = ?, dedupe_key = ?, last_seen_at = ?, pending_miss_count = 0,
             updated_at = CASE WHEN ? THEN ? ELSE updated_at END,
             sync_flag = CASE WHEN ? THEN 'amount_changed_split'
                              WHEN sync_flag = 'not_seen_at_bank' THEN NULL
                              ELSE sync_flag END
         WHERE id = ?",
        row.external_id,
        date,
        amount,
        status,
        row.raw_description,
        key,
        at,
        changed,
        at,
        split_off,
        id
    )
    .execute(&mut **tx)
    .await?;
    Ok(changed)
}

/// Inserts the feed row as the provider's, flagged `possible_duplicate` of `duplicate_of` when
/// given.
async fn insert(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: i64,
    row: &SyncedRow,
    duplicate_of: Option<i64>,
    at: &str,
) -> Result<(), Error> {
    let date = row.date.to_string();
    let amount = row.amount.minor();
    let status = if row.pending { "pending" } else { "posted" };
    let key = domain::import::dedupe_key(account_id, row.date, row.amount, &row.raw_description);
    let flag = duplicate_of.map(|_| "possible_duplicate");
    let payee = payee(tx, &row.payee_text).await?;
    sqlx::query!(
        "INSERT INTO transactions
           (account_id, date, amount_minor, status, payee_id, raw_description, source,
            external_id, dedupe_key, last_seen_at, sync_flag, possible_duplicate_of)
         VALUES (?, ?, ?, ?, ?, ?, 'provider', ?, ?, ?, ?, ?)",
        account_id,
        date,
        amount,
        status,
        payee,
        row.raw_description,
        row.external_id,
        key,
        at,
        flag,
        duplicate_of
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Counts a miss on each pending row of the account with a bank id not in `ids` (a JSON array),
/// and removes those that reach two.
async fn missed_pending(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: i64,
    ids: &str,
    at: &str,
) -> Result<(), Error> {
    let missed = sqlx::query!(
        r#"UPDATE transactions SET pending_miss_count = pending_miss_count + 1
           WHERE account_id = ? AND status = 'pending' AND external_id IS NOT NULL
             AND external_id NOT IN (SELECT value FROM json_each(?))
           RETURNING id AS "id!: i64", pending_miss_count"#,
        account_id,
        ids
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in missed {
        if row.pending_miss_count >= PENDING_MISSES {
            drop_row(tx, row.id, at).await?;
        }
    }
    Ok(())
}

/// Removes pending row `id` the bank stopped sending, keeping it whole in
/// `dropped_transactions`: `user_edited` when the household categorized it by hand, edited it,
/// gave it a memo, a split or a tag, or excluded it from budgets *(agent)*.
async fn drop_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: i64,
    at: &str,
) -> Result<(), Error> {
    sqlx::query!(
        r#"INSERT INTO dropped_transactions (account_id, snapshot, user_edited, dropped_at)
           SELECT t.account_id,
             json_object(
               'transaction', json_object(
                 'id', t.id, 'date', t.date, 'amount_minor', t.amount_minor,
                 'status', t.status, 'payee_id', t.payee_id,
                 'raw_description', t.raw_description, 'bank_category', t.bank_category,
                 'memo', t.memo, 'category_id', t.category_id,
                 'exclude_from_budget', t.exclude_from_budget,
                 'categorized_by', t.categorized_by,
                 'categorization_reason', t.categorization_reason, 'source', t.source,
                 'external_id', t.external_id, 'dedupe_key', t.dedupe_key,
                 'last_seen_at', t.last_seen_at, 'sync_flag', t.sync_flag,
                 'created_at', t.created_at, 'updated_at', t.updated_at,
                 'updated_by', t.updated_by),
               'splits', json((SELECT json_group_array(json_object(
                   'category_id', s.category_id, 'amount_minor', s.amount_minor,
                   'memo', s.memo))
                 FROM transaction_splits s WHERE s.transaction_id = t.id)),
               'tags', json((SELECT json_group_array(g.tag_id)
                 FROM transaction_tags g WHERE g.transaction_id = t.id))),
             t.categorized_by IS 'user' OR t.updated_by IS NOT NULL OR t.memo IS NOT NULL
               OR t.exclude_from_budget = 1
               OR EXISTS (SELECT 1 FROM transaction_splits s WHERE s.transaction_id = t.id)
               OR EXISTS (SELECT 1 FROM transaction_tags g WHERE g.transaction_id = t.id),
             ?
           FROM transactions t WHERE t.id = ?"#,
        at,
        id
    )
    .execute(&mut **tx)
    .await?;
    remove(tx, id).await
}

/// Deletes row `id`; a row flagged as a possible duplicate of it isn't one any more.
async fn remove(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, id: i64) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE transactions
         SET possible_duplicate_of = NULL,
             sync_flag = CASE WHEN sync_flag IN ('possible_duplicate', 'possible_manual_match')
                              THEN NULL ELSE sync_flag END
         WHERE possible_duplicate_of = ?",
        id
    )
    .execute(&mut **tx)
    .await?;
    // Its splits, tags and transfer go with it (ON DELETE CASCADE).
    sqlx::query!("DELETE FROM transactions WHERE id = ?", id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Why a row can't be deleted as a bank row.
#[derive(Debug, thiserror::Error)]
pub enum DeleteBankRowError {
    #[error("there's no such transaction")]
    NotFound,
    /// It has no bank id: nothing for sync to bring back.
    #[error("the transaction has no bank id")]
    NotFromBank,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for DeleteBankRowError {
    fn from(error: sqlx::Error) -> Self {
        DeleteBankRowError::Db(Error::Sqlx(error))
    }
}

/// Deletes row `id`, which carries a bank id, for login `user_id` at `now`. Its id goes into
/// `deleted_bank_rows`, so the next sync skips it instead of bringing it back.
///
/// # Errors
/// [`DeleteBankRowError::NotFound`], [`DeleteBankRowError::NotFromBank`], or if the database
/// fails; nothing is deleted then.
pub async fn delete_bank_row(
    pool: &SqlitePool,
    id: i64,
    user_id: i64,
    now: Timestamp,
) -> Result<(), DeleteBankRowError> {
    let at = stamp(now);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let found = sqlx::query!(
        "SELECT account_id, external_id FROM transactions WHERE id = ?",
        id
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(found) = found else {
        return Err(DeleteBankRowError::NotFound);
    };
    let Some(external_id) = found.external_id else {
        return Err(DeleteBankRowError::NotFromBank);
    };
    sqlx::query!(
        "INSERT INTO deleted_bank_rows (account_id, external_id, deleted_by, deleted_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT (account_id, external_id) DO NOTHING",
        found.account_id,
        external_id,
        user_id,
        at
    )
    .execute(&mut *tx)
    .await?;
    remove(&mut tx, id).await?;
    tx.commit().await?;
    Ok(())
}

/// The payee for a new synced row from `text`: the one an alias of its normalized text points
/// at, else the payee named exactly `text` (made if new) with that alias added. None when the
/// text normalizes to nothing.
async fn payee(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    text: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let alias = domain::import::normalize(text);
    if alias.is_empty() {
        return Ok(None);
    }
    let known = sqlx::query_scalar!(
        r#"SELECT payee_id AS "id!: i64" FROM payee_aliases WHERE normalized_text = ?"#,
        alias
    )
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(id) = known {
        return Ok(Some(id));
    }
    sqlx::query!(
        "INSERT INTO payees (name) VALUES (?) ON CONFLICT (name) DO NOTHING",
        text
    )
    .execute(&mut **tx)
    .await?;
    let id = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM payees WHERE name = ?"#,
        text
    )
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query!(
        "INSERT INTO payee_aliases (normalized_text, payee_id) VALUES (?, ?)
         ON CONFLICT (normalized_text) DO NOTHING",
        alias,
        id
    )
    .execute(&mut **tx)
    .await?;
    Ok(Some(id))
}

#[cfg(test)]
#[path = "synced_rows_tests.rs"]
mod tests;
