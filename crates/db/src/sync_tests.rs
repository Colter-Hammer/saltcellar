use domain::Money;
use domain::accounts::Role;
use domain::feed::SyncedRow;

use super::*;
use crate::tests::migrated;
use crate::{ReportedAccount, Track, record_connection_accounts, save_connection};

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn date(text: &str) -> domain::Date {
    let d: jiff::civil::Date = text.parse().unwrap();
    domain::Date::from_ymd(d.year(), d.month(), d.day()).unwrap()
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
    let first = apply_synced_rows(&pool, account, &rows, now).await.unwrap();
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

    let again = apply_synced_rows(&pool, account, &rows, now).await.unwrap();
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
    apply_synced_rows(&pool, account, &rows, at("2026-10-28T09:00:00Z"))
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
    let applied = apply_synced_rows(&pool, account, &posted, now)
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
        now,
    )
    .await
    .unwrap();
    assert_eq!(stored(&pool, "TRN-1").await.5, None);
    apply_synced_rows(
        &pool,
        account,
        &[row("TRN-1", "2026-10-27", -4500, false, "DEMO MART #2")],
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

#[tokio::test]
async fn a_run_is_recorded_with_its_accounts_and_the_back_fill_is_marked() {
    let (_dir, pool) = migrated().await;
    assert_eq!(latest_sync_run(&pool).await.unwrap(), None);
    let connection = save_connection(&pool, b"sealed").await.unwrap();
    let reported = ReportedAccount {
        external_id: "ACT-demo-checking".to_owned(),
        name: "Demo Everyday Checking".to_owned(),
        institution: None,
        currency: "USD".to_owned(),
    };
    record_connection_accounts(
        &pool,
        connection,
        &[reported],
        "USD",
        at("2026-10-28T09:00:00Z"),
    )
    .await
    .unwrap();
    let track = Track {
        role: Role::Budgeted,
        balances_only: false,
        same_as: None,
    };
    let account = crate::track_connection_account(&pool, "ACT-demo-checking", &track, "USD")
        .await
        .unwrap();
    let links = tracked_feed_accounts(&pool, connection).await.unwrap();
    assert_eq!(
        links,
        [FeedLink {
            account_id: account,
            external_id: "ACT-demo-checking".to_owned(),
            balances_only: false,
            backfilled: false,
        }]
    );

    let start = at("2026-10-28T09:00:00Z");
    let run = start_sync_run(&pool, connection, SyncTrigger::Backfill, start)
        .await
        .unwrap();
    let running = latest_sync_run(&pool).await.unwrap().unwrap();
    assert_eq!(
        (running.id, running.trigger, running.finished_at),
        (run, SyncTrigger::Backfill, None)
    );
    let end = RunEnd {
        requests: 2,
        added: 6,
        updated: 0,
        error_code: None,
        error: None,
    };
    let result = AccountResult {
        account_id: account,
        ok: true,
        error_code: None,
        error: None,
        balance_date: Some(at("2026-10-28T08:00:00Z")),
    };
    let done = at("2026-10-28T09:00:02Z");
    finish_sync_run(&pool, run, &end, &[result], done)
        .await
        .unwrap();
    mark_backfilled(&pool, connection, &["ACT-demo-checking".to_owned()], done)
        .await
        .unwrap();

    let finished = latest_sync_run(&pool).await.unwrap().unwrap();
    assert_eq!(
        (
            finished.finished_at,
            finished.requests,
            finished.added,
            finished.updated
        ),
        (Some(done), 2, 6, 0)
    );
    let saved: (i64, Option<String>) = sqlx::query_as(
        "SELECT ok, provider_balance_date FROM sync_account_results WHERE sync_run_id = ?",
    )
    .bind(run)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(saved, (1, Some("2026-10-28T08:00:00.000Z".to_owned())));
    assert!(tracked_feed_accounts(&pool, connection).await.unwrap()[0].backfilled);
    assert_eq!(
        crate::connection(&pool)
            .await
            .unwrap()
            .unwrap()
            .last_sync_at,
        Some(done)
    );

    // Disconnecting takes the history with it.
    crate::disconnect(&pool).await.unwrap();
    assert_eq!(latest_sync_run(&pool).await.unwrap(), None);
}

#[test]
fn triggers_read_back() {
    for trigger in [
        SyncTrigger::Scheduled,
        SyncTrigger::Manual,
        SyncTrigger::Retry,
        SyncTrigger::CatchUp,
        SyncTrigger::Backfill,
    ] {
        assert_eq!(SyncTrigger::parse(trigger.as_str()).unwrap(), trigger);
    }
    assert!(SyncTrigger::parse("nope").is_err());
}
