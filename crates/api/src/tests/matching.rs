//! Matching synced rows and the rows the bank stops sending, through Sync now against the fake
//! SimpleFIN server (docs/spec/m2-bank-data.md#matching-a-synced-row and
//! #rows-the-bank-stops-sending): one test per rule, each ending in the spec's outcome.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use simplefin_fake::{NOW, Scenario};

use super::sync::{DAY, claimed_app, days_ago, rows, sync_now, track};
use super::{App, get, send, with};

async fn exec(app: &App, sql: &'static str, account: i64) {
    sqlx::query(sql)
        .bind(account)
        .execute(&app.pool)
        .await
        .unwrap();
}

/// `(external id, sync flag)` of an account's rows, by id.
async fn flags(app: &App, account: i64) -> Vec<(Option<String>, Option<String>)> {
    sqlx::query_as(
        "SELECT external_id, sync_flag FROM transactions WHERE account_id = ? ORDER BY id",
    )
    .bind(account)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

async fn ok_sync(app: &App) -> Value {
    let (status, run) = sync_now(app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(run["error"], Value::Null, "{run}");
    run
}

/// A category the household sets by hand, and a memo, on the row with `external_id`.
async fn household_edits(app: &App, external_id: &str) {
    for sql in [
        "INSERT INTO category_groups (id, name) VALUES (90, 'Out')",
        "INSERT INTO categories (id, group_id, name) VALUES (91, 90, 'Eating out')",
    ] {
        sqlx::query(sql).execute(&app.pool).await.unwrap();
    }
    sqlx::query(
        "UPDATE transactions SET category_id = 91, categorized_by = 'user', memo = 'team lunch'
         WHERE external_id = ?",
    )
    .bind(external_id)
    .execute(&app.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn a_pending_row_posting_under_a_new_id_with_a_tip_keeps_the_household_s_fields() {
    let (server, app) = claimed_app(Scenario::PendingAmountChanges).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    household_edits(&app, "TRN-demo-tab-1").await;

    server.next_day();
    let run = ok_sync(&app).await;
    assert_eq!((&run["added"], &run["updated"]), (&json!(0), &json!(1)));
    let read = rows(&app, checking).await;
    assert_eq!(read.len(), 5, "{read:?}");
    let tab = read.iter().find(|r| r.0 == "TRN-demo-tab-2").unwrap();
    assert_eq!(
        (tab.1.as_str(), tab.2, tab.3.as_str()),
        (days_ago(1).as_str(), -4800, "posted")
    );
    let kept: (i64, String) = sqlx::query_as(
        "SELECT category_id, memo FROM transactions WHERE external_id = 'TRN-demo-tab-2'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(kept, (91, "team lunch".to_owned()));
}

#[tokio::test]
async fn a_re_issued_id_is_recorded_on_its_row() {
    let (server, app) = claimed_app(Scenario::IdReissued).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    let before: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = 'TRN-demo-cafe-2'")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    server.next_day();
    let run = ok_sync(&app).await;
    assert_eq!((&run["added"], &run["updated"]), (&json!(0), &json!(1)));
    let after: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = 'TRN-demo-cafe-3'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(after, before);
    assert_eq!(rows(&app, checking).await.len(), 5);
}

#[tokio::test]
async fn ids_re_issued_together_are_inserted_as_possible_duplicates() {
    let (server, app) = claimed_app(Scenario::IdsReissuedTogether).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    let first: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = 'TRN-demo-park-1'")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    server.next_day();
    let run = ok_sync(&app).await;
    assert_eq!(run["added"], 2, "{run}");
    let flagged: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT external_id, sync_flag, possible_duplicate_of FROM transactions
         WHERE account_id = ? AND sync_flag IS NOT NULL ORDER BY external_id",
    )
    .bind(checking)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    let flag = "possible_duplicate".to_owned();
    assert_eq!(
        flagged,
        [
            ("TRN-demo-park-3".to_owned(), flag.clone(), first),
            ("TRN-demo-park-4".to_owned(), flag, first),
        ]
    );
}

#[tokio::test]
async fn an_imported_row_without_a_bank_id_is_adopted() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    // The groceries came in from Actual without a bank id, a day off, categorized there.
    exec(
        &app,
        "DELETE FROM transactions WHERE account_id = ? AND external_id = 'TRN-demo-groc-1'",
        checking,
    )
    .await;
    household_edits(&app, "none").await;
    sqlx::query(
        "INSERT INTO transactions (account_id, date, amount_minor, status, raw_description,
           source, dedupe_key, category_id, categorized_by, memo)
         VALUES (?, ?, -8612, 'posted', 'Demo Grocer', 'import', 'k', 91, 'import', 'weekly')",
    )
    .bind(checking)
    .bind(days_ago(10))
    .execute(&app.pool)
    .await
    .unwrap();

    let run = ok_sync(&app).await;
    assert_eq!((&run["added"], &run["updated"]), (&json!(0), &json!(1)));
    let adopted: (String, String, String, i64, String) = sqlx::query_as(
        "SELECT external_id, source, date, category_id, memo FROM transactions
         WHERE account_id = ? AND amount_minor = -8612",
    )
    .bind(checking)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        adopted,
        (
            "TRN-demo-groc-1".to_owned(),
            "provider".to_owned(),
            days_ago(9),
            91,
            "weekly".to_owned()
        )
    );
}

#[tokio::test]
async fn a_pending_row_the_bank_drops_goes_after_two_runs_and_is_kept_if_edited() {
    let (server, app) = claimed_app(Scenario::PendingDropped).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    household_edits(&app, "TRN-demo-hold-1").await;

    server.next_day();
    ok_sync(&app).await;
    let misses: i64 = sqlx::query_scalar(
        "SELECT pending_miss_count FROM transactions WHERE external_id = 'TRN-demo-hold-1'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(misses, 1);

    ok_sync(&app).await;
    assert_eq!(rows(&app, checking).await.len(), 4);
    let dropped: (i64, String) = sqlx::query_as(
        "SELECT user_edited, snapshot ->> '$.transaction.memo' FROM dropped_transactions",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(dropped, (1, "team lunch".to_owned()));
}

#[tokio::test]
async fn an_account_missing_for_a_run_keeps_its_pending_rows_and_two_real_misses_remove_them() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    let savings = track(&app, "ACT-demo-savings", false).await;
    sqlx::query(
        "INSERT INTO transactions (account_id, date, amount_minor, status, raw_description,
           source, external_id, dedupe_key, last_seen_at)
         VALUES (?, ?, -2500, 'pending', 'DEMO TRANSFER OUT', 'provider', 'TRN-demo-xfer-1',
           'k', '2026-10-27T09:00:00.000Z')",
    )
    .bind(savings)
    .bind(days_ago(1))
    .execute(&app.pool)
    .await
    .unwrap();
    let misses = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT pending_miss_count FROM transactions WHERE external_id = 'TRN-demo-xfer-1'",
        )
        .fetch_optional(&app.pool)
        .await
        .unwrap()
    };

    // Missing twice: it fails both runs, and the count doesn't move.
    server.set_scenario(Scenario::AccountMissing);
    for _ in 0..2 {
        sync_now(&app).await;
        assert_eq!(misses().await, Some(0));
    }
    server.set_scenario(Scenario::Steady);
    ok_sync(&app).await;
    assert_eq!(misses().await, Some(1));
    ok_sync(&app).await;
    assert_eq!(misses().await, None);
    assert_eq!(rows(&app, savings).await.len(), 1);
}

#[tokio::test]
async fn a_posted_row_unseen_for_a_week_is_flagged_not_seen_at_bank() {
    let (server, app) = claimed_app(Scenario::PostedDropped).await;
    let checking = track(&app, "ACT-demo-checking", false).await;

    server.next_day();
    *app.now.lock().unwrap() = jiff::Timestamp::from_second(NOW + 6 * DAY).unwrap();
    ok_sync(&app).await;
    assert!(flags(&app, checking).await.iter().all(|r| r.1.is_none()));

    *app.now.lock().unwrap() = jiff::Timestamp::from_second(NOW + 8 * DAY).unwrap();
    ok_sync(&app).await;
    let flagged: Vec<String> = flags(&app, checking)
        .await
        .into_iter()
        .filter(|r| r.1.as_deref() == Some("not_seen_at_bank"))
        .filter_map(|r| r.0)
        .collect();
    assert_eq!(flagged, ["TRN-demo-gym-1"]);
    let last_seen: String = sqlx::query_scalar(
        "SELECT last_seen_at FROM transactions WHERE external_id = 'TRN-demo-gym-1'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(last_seen, "2026-10-28T09:00:00.000Z");
}

fn delete(uri: &str) -> Request<Body> {
    Request::delete(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn a_deleted_synced_row_never_comes_back() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    let rent: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = 'TRN-demo-rent-1'")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    let uri = format!("/api/v1/transactions/{rent}");
    let (status, body) = send(app.router.clone(), with(&app.cookie, delete(&uri))).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send(app.router.clone(), with(&app.cookie, get(&uri))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let run = ok_sync(&app).await;
    assert_eq!(run["added"], 0, "{run}");
    let ids: Vec<String> = rows(&app, checking)
        .await
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert!(!ids.contains(&"TRN-demo-rent-1".to_owned()), "{ids:?}");
    assert_eq!(ids.len(), 3);
}

#[tokio::test]
async fn excluding_a_row_from_budgets_is_the_alternative_to_deleting_it() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    let rent: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = 'TRN-demo-rent-1'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let uri = format!("/api/v1/transactions/{rent}");
    let (_, shown) = send(app.router.clone(), with(&app.cookie, get(&uri))).await;
    assert_eq!(shown["excluded"], false);

    let put = |excluded: bool| {
        Request::put(format!("{uri}/excluded"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "excluded": excluded }).to_string()))
            .unwrap()
    };
    let (status, changed) = send(app.router.clone(), with(&app.cookie, put(true))).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["excluded"], true);
    // A sync leaves it excluded: the household owns it.
    ok_sync(&app).await;
    let (_, shown) = send(app.router.clone(), with(&app.cookie, get(&uri))).await;
    assert_eq!(shown["excluded"], true);

    let missing = Request::put("/api/v1/transactions/999/excluded")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "excluded": true }).to_string()))
        .unwrap();
    let (status, _) = send(app.router.clone(), with(&app.cookie, missing)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
