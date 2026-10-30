use crate::{Error, SqlitePool};

/// Whether an import from Actual is in place and not undone. Until milestone 2 makes re-importing
/// overlap-aware, a second one is refused (docs/spec/m1-ledger-import.md#step-1-file).
///
/// `import_batches` arrives with the importer's commit step; until then there can't be one, so
/// the table is looked for first, and the query on it is checked when it runs, not at build time.
///
/// # Errors
/// If the database fails.
pub async fn actual_import_in_place(pool: &SqlitePool) -> Result<bool, Error> {
    let has_batches = sqlx::query_scalar!(
        r#"SELECT EXISTS (
             SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'import_batches'
           ) AS "has!: bool""#
    )
    .fetch_one(pool)
    .await?;
    if !has_batches {
        return Ok(false);
    }
    let in_place: bool = sqlx::query_scalar(
        "SELECT EXISTS (
           SELECT 1 FROM import_batches WHERE format = 'actual' AND undone_at IS NULL
         )",
    )
    .fetch_one(pool)
    .await?;
    Ok(in_place)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::migrated;

    #[tokio::test]
    async fn only_an_actual_batch_not_undone_counts() {
        let (_dir, pool) = migrated().await;
        assert!(!actual_import_in_place(&pool).await.unwrap());

        // The columns this reads, as docs/spec/schema.sql has them.
        sqlx::query(
            "CREATE TABLE import_batches (id INTEGER PRIMARY KEY, format TEXT NOT NULL, undone_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(!actual_import_in_place(&pool).await.unwrap());
        sqlx::query("INSERT INTO import_batches (format) VALUES ('csv')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO import_batches (format, undone_at) VALUES ('actual', '2026-10-28T09:00:00.000Z')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(!actual_import_in_place(&pool).await.unwrap());
        sqlx::query("INSERT INTO import_batches (format) VALUES ('actual')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(actual_import_in_place(&pool).await.unwrap());
    }
}
