//! The household's answers to sync flags (docs/spec/m2-bank-data.md#sync-health-in-the-ui): each
//! flag on a row has its actions, and each action clears the flag.
//!
//! - `possible_duplicate`: **Merge** or **Keep both**.
//! - `possible_manual_match`: **Yes** (merge) or **No**.
//! - `not_seen_at_bank`: **Keep** here; **Delete** is the row's ordinary delete.
//! - `amount_changed_split`: fixing the split ([`set_splits`]).
//!
//! Merging *(agent)*: the flagged row, the newer one with the bank's id and amount, survives. It
//! takes the household's fields from the row it points at (category or split lines or transfer,
//! payee, memo, tags, exclude-from-budget), and that row is deleted; a bank id it had goes into
//! `deleted_bank_rows`, so sync never brings it back. Split lines that no longer add up flag the
//! survivor `amount_changed_split`. Another row flagged a possible duplicate of the deleted one
//! points at the oldest other row it could be instead (same account, amount and status, dated
//! within 3 days, a similar description, not itself a new possible duplicate), so two rows
//! re-issued together are resolved one by one; with none, it is unflagged.

use domain::Money;
use domain::matching::similar_description;
use domain::transactions::{SyncFlag, lines_add_up};
use jiff::Timestamp;

use crate::{Error, SqlitePool, stamp};

/// The household's answer to a row's sync flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// `possible_duplicate`: the same transaction; merge them.
    Merge,
    /// `possible_duplicate`: two transactions; keep both.
    KeepBoth,
    /// `possible_manual_match`: it is that manual entry; merge them, keeping the bank's amount.
    AcceptMatch,
    /// `possible_manual_match`: it isn't.
    RejectMatch,
    /// `not_seen_at_bank`: keep it, and stop asking.
    Keep,
}

impl Resolution {
    /// The flag this answers.
    fn answers(self) -> SyncFlag {
        match self {
            Resolution::Merge | Resolution::KeepBoth => SyncFlag::PossibleDuplicate,
            Resolution::AcceptMatch | Resolution::RejectMatch => SyncFlag::PossibleManualMatch,
            Resolution::Keep => SyncFlag::NotSeenAtBank,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("there's no such transaction")]
    NotFound,
    /// The row doesn't carry the flag this answers (any more).
    #[error("the transaction doesn't have that flag")]
    NotFlagged,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for ResolveError {
    fn from(error: sqlx::Error) -> Self {
        ResolveError::Db(Error::Sqlx(error))
    }
}

/// Answers row `id`'s sync flag with `resolution`, for login `user_id` at `now`, clearing it.
/// Resolves to the row to show after: `id` itself, which survives a merge.
///
/// # Errors
/// [`ResolveError::NotFound`], [`ResolveError::NotFlagged`] when its flag isn't the one
/// `resolution` answers, or if the database fails; nothing is written then.
pub async fn resolve_sync_flag(
    pool: &SqlitePool,
    id: i64,
    resolution: Resolution,
    user_id: i64,
    now: Timestamp,
) -> Result<i64, ResolveError> {
    let at = stamp(now);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let found = sqlx::query!(
        "SELECT sync_flag, possible_duplicate_of FROM transactions WHERE id = ?",
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ResolveError::NotFound)?;
    let flag = found.sync_flag.as_deref().and_then(SyncFlag::from_stored);
    if flag != Some(resolution.answers()) {
        return Err(ResolveError::NotFlagged);
    }
    match resolution {
        Resolution::KeepBoth | Resolution::RejectMatch => {
            sqlx::query!(
                "UPDATE transactions
                 SET sync_flag = NULL, possible_duplicate_of = NULL, updated_at = ?,
                     updated_by = ?
                 WHERE id = ?",
                at,
                user_id,
                id
            )
            .execute(&mut *tx)
            .await?;
        }
        Resolution::Keep => {
            sqlx::query!(
                "UPDATE transactions
                 SET sync_flag = NULL, kept_by_household = 1, updated_at = ?, updated_by = ?
                 WHERE id = ?",
                at,
                user_id,
                id
            )
            .execute(&mut *tx)
            .await?;
        }
        Resolution::Merge | Resolution::AcceptMatch => {
            let other = found
                .possible_duplicate_of
                .ok_or(ResolveError::NotFlagged)?;
            merge(&mut tx, id, other, user_id, &at).await?;
        }
    }
    tx.commit().await?;
    Ok(id)
}

/// Merges row `other` into row `survivor`: the survivor takes the household's fields from
/// `other`, which is deleted.
async fn merge(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    survivor: i64,
    other: i64,
    user_id: i64,
    at: &str,
) -> Result<(), Error> {
    let from = sqlx::query!(
        r#"SELECT account_id, external_id, payee_id, memo,
             exclude_from_budget AS "excluded!: bool"
           FROM transactions WHERE id = ?"#,
        other
    )
    .fetch_one(&mut **tx)
    .await?;
    take_category(tx, survivor, other).await?;
    sqlx::query!(
        "INSERT OR IGNORE INTO transaction_tags (transaction_id, tag_id)
         SELECT ?, tag_id FROM transaction_tags WHERE transaction_id = ?",
        survivor,
        other
    )
    .execute(&mut **tx)
    .await?;
    if let Some(external_id) = &from.external_id {
        sqlx::query!(
            "INSERT INTO deleted_bank_rows (account_id, external_id, deleted_by, deleted_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (account_id, external_id) DO NOTHING",
            from.account_id,
            external_id,
            user_id,
            at
        )
        .execute(&mut **tx)
        .await?;
    }
    repoint(tx, other, survivor).await?;
    crate::synced_rows::remove(tx, other).await?;
    sqlx::query!(
        "UPDATE transactions
         SET payee_id = coalesce(?1, payee_id), memo = coalesce(?2, memo),
             exclude_from_budget = max(exclude_from_budget, ?3),
             possible_duplicate_of = NULL,
             sync_flag = CASE
               WHEN EXISTS (SELECT 1 FROM transaction_splits s WHERE s.transaction_id = ?6)
                AND (SELECT sum(s.amount_minor) FROM transaction_splits s
                      WHERE s.transaction_id = ?6) <> amount_minor
               THEN 'amount_changed_split' END,
             updated_at = ?4, updated_by = ?5
         WHERE id = ?6",
        from.payee_id,
        from.memo,
        from.excluded,
        at,
        user_id,
        survivor
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Gives `survivor` what `other` was categorized as, when it was: its category, its split lines,
/// or the transfer it's a leg of (unless the survivor is a leg of one already).
async fn take_category(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    survivor: i64,
    other: i64,
) -> Result<(), Error> {
    let from = sqlx::query!(
        r#"SELECT category_id, categorized_by, categorization_reason,
             EXISTS (SELECT 1 FROM transaction_splits s WHERE s.transaction_id = t.id)
               AS "split!: bool",
             EXISTS (SELECT 1 FROM transfers tr WHERE tr.from_txn_id = t.id OR tr.to_txn_id = t.id)
               AS "transfer!: bool"
           FROM transactions t WHERE id = ?"#,
        other
    )
    .fetch_one(&mut **tx)
    .await?;
    let survivor_transfer = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM transfers WHERE from_txn_id = ? OR to_txn_id = ?)
             AS "transfer!: bool""#,
        survivor,
        survivor
    )
    .fetch_one(&mut **tx)
    .await?;
    let moves_transfer = from.transfer && !survivor_transfer;
    if from.category_id.is_some() || from.split || moves_transfer {
        sqlx::query!(
            "DELETE FROM transaction_splits WHERE transaction_id = ?",
            survivor
        )
        .execute(&mut **tx)
        .await?;
        sqlx::query!(
            "UPDATE transaction_splits SET transaction_id = ? WHERE transaction_id = ?",
            survivor,
            other
        )
        .execute(&mut **tx)
        .await?;
        if moves_transfer {
            sqlx::query!(
                "UPDATE transfers
                 SET from_txn_id = CASE WHEN from_txn_id = ?1 THEN ?2 ELSE from_txn_id END,
                     to_txn_id = CASE WHEN to_txn_id = ?1 THEN ?2 ELSE to_txn_id END
                 WHERE from_txn_id = ?1 OR to_txn_id = ?1",
                other,
                survivor
            )
            .execute(&mut **tx)
            .await?;
        }
        sqlx::query!(
            "UPDATE transactions
             SET category_id = ?, categorized_by = ?, categorization_reason = ?
             WHERE id = ?",
            from.category_id,
            from.categorized_by,
            from.categorization_reason,
            survivor
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Points each other row flagged a possible duplicate of `other`, which a merge into `survivor`
/// deletes, at the oldest other row it could be a duplicate of; those left pointing at `other`
/// are unflagged when it goes.
async fn repoint(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    other: i64,
    survivor: i64,
) -> Result<(), Error> {
    let flagged = sqlx::query!(
        r#"SELECT id AS "id!: i64", account_id, date, amount_minor, status, raw_description
           FROM transactions
           WHERE possible_duplicate_of = ? AND id <> ? AND sync_flag = 'possible_duplicate'"#,
        other,
        survivor
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in flagged {
        let candidates = sqlx::query!(
            r#"SELECT id AS "id!: i64", raw_description FROM transactions
               WHERE account_id = ? AND amount_minor = ? AND status = ?
                 AND date BETWEEN date(?, '-3 days') AND date(?, '+3 days')
                 AND id NOT IN (?, ?, ?)
                 AND sync_flag IS NOT 'possible_duplicate'
               ORDER BY date, id"#,
            row.account_id,
            row.amount_minor,
            row.status,
            row.date,
            row.date,
            row.id,
            other,
            survivor
        )
        .fetch_all(&mut **tx)
        .await?;
        if let Some(instead) = candidates
            .into_iter()
            .find(|found| similar_description(&found.raw_description, &row.raw_description))
        {
            sqlx::query!(
                "UPDATE transactions SET possible_duplicate_of = ? WHERE id = ?",
                instead.id,
                row.id
            )
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

/// One line of a split, as the household fixes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSplitLine {
    pub category_id: i64,
    pub amount: Money,
    pub memo: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SplitError {
    #[error("there's no such transaction")]
    NotFound,
    /// It has no split lines to fix.
    #[error("the transaction isn't split")]
    NotSplit,
    #[error("the lines don't add up to the amount")]
    DontAddUp,
    #[error("there's no such category")]
    NoSuchCategory,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for SplitError {
    fn from(error: sqlx::Error) -> Self {
        SplitError::Db(Error::Sqlx(error))
    }
}

/// Replaces split row `id`'s lines with `lines`, which add up to its amount, for login `user_id`
/// at `now`; an `amount_changed_split` flag goes. A single line makes it a plain row in that
/// line's category *(agent)*.
///
/// # Errors
/// [`SplitError::NotFound`], [`SplitError::NotSplit`], [`SplitError::DontAddUp`],
/// [`SplitError::NoSuchCategory`], or if the database fails; nothing is written then.
pub async fn set_splits(
    pool: &SqlitePool,
    id: i64,
    lines: &[NewSplitLine],
    user_id: i64,
    now: Timestamp,
) -> Result<(), SplitError> {
    let at = stamp(now);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let found = sqlx::query!(
        r#"SELECT amount_minor,
             EXISTS (SELECT 1 FROM transaction_splits s WHERE s.transaction_id = t.id)
               AS "split!: bool"
           FROM transactions t WHERE id = ?"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(SplitError::NotFound)?;
    if !found.split {
        return Err(SplitError::NotSplit);
    }
    if !lines_add_up(
        Money::from_minor(found.amount_minor),
        lines.iter().map(|line| line.amount),
    ) {
        return Err(SplitError::DontAddUp);
    }
    for line in lines {
        let known = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM categories WHERE id = ?) AS "known!: bool""#,
            line.category_id
        )
        .fetch_one(&mut *tx)
        .await?;
        if !known {
            return Err(SplitError::NoSuchCategory);
        }
    }
    sqlx::query!(
        "DELETE FROM transaction_splits WHERE transaction_id = ?",
        id
    )
    .execute(&mut *tx)
    .await?;
    let single = match lines {
        [only] => Some(only.category_id),
        _ => None,
    };
    if single.is_none() {
        for line in lines {
            let amount = line.amount.minor();
            sqlx::query!(
                "INSERT INTO transaction_splits (transaction_id, category_id, amount_minor, memo)
                 VALUES (?, ?, ?, ?)",
                id,
                line.category_id,
                amount,
                line.memo
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    sqlx::query!(
        "UPDATE transactions
         SET category_id = ?, categorized_by = 'user',
             sync_flag = CASE WHEN sync_flag = 'amount_changed_split' THEN NULL ELSE sync_flag END,
             updated_at = ?, updated_by = ?
         WHERE id = ?",
        single,
        at,
        user_id,
        id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
