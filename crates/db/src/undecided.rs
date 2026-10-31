//! From Actual, undecided (docs/spec/m1-ledger-import.md#from-actual-undecided): the imported rows
//! whose Actual category was left on "Decide later", grouped by that category, and the one edit
//! this milestone allows, giving them a category. A row is undecided while its import isn't undone
//! and it has no category and no split lines.

use domain::Money;

use crate::{Error, SqlitePool, stamp};

/// The Actual category some undecided rows came in under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndecidedGroup {
    /// The category's name in Actual; empty if the export didn't keep it.
    pub actual_category: String,
    pub rows: i64,
    /// Their amounts summed, signed.
    pub total: Money,
}

/// Every Actual category with undecided rows, by name ignoring case.
///
/// # Errors
/// If the database fails.
pub async fn undecided_groups(pool: &SqlitePool) -> Result<Vec<UndecidedGroup>, Error> {
    let groups = sqlx::query!(
        r#"SELECT u.actual_category AS "actual_category!: String",
                  count(*) AS "rows!: i64",
                  coalesce(sum(t.amount_minor), 0) AS "total!: i64"
           FROM transactions t
           JOIN (SELECT ib.transaction_id AS id, min(coalesce(ib.source_category, '')) AS actual_category
                 FROM import_batch_rows ib JOIN import_batches b ON b.id = ib.batch_id
                 WHERE b.undone_at IS NULL AND ib.source_disposition = 'undecided'
                 GROUP BY ib.transaction_id) u ON u.id = t.id
           WHERE t.category_id IS NULL
             AND t.id NOT IN (SELECT transaction_id FROM transaction_splits)
           GROUP BY u.actual_category
           ORDER BY lower(u.actual_category), u.actual_category"#
    )
    .fetch_all(pool)
    .await?;
    Ok(groups
        .into_iter()
        .map(|group| UndecidedGroup {
            actual_category: group.actual_category,
            rows: group.rows,
            total: Money::from_minor(group.total),
        })
        .collect())
}

/// Who gave the category, and when.
#[derive(Debug, Clone)]
pub struct Decision<'a> {
    pub category_id: i64,
    pub user_id: i64,
    /// The login's display name, for the reason "Set by <name>" *(agent)*.
    pub display_name: &'a str,
    pub at: jiff::Timestamp,
}

#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    #[error("there's no such category")]
    NoSuchCategory,
    /// No such transaction, or it isn't (or is no longer) one of the undecided rows.
    #[error("there's no such undecided row")]
    NotUndecided,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for DecideError {
    fn from(error: sqlx::Error) -> Self {
        DecideError::Db(Error::Sqlx(error))
    }
}

/// Gives undecided transaction `id` the category: `categorized_by = 'user'`, the reason "Set by
/// <display name>".
///
/// # Errors
/// [`DecideError::NoSuchCategory`], or [`DecideError::NotUndecided`] if the row isn't one of the
/// undecided ones; either leaves everything as it was.
pub async fn decide_row(
    pool: &SqlitePool,
    id: i64,
    decision: &Decision<'_>,
) -> Result<(), DecideError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    check_category(&mut tx, decision.category_id).await?;
    let (reason, at) = (reason(decision), stamp(decision.at));
    let changed = sqlx::query!(
        r#"UPDATE transactions
           SET category_id = ?, categorized_by = 'user', categorization_reason = ?,
               updated_at = ?, updated_by = ?
           WHERE id = ? AND category_id IS NULL
             AND id NOT IN (SELECT transaction_id FROM transaction_splits)
             AND id IN (SELECT ib.transaction_id FROM import_batch_rows ib
                          JOIN import_batches b ON b.id = ib.batch_id
                        WHERE b.undone_at IS NULL AND ib.source_disposition = 'undecided')"#,
        decision.category_id,
        reason,
        at,
        decision.user_id,
        id
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Err(DecideError::NotUndecided);
    }
    tx.commit().await?;
    Ok(())
}

/// Gives every undecided row that came in under the Actual category `actual_category` the
/// category, all in one transaction. Returns how many rows moved.
///
/// # Errors
/// [`DecideError::NoSuchCategory`], or [`DecideError::NotUndecided`] if no row is waiting under
/// that Actual category; either leaves everything as it was.
pub async fn decide_group(
    pool: &SqlitePool,
    actual_category: &str,
    decision: &Decision<'_>,
) -> Result<u64, DecideError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    check_category(&mut tx, decision.category_id).await?;
    let (reason, at) = (reason(decision), stamp(decision.at));
    let changed = sqlx::query!(
        r#"UPDATE transactions
           SET category_id = ?, categorized_by = 'user', categorization_reason = ?,
               updated_at = ?, updated_by = ?
           WHERE category_id IS NULL
             AND id NOT IN (SELECT transaction_id FROM transaction_splits)
             AND id IN (SELECT ib.transaction_id FROM import_batch_rows ib
                          JOIN import_batches b ON b.id = ib.batch_id
                        WHERE b.undone_at IS NULL AND ib.source_disposition = 'undecided'
                          AND coalesce(ib.source_category, '') = ?)"#,
        decision.category_id,
        reason,
        at,
        decision.user_id,
        actual_category
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Err(DecideError::NotUndecided);
    }
    tx.commit().await?;
    Ok(changed)
}

fn reason(decision: &Decision<'_>) -> String {
    format!("Set by {}", decision.display_name)
}

async fn check_category(
    tx: &mut sqlx::SqliteConnection,
    category_id: i64,
) -> Result<(), DecideError> {
    let found = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM categories WHERE id = ?"#,
        category_id
    )
    .fetch_optional(tx)
    .await?;
    found.map(|_| ()).ok_or(DecideError::NoSuchCategory)
}
