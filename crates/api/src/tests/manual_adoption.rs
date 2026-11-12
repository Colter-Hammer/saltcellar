//! Sync adopting manual rows, through Sync now against the fake SimpleFIN server
//! (docs/spec/m2-bank-data.md#matching-a-synced-row step 5, and #manual-entry): one test per rule.
//! The `manual-rows-arrive` scenario sends a €120.00 check and a €46.00 payment app transfer, a
//! day old, from day 1.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use simplefin_fake::{FakeServer, Scenario};

use super::sync::{claimed_app, days_ago, sync_now, track};
use super::{App, get, send, with};

async fn call(app: &App, method: &str, uri: &str, body: &Value) -> (StatusCode, Value) {
    let request = if method == "GET" {
        get(uri)
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    send(app.router.clone(), with(&app.cookie, request)).await
}

/// A claimed install tracking the checking account, with a category to enter rows in.
async fn synced_checking() -> (FakeServer, App, i64) {
    let (server, app) = claimed_app(Scenario::ManualRowsArrive).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    sqlx::query(
        "INSERT INTO category_groups (id, name) VALUES (90, 'Home');
         INSERT INTO categories (id, group_id, name) VALUES (91, 90, 'Childcare');",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    (server, app, checking)
}

fn entry(account: i64, days: i64, minor: i64) -> Value {
    json!({
        "account_id": account, "date": days_ago(days), "amount_minor": minor,
        "payee": "Fake Landlord", "category_id": 91, "memo": "deposit", "tags": ["home"],
    })
}

/// Enters a manual row, resolving to its id.
async fn enter(app: &App, body: &Value) -> i64 {
    let (status, row) = call(app, "POST", "/api/v1/transactions", body).await;
    assert_eq!(status, StatusCode::CREATED, "{row}");
    row["id"].as_i64().unwrap()
}

async fn ok_sync(server: &FakeServer, app: &App) -> Value {
    server.next_day();
    let (status, run) = sync_now(app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(run["error"], Value::Null, "{run}");
    run
}

/// `(id, external id, source, sync flag, possible duplicate of)` of the account's rows of
/// `minor`, by id.
type Row = (i64, Option<String>, String, Option<String>, Option<i64>);
async fn of_amount(app: &App, account: i64, minor: i64) -> Vec<Row> {
    sqlx::query_as(
        "SELECT id, external_id, source, sync_flag, possible_duplicate_of FROM transactions
         WHERE account_id = ? AND amount_minor = ? ORDER BY id",
    )
    .bind(account)
    .bind(minor)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn the_bank_row_adopts_a_manual_row_which_keeps_the_household_s_fields() {
    let (server, app, checking) = synced_checking().await;
    // Written three days before the bank's date, at the edge of the window.
    let id = enter(&app, &entry(checking, 4, -12000)).await;

    let run = ok_sync(&server, &app).await;
    assert_eq!((&run["added"], &run["updated"]), (&json!(1), &json!(1)));
    assert_eq!(
        of_amount(&app, checking, -12000).await,
        [(
            id,
            Some("TRN-demo-check-1".to_owned()),
            "provider".to_owned(),
            None,
            None
        )]
    );
    let (_, row) = call(
        &app,
        "GET",
        &format!("/api/v1/transactions/{id}"),
        &Value::Null,
    )
    .await;
    assert_eq!(row["manual"], false, "{row}");
    assert_eq!(row["date"], days_ago(1));
    assert_eq!(row["category"], "Childcare");
    assert_eq!(row["memo"], "deposit");
    assert_eq!(row["tags"], json!(["home"]));
    assert_eq!(row["payee"], "Fake Landlord");
}

#[tokio::test]
async fn matching_uses_the_manual_row_s_edited_values() {
    let (server, app, checking) = synced_checking().await;
    // Entered as €100.00 a month ago, then corrected to the check's €120.00 two days ago.
    let id = enter(&app, &entry(checking, 30, -10000)).await;
    let uri = format!("/api/v1/transactions/{id}");
    let (status, edited) = call(&app, "PUT", &uri, &entry(checking, 2, -12000)).await;
    assert_eq!(status, StatusCode::OK, "{edited}");

    ok_sync(&server, &app).await;
    let rows = of_amount(&app, checking, -12000).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].0, rows[0].1.as_deref()),
        (id, Some("TRN-demo-check-1"))
    );
}

#[tokio::test]
async fn two_manual_rows_the_bank_row_could_be_make_it_a_possible_duplicate() {
    let (server, app, checking) = synced_checking().await;
    let older = enter(&app, &entry(checking, 3, -12000)).await;
    let newer = enter(&app, &entry(checking, 2, -12000)).await;

    ok_sync(&server, &app).await;
    let manual = "manual".to_owned();
    let provider = "provider".to_owned();
    let flag = Some("possible_duplicate".to_owned());
    let rows = of_amount(&app, checking, -12000).await;
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!(rows[0], (older, None, manual.clone(), None, None));
    assert_eq!(rows[1], (newer, None, manual, None, None));
    assert_eq!(
        (&rows[2].1, &rows[2].2, &rows[2].3, rows[2].4),
        (
            &Some("TRN-demo-check-1".to_owned()),
            &provider,
            &flag,
            Some(older)
        )
    );
}

#[tokio::test]
async fn a_bank_amount_up_to_a_quarter_larger_is_flagged_a_possible_manual_match() {
    let (server, app, checking) = synced_checking().await;
    // €40.00 entered; the bank sends €46.00 (a tip).
    let id = enter(&app, &entry(checking, 1, -4000)).await;

    ok_sync(&server, &app).await;
    assert_eq!(
        of_amount(&app, checking, -4000).await,
        [(id, None, "manual".to_owned(), None, None)]
    );
    let bank = of_amount(&app, checking, -4600).await;
    assert_eq!(
        (&bank[0].1, &bank[0].3, bank[0].4),
        (
            &Some("TRN-demo-sitter-1".to_owned()),
            &Some("possible_manual_match".to_owned()),
            Some(id)
        )
    );
}

#[tokio::test]
async fn a_manual_row_unmatched_14_days_after_its_date_is_not_seen_at_bank() {
    let (server, app, checking) = synced_checking().await;
    let stale = enter(&app, &entry(checking, 15, -999)).await;
    enter(&app, &entry(checking, 14, -998)).await;

    ok_sync(&server, &app).await;
    let flag = |minor| {
        let app = &app;
        async move { of_amount(app, checking, minor).await[0].3.clone() }
    };
    assert_eq!(flag(-999).await.as_deref(), Some("not_seen_at_bank"));
    assert_eq!(flag(-998).await, None);
    let (_, row) = call(
        &app,
        "GET",
        &format!("/api/v1/transactions/{stale}"),
        &Value::Null,
    )
    .await;
    assert_eq!(row["manual"], true, "kept, never deleted");

    // Dated inside the window again by an edit: the flag goes at the next sync.
    let uri = format!("/api/v1/transactions/{stale}");
    let (status, _) = call(&app, "PUT", &uri, &entry(checking, 5, -999)).await;
    assert_eq!(status, StatusCode::OK);
    ok_sync(&server, &app).await;
    assert_eq!(flag(-999).await, None);
}
