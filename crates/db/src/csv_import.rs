//! CSV and OFX import (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import): the mappings saved per
//! account, the rows an import is matched against, and committing one as an import batch.

use std::collections::{BTreeSet, HashMap};

use domain::csv_import::{self, Entry, FileRow, Outcome, Stored};
use domain::import::dedupe_key;
use domain::{Date, Money};
use sqlx::SqliteConnection;

use crate::{Error, SqlitePool};

/// Days before the import that a row is still news; older rows that no rule categorizes stay out
/// of Review (`review_quiet`, used from milestone 3).
const QUIET_AFTER_DAYS: i32 = 90;

/// A mapping saved for an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedMapping {
    /// The header it was saved for (`domain::csv_import::header_signature`).
    pub header_signature: String,
    /// Its JSON, as the API wrote it.
    pub config: String,
}

/// Whether account `id` exists.
///
/// # Errors
/// If the database fails.
pub async fn import_account_exists(pool: &SqlitePool, id: i64) -> Result<bool, Error> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM accounts WHERE id = ?) AS "exists!: bool""#,
        id
    )
    .fetch_one(pool)
    .await?)
}

/// The accounts that can take a file, by id and name: open ones that keep rows.
///
/// # Errors
/// If the database fails.
pub async fn import_accounts(pool: &SqlitePool) -> Result<Vec<(i64, String)>, Error> {
    Ok(sqlx::query!(
        r#"SELECT id AS "id!: i64", name FROM accounts
           WHERE closed = 0 AND has_transactions = 1 ORDER BY sort_order, id"#
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.id, row.name))
    .collect())
}

/// The mappings saved for an account, the latest saved first.
///
/// # Errors
/// If the database fails.
pub async fn import_mappings(
    pool: &SqlitePool,
    account_id: i64,
) -> Result<Vec<SavedMapping>, Error> {
    Ok(sqlx::query_as!(
        SavedMapping,
        "SELECT header_signature, config FROM import_mappings
         WHERE account_id = ? ORDER BY updated_at DESC, id DESC",
        account_id
    )
    .fetch_all(pool)
    .await?)
}

/// What an import of `rows` into an account would do, as the preview counts it and the commit
/// writes it.
///
/// # Errors
/// If the database fails or holds a date or amount it can't read.
pub async fn plan_csv_import(
    pool: &SqlitePool,
    account_id: i64,
    rows: Vec<FileRow>,
) -> Result<Vec<Entry>, Error> {
    let mut conn = pool.acquire().await?;
    plan(&mut conn, account_id, rows).await
}

async fn plan(
    conn: &mut SqliteConnection,
    account_id: i64,
    rows: Vec<FileRow>,
) -> Result<Vec<Entry>, Error> {
    let stored = match csv_import::window(&rows) {
        Some((from, to)) => stored_rows(conn, account_id, from, to).await?,
        None => Vec::new(),
    };
    let seen = sqlx::query!(
        r#"SELECT r.import_ref, COUNT(*) AS "count!: i64"
           FROM import_batch_rows r
           JOIN import_batches b ON b.id = r.batch_id AND b.undone_at IS NULL
           JOIN transactions t ON t.id = r.transaction_id
           WHERE t.account_id = ? GROUP BY r.import_ref"#,
        account_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| (row.import_ref, row.count.unsigned_abs()))
    .collect::<HashMap<_, _>>();
    Ok(csv_import::plan(rows, &seen, &stored))
}

async fn stored_rows(
    conn: &mut SqliteConnection,
    account_id: i64,
    from: Date,
    to: Date,
) -> Result<Vec<Stored>, Error> {
    let (from, to) = (from.to_string(), to.to_string());
    sqlx::query!(
        "SELECT id AS \"id!: i64\", date, amount_minor, raw_description FROM transactions
         WHERE account_id = ? AND date BETWEEN ? AND ?",
        account_id,
        from,
        to
    )
    .fetch_all(conn)
    .await?
    .into_iter()
    .map(|row| stored(row.id, row.date, row.amount_minor, row.raw_description))
    .collect()
}

/// The stored rows with these ids, for the rows to check to show what they might be.
///
/// # Errors
/// If the database fails or holds a date it can't read.
pub async fn csv_matched_rows(pool: &SqlitePool, ids: &[i64]) -> Result<Vec<Stored>, Error> {
    let ids = serde_json::to_string(ids).map_err(|error| Error::Unreadable {
        what: "row ids",
        value: error.to_string(),
    })?;
    sqlx::query!(
        "SELECT id AS \"id!: i64\", date, amount_minor, raw_description FROM transactions
         WHERE id IN (SELECT value FROM json_each(?))",
        ids
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| stored(row.id, row.date, row.amount_minor, row.raw_description))
    .collect()
}

fn stored(
    id: i64,
    date: String,
    amount_minor: i64,
    raw_description: String,
) -> Result<Stored, Error> {
    Ok(Stored {
        id,
        date: date.parse().map_err(|_| Error::Unreadable {
            what: "a transaction date",
            value: date,
        })?,
        amount: Money::from_minor(amount_minor),
        raw_description,
    })
}

/// A CSV import to commit.
#[derive(Debug)]
pub struct CsvCommit<'a> {
    pub account_id: i64,
    /// The uploaded file's name, for Settings › Imports.
    pub file_name: &'a str,
    /// The header the mapping is saved under.
    pub header_signature: &'a str,
    /// The mapping's JSON, saved for the account and header.
    pub config: &'a str,
    /// The rows the mapping read.
    pub rows: Vec<FileRow>,
    /// The places (`FileRow::row`) of the rows to check that the household said to add.
    pub keep: &'a BTreeSet<usize>,
    /// Records the mapping couldn't read, which count as skipped.
    pub unreadable: u64,
    /// The login importing.
    pub imported_by: i64,
    /// The import day in the household timezone.
    pub today: Date,
}

/// What a committed CSV import wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvCommitted {
    pub batch_id: i64,
    /// Rows inserted.
    pub added: u64,
    /// Rows already there: matched to a stored row, or seen in an earlier import.
    pub matched: u64,
    /// Rows left out: unreadable ones and rows to check that weren't kept.
    pub skipped: u64,
}

/// Commits a CSV import in one database transaction: the mapping saved for the account and
/// header, the rows that are new (and the rows to check that were kept) as `source = 'import'`
/// rows with their bank category, the bank category a matched row gains if it had none, the
/// opening balance moved back over rows older than it, and the import batch recording it all.
/// The rows are matched again inside the transaction, so the batch is what the preview counted
/// unless something changed in between.
///
/// # Errors
/// If the database fails.
pub async fn commit_csv_import(
    pool: &SqlitePool,
    commit: CsvCommit<'_>,
) -> Result<CsvCommitted, Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let mapping_id = sqlx::query_scalar!(
        r#"INSERT INTO import_mappings (account_id, header_signature, config)
           VALUES (?, ?, ?)
           ON CONFLICT (account_id, header_signature)
             DO UPDATE SET config = excluded.config,
                           updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
           RETURNING id AS "id!: i64""#,
        commit.account_id,
        commit.header_signature,
        commit.config
    )
    .fetch_one(&mut *tx)
    .await?;
    let done = write_batch(
        &mut tx,
        Batch {
            account_id: commit.account_id,
            format: "csv",
            file_name: commit.file_name,
            mapping_id: Some(mapping_id),
            rows: commit.rows,
            keep: commit.keep,
            unreadable: commit.unreadable,
            imported_by: commit.imported_by,
            today: commit.today,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(done)
}

/// An OFX, QFX or QBO import to commit: a CSV one's fields, without a mapping.
#[derive(Debug)]
pub struct OfxCommit<'a> {
    pub account_id: i64,
    pub file_name: &'a str,
    /// The statement's transactions, each with its FITID as the `import_ref`.
    pub rows: Vec<FileRow>,
    pub keep: &'a BTreeSet<usize>,
    pub unreadable: u64,
    pub imported_by: i64,
    pub today: Date,
}

/// Commits an OFX import (`format = 'ofx'`) as [`commit_csv_import`] does a CSV one, with no
/// mapping to save.
///
/// # Errors
/// If the database fails.
pub async fn commit_ofx_import(
    pool: &SqlitePool,
    commit: OfxCommit<'_>,
) -> Result<CsvCommitted, Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let done = write_batch(
        &mut tx,
        Batch {
            account_id: commit.account_id,
            format: "ofx",
            file_name: commit.file_name,
            mapping_id: None,
            rows: commit.rows,
            keep: commit.keep,
            unreadable: commit.unreadable,
            imported_by: commit.imported_by,
            today: commit.today,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(done)
}

/// A file's rows to write as one import batch.
struct Batch<'a> {
    account_id: i64,
    format: &'static str,
    file_name: &'a str,
    mapping_id: Option<i64>,
    rows: Vec<FileRow>,
    keep: &'a BTreeSet<usize>,
    unreadable: u64,
    imported_by: i64,
    today: Date,
}

async fn write_batch(tx: &mut SqliteConnection, batch: Batch<'_>) -> Result<CsvCommitted, Error> {
    let account_id = batch.account_id;
    let entries = plan(&mut *tx, account_id, batch.rows).await?;
    let batch_id = sqlx::query_scalar!(
        r#"INSERT INTO import_batches
             (account_id, format, file_name, mapping_id, added, matched, skipped, imported_by)
           VALUES (?, ?, ?, ?, 0, 0, 0, ?) RETURNING id AS "id!: i64""#,
        account_id,
        batch.format,
        batch.file_name,
        batch.mapping_id,
        batch.imported_by
    )
    .fetch_one(&mut *tx)
    .await?;

    let quiet_before = batch
        .today
        .plus_days(-QUIET_AFTER_DAYS)
        .unwrap_or(batch.today);
    let mut done = CsvCommitted {
        batch_id,
        added: 0,
        matched: 0,
        skipped: batch.unreadable,
    };
    for entry in entries {
        let row = &entry.row;
        match entry.outcome {
            Outcome::Seen => done.matched += 1,
            Outcome::Matched(id) => {
                done.matched += 1;
                matched_row(&mut *tx, batch_id, id, row).await?;
            }
            Outcome::Check(_) if !batch.keep.contains(&row.row) => done.skipped += 1,
            Outcome::New | Outcome::Check(_) => {
                done.added += 1;
                new_row(&mut *tx, batch_id, account_id, row, row.date < quiet_before).await?;
            }
        }
    }

    // Rows older than the opening date move the opening balance back; Undo puts it back.
    if let Some(before) = crate::balances::move_opening_back(&mut *tx, account_id).await? {
        let (minor, date) = (before.balance.minor(), before.date.to_string());
        sqlx::query!(
            "INSERT INTO import_batch_openings
               (batch_id, account_id, opening_balance_minor, opening_date)
             VALUES (?, ?, ?, ?)",
            batch_id,
            account_id,
            minor,
            date
        )
        .execute(&mut *tx)
        .await?;
    }

    let (added, matched, skipped) = (
        i64::try_from(done.added).unwrap_or(i64::MAX),
        i64::try_from(done.matched).unwrap_or(i64::MAX),
        i64::try_from(done.skipped).unwrap_or(i64::MAX),
    );
    sqlx::query!(
        "UPDATE import_batches SET added = ?, matched = ?, skipped = ? WHERE id = ?",
        added,
        matched,
        skipped,
        batch_id
    )
    .execute(&mut *tx)
    .await?;
    Ok(done)
}

/// A stored row the file matched: it keeps its amount, date, status and text, and gains the
/// file's bank category when it has none.
async fn matched_row(
    tx: &mut SqliteConnection,
    batch_id: i64,
    id: i64,
    row: &FileRow,
) -> Result<(), Error> {
    let set =
        match &row.category {
            Some(category) => sqlx::query!(
                "UPDATE transactions SET bank_category = ? WHERE id = ? AND bank_category IS NULL",
                category,
                id
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
                > 0,
            None => false,
        };
    sqlx::query!(
        "INSERT INTO import_batch_rows (batch_id, transaction_id, outcome, import_ref, set_bank_category)
         VALUES (?, ?, 'matched', ?, ?)",
        batch_id,
        id,
        row.import_ref,
        set
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn new_row(
    tx: &mut SqliteConnection,
    batch_id: i64,
    account_id: i64,
    row: &FileRow,
    quiet: bool,
) -> Result<(), Error> {
    let (date, amount) = (row.date.to_string(), row.amount.minor());
    let dedupe = dedupe_key(account_id, row.date, row.amount, &row.description);
    let id = sqlx::query_scalar!(
        r#"INSERT INTO transactions
             (account_id, date, amount_minor, status, raw_description, bank_category, memo,
              source, dedupe_key, review_quiet)
           VALUES (?, ?, ?, 'posted', ?, ?, ?, 'import', ?, ?)
           RETURNING id AS "id!: i64""#,
        account_id,
        date,
        amount,
        row.description,
        row.category,
        row.memo,
        dedupe,
        quiet
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO import_batch_rows
           (batch_id, transaction_id, outcome, import_ref, set_bank_category)
         VALUES (?, ?, 'inserted', ?, ?)",
        batch_id,
        id,
        row.import_ref,
        row.category.is_some()
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
