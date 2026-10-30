//! Transactions (docs/spec/m1-ledger-import.md#transactions): read a page at a time, newest first,
//! and one at a time in full. Read-only in milestone 1, where the Actual importer writes them.

use domain::transactions::{Cursor, Source, Status};
use domain::{Date, Money};

use crate::{Error, SqlitePool, parse_stamp};

/// A transaction as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionRow {
    pub id: i64,
    pub date: Date,
    pub amount: Money,
    pub status: Status,
    pub payee: Option<String>,
    /// Exactly what the bank sent.
    pub raw_description: String,
    pub account_id: i64,
    pub account: String,
    /// `None` when it's split, a transfer leg, or uncategorized.
    pub category_id: Option<i64>,
    pub category: Option<String>,
    /// Whether its amount is split across categories, in its split lines.
    pub split: bool,
    /// Its tags' names, in name order.
    pub tags: Vec<String>,
}

/// One page of transactions, and where the next one starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionPage {
    pub transactions: Vec<TransactionRow>,
    /// `None` on the last page.
    pub next: Option<Cursor>,
}

/// Up to `limit` transactions, newest first by date then id, starting after `after` (or from the
/// newest).
///
/// # Errors
/// If the database fails, or holds a date or a status it can't read.
pub async fn transactions(
    pool: &SqlitePool,
    after: Option<Cursor>,
    limit: u32,
) -> Result<TransactionPage, Error> {
    // One more than asked, to know whether there's another page.
    let mut transactions = rows(pool, None, after, i64::from(limit) + 1).await?;
    let next = if transactions.len() > limit as usize {
        transactions.truncate(limit as usize);
        transactions.last().map(|last| Cursor {
            date: last.date,
            id: last.id,
        })
    } else {
        None
    };
    Ok(TransactionPage { transactions, next })
}

/// One of a split transaction's lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitLine {
    pub category_id: i64,
    pub category: String,
    pub amount: Money,
    pub memo: Option<String>,
}

/// The import a transaction came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRef {
    pub id: i64,
    pub file_name: String,
    pub imported_at: jiff::Timestamp,
}

/// A transaction in full, as its detail shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionDetail {
    pub row: TransactionRow,
    pub memo: Option<String>,
    /// Empty unless it's split.
    pub splits: Vec<SplitLine>,
    pub source: Source,
    /// How it got its category, in words.
    pub categorization_reason: Option<String>,
    /// The first import it came in with; `None` if it came from none.
    pub batch: Option<BatchRef>,
}

/// Transaction `id` in full; `None` if there's no such transaction.
///
/// # Errors
/// As [`transactions`], or if it holds a source or a timestamp it can't read.
pub async fn transaction(pool: &SqlitePool, id: i64) -> Result<Option<TransactionDetail>, Error> {
    let Some(row) = rows(pool, Some(id), None, 1).await?.into_iter().next() else {
        return Ok(None);
    };
    let more = sqlx::query!(
        r#"SELECT memo, source, categorization_reason
           FROM transactions WHERE id = ?"#,
        id
    )
    .fetch_one(pool)
    .await?;
    let splits = sqlx::query!(
        r#"SELECT sp.category_id AS "category_id!: i64", c.name AS category, sp.amount_minor, sp.memo
           FROM transaction_splits sp JOIN categories c ON c.id = sp.category_id
           WHERE sp.transaction_id = ? ORDER BY sp.id"#,
        id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|line| SplitLine {
        category_id: line.category_id,
        category: line.category,
        amount: Money::from_minor(line.amount_minor),
        memo: line.memo,
    })
    .collect();
    let batch = sqlx::query!(
        r#"SELECT b.id AS "id!: i64", b.file_name, b.imported_at
           FROM import_batch_rows r JOIN import_batches b ON b.id = r.batch_id
           WHERE r.transaction_id = ? ORDER BY b.id LIMIT 1"#,
        id
    )
    .fetch_optional(pool)
    .await?
    .map(|batch| {
        Ok::<_, Error>(BatchRef {
            id: batch.id,
            file_name: batch.file_name,
            imported_at: parse_stamp(batch.imported_at)?,
        })
    })
    .transpose()?;
    Ok(Some(TransactionDetail {
        row,
        memo: more.memo,
        splits,
        source: Source::from_stored(&more.source).ok_or(Error::Unreadable {
            what: "a transaction source",
            value: more.source,
        })?,
        categorization_reason: more.categorization_reason,
        batch,
    }))
}

/// Up to `limit` rows newest first, only transaction `only` if given, after `after` if given.
async fn rows(
    pool: &SqlitePool,
    only: Option<i64>,
    after: Option<Cursor>,
    limit: i64,
) -> Result<Vec<TransactionRow>, Error> {
    let after_date = after.map(|cursor| cursor.date.to_string());
    let after_id = after.map(|cursor| cursor.id);
    let found = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.date, t.amount_minor, t.status, p.name AS "payee?",
             t.raw_description, t.account_id, a.name AS account, t.category_id, c.name AS "category?",
             EXISTS (SELECT 1 FROM transaction_splits sp WHERE sp.transaction_id = t.id)
               AS "split!: bool",
             (SELECT json_group_array(name) FROM
               (SELECT g.name FROM transaction_tags tt JOIN tags g ON g.id = tt.tag_id
                 WHERE tt.transaction_id = t.id ORDER BY g.name)) AS "tags!: String"
           FROM transactions t
           JOIN accounts a ON a.id = t.account_id
           LEFT JOIN payees p ON p.id = t.payee_id
           LEFT JOIN categories c ON c.id = t.category_id
           WHERE (?1 IS NULL OR t.id = ?1)
             AND (?2 IS NULL OR t.date < ?2 OR (t.date = ?2 AND t.id < ?3))
           ORDER BY t.date DESC, t.id DESC
           LIMIT ?4"#,
        only,
        after_date,
        after_id,
        limit
    )
    .fetch_all(pool)
    .await?;
    found
        .into_iter()
        .map(|row| {
            Ok(TransactionRow {
                id: row.id,
                date: row.date.parse().map_err(|_| Error::Unreadable {
                    what: "a date",
                    value: row.date.clone(),
                })?,
                amount: Money::from_minor(row.amount_minor),
                status: Status::from_stored(&row.status).ok_or(Error::Unreadable {
                    what: "a transaction status",
                    value: row.status.clone(),
                })?,
                payee: row.payee,
                raw_description: row.raw_description,
                account_id: row.account_id,
                account: row.account,
                category_id: row.category_id,
                category: row.category,
                split: row.split,
                tags: read_tags(&row.tags)?,
            })
        })
        .collect()
}

/// The tag names SQLite's `json_group_array` wrote: a JSON array of strings.
fn read_tags(json: &str) -> Result<Vec<String>, Error> {
    serde_json::from_str(json).map_err(|_| Error::Unreadable {
        what: "a transaction's tags",
        value: json.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::migrated;

    /// Two accounts and five transactions, two of them on one day, one split and one tagged.
    async fn ledger() -> (tempfile::TempDir, SqlitePool) {
        let (dir, pool) = migrated().await;
        sqlx::query(
            r#"INSERT INTO accounts (id, name, type, role) VALUES
                 (1, 'Checking', 'checking', 'budgeted'), (2, 'Card', 'credit_card', 'budgeted');
               INSERT INTO category_groups (id, name) VALUES (1, 'Food');
               INSERT INTO categories (id, group_id, name) VALUES (1, 1, 'Groceries'), (2, 1, 'Dining out');
               INSERT INTO payees (id, name) VALUES (1, 'Corner Market');
               INSERT INTO tags (id, name) VALUES (1, 'trip'), (2, 'gift "big"');
               INSERT INTO transactions (id, account_id, date, amount_minor, status, payee_id,
                   raw_description, memo, category_id, categorization_reason, source, dedupe_key)
                 VALUES
                 (1, 1, '2026-01-02', -1250, 'posted', 1, 'CORNER MKT 01', NULL, 1, 'From Actual: Groceries', 'import', 'a'),
                 (2, 1, '2026-01-05', 4000, 'posted', NULL, 'REFUND', 'Returned', NULL, NULL, 'manual', 'b'),
                 (3, 2, '2026-01-05', -3000, 'pending', 1, 'CORNER MKT 02', NULL, NULL, NULL, 'provider', 'c'),
                 (4, 2, '2026-01-03', -900, 'posted', NULL, 'CAFE', NULL, 2, NULL, 'import', 'd'),
                 (5, 1, '2026-01-01', -100, 'posted', NULL, 'FEE', NULL, NULL, NULL, 'import', 'e');
               INSERT INTO transaction_splits (transaction_id, category_id, amount_minor, memo) VALUES
                 (3, 1, -2000, 'food'), (3, 2, -1000, NULL);
               INSERT INTO transaction_tags (transaction_id, tag_id) VALUES (3, 2), (3, 1);
               INSERT INTO import_batches (id, format, file_name, added, matched, skipped, imported_at)
                 VALUES (7, 'actual', 'export.zip', 3, 0, 0, '2026-02-01T10:00:00.000Z');
               INSERT INTO import_batch_rows (batch_id, transaction_id, outcome, import_ref) VALUES
                 (7, 1, 'inserted', 'x1'), (7, 4, 'inserted', 'x4'), (7, 5, 'inserted', 'x5');"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn pages_run_newest_first_with_no_gaps_or_repeats() {
        let (_dir, pool) = ledger().await;
        let mut seen = Vec::new();
        let mut after = None;
        loop {
            let page = transactions(&pool, after, 2).await.unwrap();
            assert!(page.transactions.len() <= 2);
            seen.extend(page.transactions.iter().map(|row| row.id));
            match page.next {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        // By date, then id, newest first.
        assert_eq!(seen, [3, 2, 4, 1, 5]);

        let all = transactions(&pool, None, 10).await.unwrap();
        assert_eq!(all.next, None);
        assert_eq!(all.transactions.len(), 5);
        let exact = transactions(&pool, None, 5).await.unwrap();
        assert_eq!(exact.next, None);
    }

    #[tokio::test]
    async fn rows_carry_what_the_list_shows() {
        let (_dir, pool) = ledger().await;
        let page = transactions(&pool, None, 10).await.unwrap();
        let split = &page.transactions[0];
        assert_eq!(
            split,
            &TransactionRow {
                id: 3,
                date: "2026-01-05".parse().unwrap(),
                amount: Money::from_minor(-3000),
                status: Status::Pending,
                payee: Some("Corner Market".to_owned()),
                raw_description: "CORNER MKT 02".to_owned(),
                account_id: 2,
                account: "Card".to_owned(),
                category_id: None,
                category: None,
                split: true,
                tags: vec!["gift \"big\"".to_owned(), "trip".to_owned()],
            }
        );
        let refund = &page.transactions[1];
        assert_eq!(refund.amount, Money::from_minor(4000));
        assert_eq!(refund.payee, None);
        assert_eq!(refund.tags, Vec::<String>::new());
        let cafe = &page.transactions[2];
        assert_eq!(cafe.category.as_deref(), Some("Dining out"));
        assert!(!cafe.split);
    }

    #[tokio::test]
    async fn one_transaction_reads_in_full() {
        let (_dir, pool) = ledger().await;
        let groceries = transaction(&pool, 1).await.unwrap().unwrap();
        assert_eq!(groceries.row.category.as_deref(), Some("Groceries"));
        assert_eq!(groceries.row.raw_description, "CORNER MKT 01");
        assert_eq!(groceries.source, Source::Import);
        assert_eq!(
            groceries.categorization_reason.as_deref(),
            Some("From Actual: Groceries")
        );
        assert_eq!(
            groceries.batch,
            Some(BatchRef {
                id: 7,
                file_name: "export.zip".to_owned(),
                imported_at: "2026-02-01T10:00:00Z".parse().unwrap(),
            })
        );
        assert_eq!(groceries.splits, []);

        let split = transaction(&pool, 3).await.unwrap().unwrap();
        assert_eq!(split.source, Source::Provider);
        assert_eq!(split.batch, None);
        assert_eq!(
            split.splits,
            [
                SplitLine {
                    category_id: 1,
                    category: "Groceries".to_owned(),
                    amount: Money::from_minor(-2000),
                    memo: Some("food".to_owned()),
                },
                SplitLine {
                    category_id: 2,
                    category: "Dining out".to_owned(),
                    amount: Money::from_minor(-1000),
                    memo: None,
                },
            ]
        );

        let refund = transaction(&pool, 2).await.unwrap().unwrap();
        assert_eq!(refund.memo.as_deref(), Some("Returned"));
        assert_eq!(refund.source, Source::Manual);

        assert_eq!(transaction(&pool, 99).await.unwrap(), None);
    }

    #[test]
    fn tags_that_arent_a_json_list_are_unreadable() {
        assert_eq!(read_tags("[]").unwrap(), Vec::<String>::new());
        assert!(read_tags("trip").is_err());
    }
}
