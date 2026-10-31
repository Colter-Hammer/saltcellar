//! Settings › Imports: the batches, and undoing one (docs/spec/m1-ledger-import.md#settings--imports).

use crate::{Error, SqlitePool, parse_stamp, stamp};

/// One import batch, as Settings › Imports lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub id: i64,
    pub imported_at: jiff::Timestamp,
    pub format: String,
    pub file_name: String,
    /// The importing login's display name; `None` if that login is gone.
    pub imported_by: Option<String>,
    pub added: i64,
    pub matched: i64,
    /// When it was undone, if it was.
    pub undone_at: Option<jiff::Timestamp>,
}

/// Every batch, the latest first.
///
/// # Errors
/// If the database fails or holds a timestamp it can't read.
pub async fn import_batches(pool: &SqlitePool) -> Result<Vec<Batch>, Error> {
    let rows = sqlx::query!(
        r#"SELECT b.id AS "id!: i64", b.imported_at, b.format, b.file_name,
                  u.display_name AS "imported_by?", b.added, b.matched, b.undone_at
           FROM import_batches b LEFT JOIN users u ON u.id = b.imported_by
           ORDER BY b.imported_at DESC, b.id DESC"#
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(Batch {
                id: row.id,
                imported_at: parse_stamp(row.imported_at)?,
                format: row.format,
                file_name: row.file_name,
                imported_by: row.imported_by,
                added: row.added,
                matched: row.matched,
                undone_at: row.undone_at.map(parse_stamp).transpose()?,
            })
        })
        .collect()
}

/// What an undo removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Undone {
    pub rows_deleted: u64,
    pub accounts_deleted: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum UndoError {
    #[error("there's no such import")]
    NotFound,
    #[error("that import was already undone")]
    AlreadyUndone,
    /// Rows the batch inserted were changed since, and the undo wasn't confirmed.
    #[error("{0} rows were edited since the import")]
    Edited(u64),
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for UndoError {
    fn from(error: sqlx::Error) -> Self {
        UndoError::Db(Error::Sqlx(error))
    }
}

/// The transactions a batch inserted, as a subquery on its id (`?1`).
const INSERTED: &str =
    "(SELECT transaction_id FROM import_batch_rows WHERE batch_id = ?1 AND outcome = 'inserted')";

/// Undoes a batch in one database transaction: deletes the rows it inserted with their split
/// lines, tags and transfers, restores `prior_fields` on the rows it matched, deletes the accounts
/// it created that have no rows left, the payees and tags its rows used that nothing else uses
/// *(agent for tags)*, and marks it undone. Categories stay.
///
/// A row counts as edited when `categorized_by = 'user'` or it was updated after it was written
/// (`updated_by` set, or `updated_at` moved) *(agent)*. With edited rows, nothing happens unless
/// `confirmed`.
///
/// # Errors
/// [`UndoError::NotFound`], [`UndoError::AlreadyUndone`], or [`UndoError::Edited`] with the
/// count of edited rows when not `confirmed`; each leaves everything as it was.
pub async fn undo_import(
    pool: &SqlitePool,
    batch_id: i64,
    confirmed: bool,
    at: jiff::Timestamp,
) -> Result<Undone, UndoError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let batch = sqlx::query_scalar!(
        "SELECT undone_at FROM import_batches WHERE id = ?",
        batch_id
    )
    .fetch_optional(&mut *tx)
    .await?;
    match batch {
        None => return Err(UndoError::NotFound),
        Some(Some(_)) => return Err(UndoError::AlreadyUndone),
        Some(None) => {}
    }

    let edited = sqlx::query_scalar!(
        r#"SELECT count(*) AS "edited!: i64" FROM transactions t
           JOIN import_batch_rows ib ON ib.transaction_id = t.id
           WHERE ib.batch_id = ? AND ib.outcome = 'inserted'
             AND (t.categorized_by = 'user' OR t.updated_by IS NOT NULL
                  OR t.updated_at <> t.created_at)"#,
        batch_id
    )
    .fetch_one(&mut *tx)
    .await?;
    if edited > 0 && !confirmed {
        return Err(UndoError::Edited(edited.unsigned_abs()));
    }

    // Rows the batch matched get back what it filled.
    sqlx::query(
        "UPDATE transactions SET
           category_id = json_extract(ib.prior_fields, '$.category_id'),
           categorized_by = json_extract(ib.prior_fields, '$.categorized_by'),
           memo = json_extract(ib.prior_fields, '$.memo'),
           payee_id = json_extract(ib.prior_fields, '$.payee_id')
         FROM import_batch_rows ib
         WHERE ib.batch_id = ?1 AND ib.transaction_id = transactions.id
           AND ib.outcome = 'matched' AND ib.prior_fields IS NOT NULL",
    )
    .bind(batch_id)
    .execute(&mut *tx)
    .await?;

    let used = Used::of(&mut tx, batch_id).await?;

    for statement in [
        "DELETE FROM transfers WHERE from_txn_id IN {INSERTED} OR to_txn_id IN {INSERTED}",
        "UPDATE transactions SET possible_duplicate_of = NULL WHERE possible_duplicate_of IN {INSERTED}",
        "DELETE FROM transaction_splits WHERE transaction_id IN {INSERTED}",
        "DELETE FROM transaction_tags WHERE transaction_id IN {INSERTED}",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(
            statement.replace("{INSERTED}", INSERTED),
        ))
        .bind(batch_id)
        .execute(&mut *tx)
        .await?;
    }
    // The batch's own row for each goes with it (ON DELETE CASCADE).
    let rows_deleted = sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM transactions WHERE id IN {INSERTED}"
    )))
    .bind(batch_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // Its snapshots go with the accounts (ON DELETE CASCADE).
    let accounts_deleted = sqlx::query!(
        "DELETE FROM accounts WHERE created_by_batch_id = ?
           AND NOT EXISTS (SELECT 1 FROM transactions WHERE account_id = accounts.id)",
        batch_id
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    used.delete_unused(&mut tx).await?;

    let at = stamp(at);
    sqlx::query!(
        "UPDATE import_batches SET undone_at = ? WHERE id = ?",
        at,
        batch_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Undone {
        rows_deleted,
        accounts_deleted,
    })
}

/// The payees and tags a batch's inserted rows used, to tidy once the rows are gone.
struct Used {
    payees: Vec<i64>,
    tags: Vec<i64>,
}

impl Used {
    async fn of(tx: &mut sqlx::SqliteConnection, batch_id: i64) -> Result<Used, sqlx::Error> {
        let payees = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT DISTINCT payee_id FROM transactions
             WHERE payee_id IS NOT NULL AND id IN {INSERTED}"
        )))
        .bind(batch_id)
        .fetch_all(&mut *tx)
        .await?;
        let tags = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT DISTINCT tag_id FROM transaction_tags WHERE transaction_id IN {INSERTED}"
        )))
        .bind(batch_id)
        .fetch_all(&mut *tx)
        .await?;
        Ok(Used { payees, tags })
    }

    /// Deletes those no transaction or tagging uses now (a payee's aliases go with it).
    async fn delete_unused(self, tx: &mut sqlx::SqliteConnection) -> Result<(), sqlx::Error> {
        for id in self.payees {
            sqlx::query!(
                "DELETE FROM payees WHERE id = ?1
                   AND NOT EXISTS (SELECT 1 FROM transactions WHERE payee_id = ?1)",
                id
            )
            .execute(&mut *tx)
            .await?;
        }
        for id in self.tags {
            sqlx::query!(
                "DELETE FROM tags WHERE id = ?1
                   AND NOT EXISTS (SELECT 1 FROM transaction_tags WHERE tag_id = ?1)",
                id
            )
            .execute(&mut *tx)
            .await?;
        }
        Ok(())
    }
}
