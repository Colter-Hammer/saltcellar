use domain::Money;
use domain::feed::SyncedRow;

use super::*;
use crate::tests::migrated;

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn date(text: &str) -> domain::Date {
    let d: jiff::civil::Date = text.parse().unwrap();
    domain::Date::from_ymd(d.year(), d.month(), d.day()).unwrap()
}

/// The daily window's first day, for a run on 2026-10-28.
fn start() -> Date {
    date("2026-09-28")
}

fn row(id: &str, day: &str, minor: i64, pending: bool, text: &str) -> SyncedRow {
    SyncedRow {
        external_id: id.to_owned(),
        date: date(day),
        amount: Money::from_minor(minor),
        pending,
        raw_description: text.to_owned(),
        payee_text: "Demo Cafe".to_owned(),
    }
}

async fn account(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO accounts (name, type, role) VALUES ('Demo Checking', 'checking', 'budgeted')
         RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

type Stored = (String, i64, String, String, String, Option<String>);

async fn stored(pool: &SqlitePool, id: &str) -> Stored {
    sqlx::query_as(
        "SELECT date, amount_minor, status, raw_description, source, sync_flag
         FROM transactions WHERE external_id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn new_rows_are_inserted_as_the_provider_s_and_a_second_pass_changes_nothing() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    let rows = [
        row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE"),
        row("TRN-2", "2026-10-27", -1200, true, "DEMO CAFE"),
    ];
    let now = at("2026-10-28T09:00:00Z");
    let first = apply_synced_rows(&pool, account, &rows, start(), now)
        .await
        .unwrap();
    assert_eq!(
        first,
        Applied {
            added: 2,
            updated: 0
        }
    );
    assert_eq!(
        stored(&pool, "TRN-2").await,
        (
            "2026-10-27".to_owned(),
            -1200,
            "pending".to_owned(),
            "DEMO CAFE".to_owned(),
            "provider".to_owned(),
            None
        )
    );
    // Both rows share one payee, made with its alias.
    let payees: Vec<(String, i64)> = sqlx::query_as(
        "SELECT p.name, (SELECT COUNT(*) FROM transactions t WHERE t.payee_id = p.id)
         FROM payees p",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(payees, [("Demo Cafe".to_owned(), 2)]);
    let alias: String = sqlx::query_scalar("SELECT normalized_text FROM payee_aliases")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(alias, "demo cafe");

    let again = apply_synced_rows(&pool, account, &rows, start(), now)
        .await
        .unwrap();
    assert_eq!(again, Applied::default());
}

#[tokio::test]
async fn an_existing_alias_picks_the_payee() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    sqlx::query("INSERT INTO payees (id, name) VALUES (5, 'Corner Cafe')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO payee_aliases (normalized_text, payee_id) VALUES ('demo cafe', 5)")
        .execute(&pool)
        .await
        .unwrap();
    let rows = [row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE")];
    apply_synced_rows(&pool, account, &rows, start(), at("2026-10-28T09:00:00Z"))
        .await
        .unwrap();
    let payee: i64 = sqlx::query_scalar("SELECT payee_id FROM transactions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(payee, 5);
}

#[tokio::test]
async fn a_known_id_updates_only_the_sync_owned_fields() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    let now = at("2026-10-28T09:00:00Z");
    apply_synced_rows(
        &pool,
        account,
        &[row("TRN-1", "2026-10-27", -4000, true, "DEMO BISTRO")],
        start(),
        now,
    )
    .await
    .unwrap();
    // The household sets its fields.
    sqlx::query("INSERT INTO category_groups (id, name) VALUES (1, 'Food')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO categories (id, group_id, name) VALUES (9, 1, 'Eating out')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO payees (id, name) VALUES (7, 'The Bistro')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tags (id, name) VALUES (3, 'trip')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE transactions SET category_id = 9, categorized_by = 'user', memo = 'dinner',
           payee_id = 7, exclude_from_budget = 1",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transaction_tags (transaction_id, tag_id) SELECT id, 3 FROM transactions",
    )
    .execute(&pool)
    .await
    .unwrap();

    let posted = [row("TRN-1", "2026-10-28", -4800, false, "DEMO BISTRO 123")];
    let applied = apply_synced_rows(&pool, account, &posted, start(), now)
        .await
        .unwrap();
    assert_eq!(
        applied,
        Applied {
            added: 0,
            updated: 1
        }
    );
    assert_eq!(
        stored(&pool, "TRN-1").await,
        (
            "2026-10-28".to_owned(),
            -4800,
            "posted".to_owned(),
            "DEMO BISTRO 123".to_owned(),
            "provider".to_owned(),
            None
        )
    );
    let household: (i64, String, String, i64, i64, i64) = sqlx::query_as(
        "SELECT category_id, categorized_by, memo, payee_id, exclude_from_budget,
           (SELECT tag_id FROM transaction_tags) FROM transactions",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        household,
        (9, "user".to_owned(), "dinner".to_owned(), 7, 1, 3)
    );
}

#[tokio::test]
async fn a_split_row_whose_amount_changes_is_flagged() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    let now = at("2026-10-28T09:00:00Z");
    apply_synced_rows(
        &pool,
        account,
        &[row("TRN-1", "2026-10-27", -4000, false, "DEMO MART")],
        start(),
        now,
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO category_groups (id, name) VALUES (1, 'Home')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO categories (id, group_id, name) VALUES (9, 1, 'Supplies')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transaction_splits (transaction_id, category_id, amount_minor)
         SELECT id, 9, -2000 FROM transactions UNION ALL SELECT id, 9, -2000 FROM transactions",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Text alone changing leaves the split be.
    apply_synced_rows(
        &pool,
        account,
        &[row("TRN-1", "2026-10-27", -4000, false, "DEMO MART #2")],
        start(),
        now,
    )
    .await
    .unwrap();
    assert_eq!(stored(&pool, "TRN-1").await.5, None);
    apply_synced_rows(
        &pool,
        account,
        &[row("TRN-1", "2026-10-27", -4500, false, "DEMO MART #2")],
        start(),
        now,
    )
    .await
    .unwrap();
    assert_eq!(
        stored(&pool, "TRN-1").await.5.as_deref(),
        Some("amount_changed_split")
    );
}

#[tokio::test]
async fn an_id_the_household_deleted_is_skipped() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    sqlx::query("INSERT INTO deleted_bank_rows (account_id, external_id) VALUES (?, 'TRN-1')")
        .bind(account)
        .execute(&pool)
        .await
        .unwrap();
    let applied = apply_synced_rows(
        &pool,
        account,
        &[
            row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE"),
            row("TRN-2", "2026-10-21", -575, false, "DEMO CAFE"),
        ],
        start(),
        at("2026-10-28T09:00:00Z"),
    )
    .await
    .unwrap();
    assert_eq!(applied.added, 1);
    let ids: Vec<String> = sqlx::query_scalar("SELECT external_id FROM transactions")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(ids, ["TRN-2"]);
}

async fn exec(pool: &SqlitePool, sql: &'static str) {
    sqlx::query(sql).execute(pool).await.unwrap();
}

/// `(external id, status, sync flag, possible duplicate of)` of every row, by id.
async fn listed(pool: &SqlitePool) -> Vec<(Option<String>, String, Option<String>, Option<i64>)> {
    sqlx::query_as(
        "SELECT external_id, status, sync_flag, possible_duplicate_of FROM transactions
         ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// A category, a payee and a tag for the household to set.
async fn household_things(pool: &SqlitePool) {
    exec(
        pool,
        "INSERT INTO category_groups (id, name) VALUES (1, 'Food')",
    )
    .await;
    exec(
        pool,
        "INSERT INTO categories (id, group_id, name) VALUES (9, 1, 'Eating out')",
    )
    .await;
    exec(
        pool,
        "INSERT INTO categories (id, group_id, name) VALUES (10, 1, 'Coffee')",
    )
    .await;
    exec(
        pool,
        "INSERT INTO payees (id, name) VALUES (7, 'The Bistro')",
    )
    .await;
    exec(pool, "INSERT INTO tags (id, name) VALUES (3, 'trip')").await;
}

#[tokio::test]
async fn a_pending_row_posting_under_a_new_id_keeps_what_the_household_set() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    let now = at("2026-10-28T09:00:00Z");
    let held = [row("TRN-tab-1", "2026-10-26", -4000, true, "DEMO BISTRO")];
    apply_synced_rows(&pool, account, &held, start(), now)
        .await
        .unwrap();
    household_things(&pool).await;
    exec(
        &pool,
        "UPDATE transactions SET memo = 'dinner', payee_id = 7, categorized_by = 'user'",
    )
    .await;
    exec(
        &pool,
        "INSERT INTO transaction_splits (transaction_id, category_id, amount_minor)
         SELECT id, 9, -3000 FROM transactions UNION ALL SELECT id, 10, -1000 FROM transactions",
    )
    .await;
    exec(
        &pool,
        "INSERT INTO transaction_tags SELECT id, 3 FROM transactions",
    )
    .await;

    let posted = [row(
        "TRN-tab-2",
        "2026-10-27",
        -4800,
        false,
        "DEMO BISTRO 4411",
    )];
    let applied = apply_synced_rows(&pool, account, &posted, start(), now)
        .await
        .unwrap();
    assert_eq!(
        applied,
        Applied {
            added: 0,
            updated: 1
        }
    );
    // The same row, now posted under the new id; its split is flagged since the amount moved.
    assert_eq!(
        listed(&pool).await,
        [(
            Some("TRN-tab-2".to_owned()),
            "posted".to_owned(),
            Some("amount_changed_split".to_owned()),
            None
        )]
    );
    let kept: (String, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT memo, payee_id, amount_minor,
           (SELECT COUNT(*) FROM transaction_splits), (SELECT COUNT(*) FROM transaction_tags)
         FROM transactions",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kept, ("dinner".to_owned(), 7, -4800, 2, 1));
}

#[tokio::test]
async fn a_re_issued_id_is_recorded() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    let now = at("2026-10-28T09:00:00Z");
    let first = [row(
        "TRN-a",
        "2026-10-20",
        -450,
        false,
        "DEMO COFFEE ROASTERS",
    )];
    apply_synced_rows(&pool, account, &first, start(), now)
        .await
        .unwrap();
    let again = [row(
        "TRN-b",
        "2026-10-21",
        -450,
        false,
        "DEMO COFFEE ROASTERS",
    )];
    let applied = apply_synced_rows(&pool, account, &again, start(), now)
        .await
        .unwrap();
    assert_eq!(
        applied,
        Applied {
            added: 0,
            updated: 1
        }
    );
    assert_eq!(
        listed(&pool).await,
        [(Some("TRN-b".to_owned()), "posted".to_owned(), None, None)]
    );
}

#[tokio::test]
async fn an_ambiguous_re_issued_id_is_flagged() {
    // Two equal rows a day apart, then both under new ids: neither can be told apart.
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    let now = at("2026-10-28T09:00:00Z");
    let first = [
        row("TRN-a", "2026-10-20", -450, false, "DEMO COFFEE ROASTERS"),
        row("TRN-b", "2026-10-21", -450, false, "DEMO COFFEE ROASTERS"),
    ];
    apply_synced_rows(&pool, account, &first, start(), now)
        .await
        .unwrap();
    let again = [
        row("TRN-c", "2026-10-20", -450, false, "DEMO COFFEE ROASTERS"),
        row("TRN-d", "2026-10-21", -450, false, "DEMO COFFEE ROASTERS"),
    ];
    let applied = apply_synced_rows(&pool, account, &again, start(), now)
        .await
        .unwrap();
    assert_eq!(
        applied,
        Applied {
            added: 2,
            updated: 0
        }
    );
    let flag = Some("possible_duplicate".to_owned());
    let rows = listed(&pool).await;
    let first_id: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = 'TRN-a'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        rows[2],
        (
            Some("TRN-c".to_owned()),
            "posted".to_owned(),
            flag.clone(),
            Some(first_id)
        )
    );
    assert_eq!(
        rows[3],
        (
            Some("TRN-d".to_owned()),
            "posted".to_owned(),
            flag,
            Some(first_id)
        )
    );
}

#[tokio::test]
async fn an_imported_row_without_a_bank_id_is_adopted_and_ties_are_flagged() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    household_things(&pool).await;
    for (day, memo) in [
        ("2026-10-18", "first"),
        ("2026-10-18", "second"),
        ("2026-10-10", "older"),
    ] {
        sqlx::query(
            "INSERT INTO transactions (account_id, date, amount_minor, status, raw_description,
               source, dedupe_key, category_id, categorized_by, memo)
             VALUES (?, ?, -1250, 'posted', 'Demo Books', 'import', 'k', 9, 'import', ?)",
        )
        .bind(account)
        .bind(day)
        .bind(memo)
        .execute(&pool)
        .await
        .unwrap();
    }
    // Two identical rows on the bank, two in the import: one each. The third isn't in reach.
    let bank = [
        row("TRN-b1", "2026-10-20", -1250, false, "DEMO BOOKS"),
        row("TRN-b2", "2026-10-20", -1250, false, "DEMO BOOKS"),
    ];
    let now = at("2026-10-28T09:00:00Z");
    let applied = apply_synced_rows(&pool, account, &bank, start(), now)
        .await
        .unwrap();
    assert_eq!(
        applied,
        Applied {
            added: 0,
            updated: 2
        }
    );
    let adopted: Vec<(Option<String>, String, String, i64, String)> = sqlx::query_as(
        "SELECT external_id, source, memo, category_id, date FROM transactions ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        adopted,
        [
            (
                Some("TRN-b1".to_owned()),
                "provider".to_owned(),
                "first".to_owned(),
                9,
                "2026-10-20".to_owned()
            ),
            (
                Some("TRN-b2".to_owned()),
                "provider".to_owned(),
                "second".to_owned(),
                9,
                "2026-10-20".to_owned()
            ),
            (
                None,
                "import".to_owned(),
                "older".to_owned(),
                9,
                "2026-10-10".to_owned()
            ),
        ]
    );

    // Two imported rows equally near on either side: a tie, flagged.
    exec(&pool, "DELETE FROM transactions").await;
    for day in ["2026-10-19", "2026-10-21"] {
        sqlx::query(
            "INSERT INTO transactions (account_id, date, amount_minor, status, raw_description,
               source, dedupe_key)
             VALUES (?, ?, -300, 'posted', 'DEMO TRANSIT', 'import', 'k')",
        )
        .bind(account)
        .bind(day)
        .execute(&pool)
        .await
        .unwrap();
    }
    let bank = [row("TRN-t1", "2026-10-20", -300, false, "DEMO TRANSIT")];
    let applied = apply_synced_rows(&pool, account, &bank, start(), now)
        .await
        .unwrap();
    assert_eq!(
        applied,
        Applied {
            added: 1,
            updated: 0
        }
    );
    let rows = listed(&pool).await;
    assert_eq!(rows[2].2.as_deref(), Some("possible_duplicate"));
}

#[tokio::test]
async fn a_pending_row_missing_twice_is_removed_and_kept_if_edited() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    let now = at("2026-10-28T09:00:00Z");
    let held = [
        row("TRN-hold", "2026-10-26", -15000, true, "DEMO HOTEL DEPOSIT"),
        row("TRN-snack", "2026-10-27", -300, true, "DEMO SNACKS"),
        row("TRN-rent", "2026-10-01", -90000, false, "DEMO RENT"),
    ];
    apply_synced_rows(&pool, account, &held, start(), now)
        .await
        .unwrap();
    household_things(&pool).await;
    exec(
        &pool,
        "UPDATE transactions SET memo = 'trip' WHERE external_id = 'TRN-hold'",
    )
    .await;

    // One miss: both stay, counted.
    let rent = [held[2].clone()];
    apply_synced_rows(&pool, account, &rent, start(), now)
        .await
        .unwrap();
    let counts: Vec<i64> =
        sqlx::query_scalar("SELECT pending_miss_count FROM transactions ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(counts, [1, 1, 0]);

    // The second: both go; only the edited one is listed as dropped.
    apply_synced_rows(&pool, account, &rent, start(), now)
        .await
        .unwrap();
    assert_eq!(
        listed(&pool).await,
        [(Some("TRN-rent".to_owned()), "posted".to_owned(), None, None)]
    );
    let dropped: Vec<(i64, i64, String, String)> = sqlx::query_as(
        "SELECT account_id, user_edited, snapshot ->> '$.transaction.external_id',
           snapshot ->> '$.transaction.memo'
         FROM dropped_transactions ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(dropped.len(), 2);
    assert_eq!(
        dropped[0],
        (account, 1, "TRN-hold".to_owned(), "trip".to_owned())
    );
    assert_eq!((dropped[1].1, dropped[1].2.as_str()), (0, "TRN-snack"));
}

#[tokio::test]
async fn a_pending_row_that_comes_back_starts_its_count_again() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    let now = at("2026-10-28T09:00:00Z");
    let held = [row(
        "TRN-hold",
        "2026-10-26",
        -15000,
        true,
        "DEMO HOTEL DEPOSIT",
    )];
    apply_synced_rows(&pool, account, &held, start(), now)
        .await
        .unwrap();
    apply_synced_rows(&pool, account, &[], start(), now)
        .await
        .unwrap();
    apply_synced_rows(&pool, account, &held, start(), now)
        .await
        .unwrap();
    apply_synced_rows(&pool, account, &[], start(), now)
        .await
        .unwrap();
    assert_eq!(listed(&pool).await.len(), 1);
}

#[tokio::test]
async fn a_posted_row_unseen_for_a_week_inside_the_window_is_flagged() {
    let (_dir, pool) = migrated().await;
    let account = account(&pool).await;
    let then = at("2026-10-20T09:00:00Z");
    let rows = [
        row("TRN-gym", "2026-10-10", -3500, false, "DEMO GYM"),
        // Too near the window's start to count.
        row("TRN-edge", "2026-10-02", -800, false, "DEMO EDGE"),
        row("TRN-pay", "2026-10-15", 240_000, false, "DEMO PAYROLL"),
    ];
    apply_synced_rows(&pool, account, &rows, start(), then)
        .await
        .unwrap();
    let seen = [rows[2].clone()];

    // Six days unseen: nothing yet.
    apply_synced_rows(&pool, account, &seen, start(), at("2026-10-26T09:00:00Z"))
        .await
        .unwrap();
    assert!(listed(&pool).await.iter().all(|r| r.2.is_none()));

    // Over seven: flagged, `last_seen_at` kept.
    let now = at("2026-10-28T09:00:00Z");
    apply_synced_rows(&pool, account, &seen, start(), now)
        .await
        .unwrap();
    let flags: Vec<(String, Option<String>, String)> =
        sqlx::query_as("SELECT external_id, sync_flag, last_seen_at FROM transactions ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        flags,
        [
            (
                "TRN-gym".to_owned(),
                Some("not_seen_at_bank".to_owned()),
                crate::stamp(then)
            ),
            ("TRN-edge".to_owned(), None, crate::stamp(then)),
            ("TRN-pay".to_owned(), None, crate::stamp(now)),
        ]
    );

    // It comes back: the flag goes.
    apply_synced_rows(&pool, account, &rows, start(), now)
        .await
        .unwrap();
    assert!(listed(&pool).await.iter().all(|r| r.2.is_none()));
}

#[tokio::test]
async fn a_deleted_bank_row_is_remembered_and_never_comes_back() {
    let (_dir, pool) = crate::tests::set_up().await;
    let account = account(&pool).await;
    let now = at("2026-10-28T09:00:00Z");
    let rows = [
        row("TRN-a", "2026-10-20", -450, false, "DEMO COFFEE ROASTERS"),
        row("TRN-b", "2026-10-21", -450, false, "DEMO COFFEE ROASTERS"),
    ];
    apply_synced_rows(&pool, account, &rows, start(), now)
        .await
        .unwrap();
    // A third, flagged as a possible duplicate of the first.
    let first: i64 = sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = 'TRN-a'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (account_id, date, amount_minor, status, raw_description,
           source, external_id, dedupe_key, sync_flag, possible_duplicate_of)
         VALUES (?, '2026-10-20', -450, 'posted', 'DEMO COFFEE ROASTERS', 'provider', 'TRN-c',
           'k', 'possible_duplicate', ?)",
    )
    .bind(account)
    .bind(first)
    .execute(&pool)
    .await
    .unwrap();
    let user: i64 = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();

    delete_bank_row(&pool, first, user, now).await.unwrap();
    let remembered: (String, i64) =
        sqlx::query_as("SELECT external_id, deleted_by FROM deleted_bank_rows")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remembered, ("TRN-a".to_owned(), user));
    // The duplicate it pointed at stands alone now.
    let all = [
        rows[0].clone(),
        rows[1].clone(),
        row("TRN-c", "2026-10-20", -450, false, "DEMO COFFEE ROASTERS"),
    ];
    let applied = apply_synced_rows(&pool, account, &all, start(), now)
        .await
        .unwrap();
    assert_eq!(applied, Applied::default());
    assert_eq!(
        listed(&pool).await,
        [
            (Some("TRN-b".to_owned()), "posted".to_owned(), None, None),
            (Some("TRN-c".to_owned()), "posted".to_owned(), None, None),
        ]
    );

    assert!(matches!(
        delete_bank_row(&pool, 999, user, now).await,
        Err(DeleteBankRowError::NotFound)
    ));
    exec(
        &pool,
        "INSERT INTO transactions (id, account_id, date, amount_minor, status, raw_description,
           source, dedupe_key) VALUES (50, 1, '2026-10-20', -1, 'posted', 'x', 'import', 'k')",
    )
    .await;
    assert!(matches!(
        delete_bank_row(&pool, 50, user, now).await,
        Err(DeleteBankRowError::NotFromBank)
    ));
}
