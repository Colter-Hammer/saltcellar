use domain::csv_import::{FileRow, Outcome};

use super::*;
use crate::tests::set_up;
use crate::undo_import;

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

fn file_row(n: usize, day: &str, minor: i64, text: &str, category: Option<&str>) -> FileRow {
    FileRow {
        row: n,
        date: date(day),
        post_date: None,
        description: text.to_owned(),
        amount: Money::from_minor(minor),
        category: category.map(str::to_owned),
        memo: None,
        import_ref: format!("ref-{n}-{day}-{minor}"),
    }
}

async fn account(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO accounts (name, type, role, opening_balance_minor, opening_date)
         VALUES ('Demo Checking', 'checking', 'budgeted', 100000, '2026-06-01') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn stored(pool: &SqlitePool, account: i64, day: &str, minor: i64, source: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO transactions (account_id, date, amount_minor, raw_description, source, dedupe_key)
         VALUES (?, ?, ?, 'DEMO CAFE', ?, 'k') RETURNING id",
    )
    .bind(account)
    .bind(day)
    .bind(minor)
    .bind(source)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn commit<'a>(
    account_id: i64,
    rows: Vec<FileRow>,
    keep: &'a BTreeSet<usize>,
    today: &str,
) -> CsvCommit<'a> {
    CsvCommit {
        account_id,
        file_name: "demo.csv",
        header_signature: "sig",
        config: "{}",
        rows,
        keep,
        unreadable: 1,
        imported_by: 1,
        today: date(today),
    }
}

fn rows() -> Vec<FileRow> {
    vec![
        file_row(2, "2026-10-20", -575, "Demo Cafe", Some("Dining")),
        file_row(3, "2026-10-21", -1000, "Demo Books", Some("Shopping")),
        file_row(4, "2026-03-01", -500, "Old row", None),
    ]
}

#[tokio::test]
async fn an_import_adds_matches_and_undoes() {
    let (_dir, pool) = set_up().await;
    let account = account(&pool).await;
    assert!(import_account_exists(&pool, account).await.unwrap());
    assert!(!import_account_exists(&pool, account + 1).await.unwrap());
    let done = import_demo(&pool, account).await;
    again_adds_nothing(&pool, account).await;
    adopted_rows_stay_on_undo(&pool, account, done.batch_id).await;
}

async fn import_demo(pool: &SqlitePool, account: i64) -> CsvCommitted {
    let synced = stored(pool, account, "2026-10-19", -575, "provider").await;
    let keep = BTreeSet::new();

    let plan = plan_csv_import(pool, account, rows()).await.unwrap();
    assert_eq!(plan[0].outcome, Outcome::Matched(synced));
    assert_eq!(plan[1].outcome, Outcome::New);

    let done = commit_csv_import(pool, commit(account, rows(), &keep, "2026-10-28"))
        .await
        .unwrap();
    assert_eq!(
        (done.added, done.matched, done.skipped),
        (2, 1, 1),
        "two new, one matched, the unreadable one skipped"
    );

    // The matched row only gained the bank category; the new ones are import rows, the old one quiet.
    let matched: (Option<String>, String, i64) =
        sqlx::query_as("SELECT bank_category, source, amount_minor FROM transactions WHERE id = ?")
            .bind(synced)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        matched,
        (Some("Dining".to_owned()), "provider".to_owned(), -575)
    );
    let added: Vec<(String, Option<String>, String, bool, String)> = sqlx::query_as(
        "SELECT date, bank_category, source, review_quiet, status FROM transactions
         WHERE source = 'import' ORDER BY date",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(
        added,
        [
            (
                "2026-03-01".to_owned(),
                None,
                "import".to_owned(),
                true,
                "posted".to_owned()
            ),
            (
                "2026-10-21".to_owned(),
                Some("Shopping".to_owned()),
                "import".to_owned(),
                false,
                "posted".to_owned()
            ),
        ]
    );
    // The old row sits before the opening date: the opening moved back over it.
    let opening: (i64, String) =
        sqlx::query_as("SELECT opening_balance_minor, opening_date FROM accounts WHERE id = ?")
            .bind(account)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(opening, (100_500, "2026-02-28".to_owned()));
    done
}

/// The same file again adds nothing.
async fn again_adds_nothing(pool: &SqlitePool, account: i64) {
    let keep = BTreeSet::new();
    let again = plan_csv_import(pool, account, rows()).await.unwrap();
    assert!(again.iter().all(|entry| entry.outcome == Outcome::Seen));
    let second = commit_csv_import(pool, commit(account, rows(), &keep, "2026-10-28"))
        .await
        .unwrap();
    assert_eq!((second.added, second.matched), (0, 3));
    assert_eq!(import_mappings(pool, account).await.unwrap().len(), 1);
}

async fn adopted_rows_stay_on_undo(pool: &SqlitePool, account: i64, batch_id: i64) {
    // Sync adopts the imported 10-21 row; undoing the first batch keeps it, removes the other
    // inserted row and clears the bank category it set on the synced one.
    sqlx::query(
        "UPDATE transactions SET source = 'provider', updated_by = 1 WHERE date = '2026-10-21'",
    )
    .execute(pool)
    .await
    .unwrap();
    let undone = undo_import(
        pool,
        batch_id,
        false,
        "2026-10-29T09:00:00Z".parse().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(undone.rows_deleted, 1);
    let left: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT date, bank_category FROM transactions ORDER BY date")
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(
        left,
        [
            ("2026-10-19".to_owned(), None),
            // Adopted: stays, and the batch's bank category on it goes.
            ("2026-10-21".to_owned(), None),
        ]
    );
    let opening: (i64, String) =
        sqlx::query_as("SELECT opening_balance_minor, opening_date FROM accounts WHERE id = ?")
            .bind(account)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(opening, (100_000, "2026-06-01".to_owned()));
}

#[tokio::test]
async fn rows_to_check_are_added_only_when_kept_and_a_mapping_is_replaced() {
    let (_dir, pool) = set_up().await;
    let account = account(&pool).await;
    stored(&pool, account, "2026-10-20", -575, "manual").await;
    sqlx::query(
        "INSERT INTO transactions (account_id, date, amount_minor, raw_description, source, dedupe_key)
         VALUES (?, '2026-10-20', -575, 'OTHER', 'manual', 'k2')",
    )
    .bind(account)
    .execute(&pool)
    .await
    .unwrap();
    let rows = vec![file_row(2, "2026-10-20", -575, "Unlike both", None)];
    let plan = plan_csv_import(&pool, account, rows.clone()).await.unwrap();
    assert!(matches!(plan[0].outcome, Outcome::Check(_)));

    let none = BTreeSet::new();
    let skipped = commit_csv_import(&pool, commit(account, rows.clone(), &none, "2026-10-28"))
        .await
        .unwrap();
    assert_eq!((skipped.added, skipped.skipped), (0, 2));
    let kept = BTreeSet::from([2]);
    let mut again = commit(account, rows, &kept, "2026-10-28");
    again.config = "{\"changed\":true}";
    let added = commit_csv_import(&pool, again).await.unwrap();
    assert_eq!((added.added, added.skipped), (1, 1));

    let saved = import_mappings(&pool, account).await.unwrap();
    assert_eq!(
        saved,
        [SavedMapping {
            header_signature: "sig".to_owned(),
            config: "{\"changed\":true}".to_owned()
        }]
    );
    let found = csv_matched_rows(&pool, &[1, 2, 99]).await.unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].raw_description, "DEMO CAFE");
    // No rows at all plans nothing.
    assert_eq!(
        plan_csv_import(&pool, account, Vec::new())
            .await
            .unwrap()
            .len(),
        0
    );
}
