//! Transactions (docs/spec/m1-ledger-import.md#transactions): read a page at a time, newest first,
//! searched and filtered, and one at a time in full. Read-only in milestone 1, where the Actual importer writes them.

use domain::transactions::{CategoryFilter, Cursor, Search, Source, Status};
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

/// Which transactions the list shows: every one when it's the default. Each filter set narrows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Payee, raw description or memo containing the text (ignoring ASCII case), or the amount
    /// either way round.
    pub search: Option<Search>,
    pub account_id: Option<i64>,
    pub category: Option<CategoryFilter>,
    /// On or after this day.
    pub from: Option<Date>,
    /// On or before this day.
    pub to: Option<Date>,
    pub tag_id: Option<i64>,
    /// Only rows from an Actual import whose category was mapped to "Decide later" and that still
    /// have none: the **From Actual, undecided** rows.
    pub undecided: bool,
}

/// Up to `limit` of the transactions `filter` lets through, newest first by date then id, starting
/// after `after` (or from the newest).
///
/// # Errors
/// If the database fails, or holds a date or a status it can't read.
pub async fn transactions(
    pool: &SqlitePool,
    filter: &Filter,
    after: Option<Cursor>,
    limit: u32,
) -> Result<TransactionPage, Error> {
    // One more than asked, to know whether there's another page.
    let mut transactions = rows(pool, None, filter, after, i64::from(limit) + 1).await?;
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
    let Some(row) = rows(pool, Some(id), &Filter::default(), None, 1)
        .await?
        .into_iter()
        .next()
    else {
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

/// Up to `limit` rows newest first that `filter` lets through, only transaction `only` if given,
/// after `after` if given.
///
/// The sets the filters look in (payees, tags, split lines, undecided import rows) are read once
/// each with `IN (SELECT …)`, not per row, so a filter costs one pass over the transactions.
async fn rows(
    pool: &SqlitePool,
    only: Option<i64>,
    filter: &Filter,
    after: Option<Cursor>,
    limit: i64,
) -> Result<Vec<TransactionRow>, Error> {
    let after_date = after.map(|cursor| cursor.date.to_string());
    let after_id = after.map(|cursor| cursor.id);
    let pattern = filter
        .search
        .as_ref()
        .map(|search| like_pattern(&search.text));
    let amount = filter
        .search
        .as_ref()
        .and_then(|search| search.amount_minor);
    let category_id = match filter.category {
        Some(CategoryFilter::Category(id)) => Some(id),
        _ => None,
    };
    let uncategorized = filter.category == Some(CategoryFilter::Uncategorized);
    let from = filter.from.map(|date| date.to_string());
    let to = filter.to.map(|date| date.to_string());
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
             AND (?5 IS NULL
               OR t.raw_description LIKE ?5 ESCAPE '\'
               OR t.memo LIKE ?5 ESCAPE '\'
               OR t.payee_id IN (SELECT id FROM payees WHERE name LIKE ?5 ESCAPE '\')
               OR abs(t.amount_minor) = ?6)
             AND (?7 IS NULL OR t.account_id = ?7)
             AND (?8 IS NULL
               OR t.category_id = ?8
               OR t.id IN (SELECT transaction_id FROM transaction_splits WHERE category_id = ?8))
             AND (NOT ?9 OR (t.category_id IS NULL
               AND t.id NOT IN (SELECT transaction_id FROM transaction_splits)
               AND t.id NOT IN (SELECT from_txn_id FROM transfers WHERE from_txn_id IS NOT NULL
                                UNION ALL
                                SELECT to_txn_id FROM transfers WHERE to_txn_id IS NOT NULL)))
             AND (?10 IS NULL OR t.date >= ?10)
             AND (?11 IS NULL OR t.date <= ?11)
             AND (?12 IS NULL
               OR t.id IN (SELECT transaction_id FROM transaction_tags WHERE tag_id = ?12))
             AND (NOT ?13 OR (t.category_id IS NULL
               AND t.id NOT IN (SELECT transaction_id FROM transaction_splits)
               AND t.id IN (SELECT ib.transaction_id FROM import_batch_rows ib
                              JOIN import_batches b ON b.id = ib.batch_id
                            WHERE b.undone_at IS NULL AND ib.source_disposition = 'undecided')))
           ORDER BY t.date DESC, t.id DESC
           LIMIT ?4"#,
        only,
        after_date,
        after_id,
        limit,
        pattern,
        amount,
        filter.account_id,
        category_id,
        uncategorized,
        from,
        to,
        filter.tag_id,
        filter.undecided
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

/// A `LIKE … ESCAPE '\'` pattern for text containing `text`: its `%`, `_` and `\` match only
/// themselves.
fn like_pattern(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() + 2);
    pattern.push('%');
    for character in text.chars() {
        if matches!(character, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
}

/// A tag the household has, for the Transactions tag filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: i64,
    pub name: String,
}

/// Every tag, by name (ignoring case).
///
/// # Errors
/// If the database fails.
pub async fn tags(pool: &SqlitePool) -> Result<Vec<Tag>, Error> {
    Ok(sqlx::query_as!(
        Tag,
        r#"SELECT id AS "id!: i64", name FROM tags ORDER BY name COLLATE NOCASE, id"#
    )
    .fetch_all(pool)
    .await?)
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

    /// Two accounts and five transactions, two of them on one day, one split and one tagged, and
    /// one (5) an undecided Actual row.
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
               INSERT INTO import_batch_rows (batch_id, transaction_id, outcome, import_ref,
                   source_category, source_disposition) VALUES
                 (7, 1, 'inserted', 'x1', 'Food', 'mapped'), (7, 4, 'inserted', 'x4', 'Cafe', 'mapped'),
                 (7, 5, 'inserted', 'x5', 'Bank fees', 'undecided');"#,
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
            let page = transactions(&pool, &Filter::default(), after, 2)
                .await
                .unwrap();
            assert!(page.transactions.len() <= 2);
            seen.extend(page.transactions.iter().map(|row| row.id));
            match page.next {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        // By date, then id, newest first.
        assert_eq!(seen, [3, 2, 4, 1, 5]);

        let all = transactions(&pool, &Filter::default(), None, 10)
            .await
            .unwrap();
        assert_eq!(all.next, None);
        assert_eq!(all.transactions.len(), 5);
        let exact = transactions(&pool, &Filter::default(), None, 5)
            .await
            .unwrap();
        assert_eq!(exact.next, None);
    }

    #[tokio::test]
    async fn rows_carry_what_the_list_shows() {
        let (_dir, pool) = ledger().await;
        let page = transactions(&pool, &Filter::default(), None, 10)
            .await
            .unwrap();
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

    /// The ids of the first page `filter` lets through, newest first.
    async fn ids(pool: &SqlitePool, filter: Filter) -> Vec<i64> {
        let page = transactions(pool, &filter, None, 200).await.unwrap();
        page.transactions.iter().map(|row| row.id).collect()
    }

    fn search(text: &str) -> Option<Search> {
        Search::new(text, "USD".parse().unwrap())
    }

    fn day(text: &str) -> Option<Date> {
        text.parse().ok()
    }

    #[tokio::test]
    async fn search_looks_in_payee_raw_description_memo_and_amount() {
        let (_dir, pool) = ledger().await;
        let find = |text: &str| Filter {
            search: search(text),
            ..Filter::default()
        };
        // Payee (1 and 3), then the raw description alone.
        assert_eq!(ids(&pool, find("corner market")).await, [3, 1]);
        assert_eq!(ids(&pool, find("mkt 02")).await, [3]);
        assert_eq!(ids(&pool, find("CAFE")).await, [4]);
        assert_eq!(ids(&pool, find("returned")).await, [2]);
        // Amounts, either way round, whole or decimal.
        assert_eq!(ids(&pool, find("12.50")).await, [1]);
        assert_eq!(ids(&pool, find("-12.5")).await, [1]);
        assert_eq!(ids(&pool, find("40")).await, [2]);
        assert_eq!(ids(&pool, find("9.00")).await, [4]);
        // "1" is 1.00 (5) and in "CORNER MKT 01" (1).
        assert_eq!(ids(&pool, find("1")).await, [1, 5]);
        // LIKE's wildcards match only themselves.
        assert_eq!(ids(&pool, find("_")).await, Vec::<i64>::new());
        assert_eq!(ids(&pool, find("%")).await, Vec::<i64>::new());
        assert_eq!(ids(&pool, find("nothing like it")).await, Vec::<i64>::new());
    }

    #[tokio::test]
    async fn each_filter_narrows_the_list() {
        let (_dir, pool) = ledger().await;
        let account = Filter {
            account_id: Some(2),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, account).await, [3, 4]);
        // A split row counts as in each of its lines' categories.
        let groceries = Filter {
            category: Some(CategoryFilter::Category(1)),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, groceries).await, [3, 1]);
        let dining = Filter {
            category: Some(CategoryFilter::Category(2)),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, dining).await, [3, 4]);
        let uncategorized = Filter {
            category: Some(CategoryFilter::Uncategorized),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, uncategorized).await, [2, 5]);
        let dates = Filter {
            from: day("2026-01-02"),
            to: day("2026-01-03"),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, dates).await, [4, 1]);
        let since = Filter {
            from: day("2026-01-05"),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, since).await, [3, 2]);
        let until = Filter {
            to: day("2026-01-01"),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, until).await, [5]);
        let tag = Filter {
            tag_id: Some(1),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, tag).await, [3]);
        let undecided = Filter {
            undecided: true,
            ..Filter::default()
        };
        assert_eq!(ids(&pool, undecided).await, [5]);
    }

    #[tokio::test]
    async fn filters_and_search_combine() {
        let (_dir, pool) = ledger().await;
        let corner_checking = Filter {
            search: search("corner"),
            account_id: Some(1),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, corner_checking).await, [1]);
        let uncategorized_since = Filter {
            category: Some(CategoryFilter::Uncategorized),
            from: day("2026-01-02"),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, uncategorized_since).await, [2]);
        let everything = Filter {
            search: search("30"),
            account_id: Some(2),
            category: Some(CategoryFilter::Category(2)),
            from: day("2026-01-04"),
            to: day("2026-01-05"),
            tag_id: Some(2),
            undecided: false,
        };
        assert_eq!(ids(&pool, everything).await, [3]);
        let none = Filter {
            account_id: Some(2),
            undecided: true,
            ..Filter::default()
        };
        assert_eq!(ids(&pool, none).await, Vec::<i64>::new());
        // Filtered pages run on with no gaps.
        let checking = Filter {
            account_id: Some(1),
            ..Filter::default()
        };
        let first = transactions(&pool, &checking, None, 2).await.unwrap();
        let rest = transactions(&pool, &checking, first.next, 2).await.unwrap();
        let seen: Vec<i64> = first
            .transactions
            .iter()
            .chain(&rest.transactions)
            .map(|row| row.id)
            .collect();
        assert_eq!(seen, [2, 1, 5]);
        assert_eq!(rest.next, None);
    }

    #[tokio::test]
    async fn undecided_rows_leave_once_categorized_or_undone() {
        let (_dir, pool) = ledger().await;
        let undecided = || Filter {
            undecided: true,
            ..Filter::default()
        };
        sqlx::query("UPDATE transactions SET category_id = 1 WHERE id = 5")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(ids(&pool, undecided()).await, Vec::<i64>::new());
        sqlx::query(
            "UPDATE transactions SET category_id = NULL WHERE id = 5;
             UPDATE import_batches SET undone_at = '2026-02-02T10:00:00.000Z' WHERE id = 7;",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(ids(&pool, undecided()).await, Vec::<i64>::new());
    }

    #[tokio::test]
    async fn transfer_legs_arent_uncategorized() {
        let (_dir, pool) = ledger().await;
        sqlx::query(
            "INSERT INTO transfers (from_account_id, to_account_id, from_txn_id, to_txn_id, matched_by)
               VALUES (1, 2, 2, NULL, 'import')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let uncategorized = Filter {
            category: Some(CategoryFilter::Uncategorized),
            ..Filter::default()
        };
        assert_eq!(ids(&pool, uncategorized).await, [5]);
    }

    #[tokio::test]
    async fn tags_list_by_name() {
        let (_dir, pool) = ledger().await;
        assert_eq!(
            tags(&pool).await.unwrap(),
            [
                Tag {
                    id: 2,
                    name: "gift \"big\"".to_owned()
                },
                Tag {
                    id: 1,
                    name: "trip".to_owned()
                },
            ]
        );
    }

    /// A generated ledger of 50,000 rows over four accounts and five years: 2,000 payees, 40
    /// categories, a seventh of the rows uncategorized, some undecided, every 13th tagged.
    async fn fifty_thousand_rows() -> (tempfile::TempDir, SqlitePool) {
        let (dir, pool) = migrated().await;
        sqlx::query(
            r"INSERT INTO accounts (id, name, type, role) VALUES
                 (1, 'Checking', 'checking', 'budgeted'), (2, 'Card', 'credit_card', 'budgeted'),
                 (3, 'Savings', 'savings', 'tracking'), (4, 'Cash', 'cash', 'budgeted');
               INSERT INTO category_groups (id, name) VALUES (1, 'Everything');
               WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 40)
                 INSERT INTO categories (id, group_id, name) SELECT i, 1, 'Category ' || i FROM n;
               WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2000)
                 INSERT INTO payees (id, name) SELECT i, 'Shop number ' || i FROM n;
               INSERT INTO tags (id, name) VALUES (1, 'trip'), (2, 'gift');
               INSERT INTO import_batches (id, format, file_name, added, matched, skipped)
                 VALUES (1, 'actual', 'export.zip', 50000, 0, 0);
               WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50000)
                 INSERT INTO transactions (id, account_id, date, amount_minor, status, payee_id,
                     raw_description, memo, category_id, source, dedupe_key)
                 SELECT i, 1 + i % 4, date('2021-01-01', '+' || (i / 30) || ' days'),
                        -(i * 37 % 100000), 'posted', CASE WHEN i % 5 = 0 THEN NULL ELSE 1 + i % 2000 END,
                        'POS PURCHASE ' || (i * 7919 % 100000) || ' TERMINAL ' || (i % 977),
                        CASE WHEN i % 11 = 0 THEN 'note ' || i END,
                        CASE WHEN i % 7 = 0 THEN NULL ELSE 1 + i % 40 END, 'import', 'k' || i
                 FROM n;
               INSERT INTO transaction_tags (transaction_id, tag_id)
                 SELECT id, 1 + id % 2 FROM transactions WHERE id % 13 = 0;
               INSERT INTO import_batch_rows (batch_id, transaction_id, outcome, import_ref,
                   source_category, source_disposition)
                 SELECT 1, id, 'inserted', 'r' || id, 'Old',
                        CASE WHEN category_id IS NULL AND id % 3 = 0 THEN 'undecided' ELSE 'mapped' END
                 FROM transactions;",
        )
        .execute(&pool)
        .await
        .unwrap();
        (dir, pool)
    }

    /// The 50k-row timing check (issue #17): on a generated ledger of 50,000 rows, the first page
    /// of every filter, and of searches that match much, little and nothing, comes back in under
    /// 200 ms *(agent)*, even in a debug build.
    #[tokio::test]
    async fn fifty_thousand_rows_search_and_filter_in_under_200_ms() {
        let (_dir, pool) = fifty_thousand_rows().await;
        let checks = [
            ("everything", Filter::default()),
            (
                "payee text",
                Filter {
                    search: search("shop number 1234"),
                    ..Filter::default()
                },
            ),
            (
                "common text",
                Filter {
                    search: search("terminal"),
                    ..Filter::default()
                },
            ),
            (
                "amount",
                Filter {
                    search: search("12.50"),
                    ..Filter::default()
                },
            ),
            (
                "no match",
                Filter {
                    search: search("nothing like it"),
                    ..Filter::default()
                },
            ),
            (
                "account and dates",
                Filter {
                    account_id: Some(2),
                    from: day("2023-01-01"),
                    to: day("2023-06-30"),
                    ..Filter::default()
                },
            ),
            (
                "category",
                Filter {
                    category: Some(CategoryFilter::Category(9)),
                    ..Filter::default()
                },
            ),
            (
                "uncategorized",
                Filter {
                    category: Some(CategoryFilter::Uncategorized),
                    ..Filter::default()
                },
            ),
            (
                "tag",
                Filter {
                    tag_id: Some(2),
                    ..Filter::default()
                },
            ),
            (
                "undecided",
                Filter {
                    undecided: true,
                    ..Filter::default()
                },
            ),
            (
                "all of them",
                Filter {
                    search: search("shop"),
                    account_id: Some(1),
                    category: Some(CategoryFilter::Uncategorized),
                    from: day("2021-06-01"),
                    to: day("2024-12-31"),
                    tag_id: Some(1),
                    undecided: true,
                },
            ),
        ];
        for (name, filter) in checks {
            let started = std::time::Instant::now();
            let page = transactions(&pool, &filter, None, 50).await.unwrap();
            let took = started.elapsed();
            eprintln!("{name}: {} rows in {took:?}", page.transactions.len());
            assert!(
                took < std::time::Duration::from_millis(200),
                "{name} took {took:?}"
            );
        }
    }

    #[test]
    fn like_patterns_escape_the_wildcards() {
        assert_eq!(like_pattern("corner"), "%corner%");
        assert_eq!(like_pattern(r"50%_\x"), r"%50\%\_\\x%");
    }

    #[test]
    fn tags_that_arent_a_json_list_are_unreadable() {
        assert_eq!(read_tags("[]").unwrap(), Vec::<String>::new());
        assert!(read_tags("trip").is_err());
    }
}
