use super::*;
use crate::tests::set_up;

const AT: &str = "2026-10-30T12:00:00Z";

/// An account, a category, and an Actual batch with one matched row whose `prior_fields` are the
/// category and memo it had before the batch filled them. Returns (batch, matched row, category).
async fn matched_row(pool: &SqlitePool) -> (i64, i64, i64) {
    sqlx::query("INSERT INTO category_groups (id, name) VALUES (1, 'Food')")
        .execute(pool)
        .await
        .unwrap();
    let before: i64 = sqlx::query_scalar(
        "INSERT INTO categories (group_id, name) VALUES (1, 'Before') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let after: i64 = sqlx::query_scalar(
        "INSERT INTO categories (group_id, name) VALUES (1, 'After') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO accounts (id, name, type, role) VALUES (1, 'Main', 'checking', 'budgeted')",
    )
    .execute(pool)
    .await
    .unwrap();
    // The row as the batch left it: its category and memo filled in.
    let txn: i64 = sqlx::query_scalar(
        "INSERT INTO transactions
           (account_id, date, amount_minor, raw_description, memo, category_id, categorized_by,
            source, dedupe_key)
         VALUES (1, '2026-10-01', -500, 'COFFEE', 'filled', ?, 'import', 'manual', 'k')
         RETURNING id",
    )
    .bind(after)
    .fetch_one(pool)
    .await
    .unwrap();
    let batch: i64 = sqlx::query_scalar(
        "INSERT INTO import_batches (format, file_name, added, matched, skipped)
         VALUES ('actual', 'x.zip', 0, 1, 0) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let prior = format!(
        r#"{{"category_id": null, "categorized_by": null, "memo": "kept", "payee_id": null, "before": {before}}}"#
    );
    sqlx::query(
        "INSERT INTO import_batch_rows (batch_id, transaction_id, outcome, import_ref, prior_fields)
         VALUES (?, ?, 'matched', 'a1', ?)",
    )
    .bind(batch)
    .bind(txn)
    .bind(prior)
    .execute(pool)
    .await
    .unwrap();
    (batch, txn, after)
}

#[tokio::test]
async fn undoing_restores_the_fields_a_batch_filled_on_a_matched_row() {
    let (_dir, pool) = set_up().await;
    let (batch, txn, _) = matched_row(&pool).await;
    let undone = undo_import(&pool, batch, false, AT.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(undone.rows_deleted, 0);
    assert_eq!(undone.accounts_deleted, 0);
    let (category, by, memo): (Option<i64>, Option<String>, Option<String>) =
        sqlx::query_as("SELECT category_id, categorized_by, memo FROM transactions WHERE id = ?")
            .bind(txn)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((category, by, memo.as_deref()), (None, None, Some("kept")));
    let undone_at: Option<String> =
        sqlx::query_scalar("SELECT undone_at FROM import_batches WHERE id = ?")
            .bind(batch)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(undone_at.as_deref(), Some("2026-10-30T12:00:00.000Z"));
}

#[tokio::test]
async fn an_undone_batch_cant_be_undone_again_and_a_missing_one_isnt_there() {
    let (_dir, pool) = set_up().await;
    let (batch, _, _) = matched_row(&pool).await;
    undo_import(&pool, batch, false, AT.parse().unwrap())
        .await
        .unwrap();
    let again = undo_import(&pool, batch, true, AT.parse().unwrap()).await;
    assert!(matches!(again, Err(UndoError::AlreadyUndone)));
    let missing = undo_import(&pool, batch + 1, true, AT.parse().unwrap()).await;
    assert!(matches!(missing, Err(UndoError::NotFound)));
}
