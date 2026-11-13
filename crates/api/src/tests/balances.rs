//! Balances through the API, against the fake SimpleFIN server: snapshots from sync runs, the
//! drift flag with its suspect rows, accepting and undoing an adjustment, and the sign rule.

use axum::http::StatusCode;
use serde_json::{Value, json};
use simplefin_fake::Scenario;

use super::sync::{claimed_app, days_ago, sync_now, track};
use super::{App, get, post, send, with};

async fn account(app: &App, id: i64) -> Value {
    let uri = format!("/api/v1/accounts/{id}");
    let (status, body) = send(app.router.clone(), with(&app.cookie, get(&uri))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn snapshot_count(app: &App, id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM balance_snapshots WHERE account_id = ?")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn accept(app: &App, id: i64, body: &Value) -> (StatusCode, Value) {
    let uri = format!("/api/v1/accounts/{id}/balance-adjustments");
    send(app.router.clone(), with(&app.cookie, post(&uri, body))).await
}

async fn undo(app: &App, id: i64) -> (StatusCode, Value) {
    let uri = format!("/api/v1/balance-adjustments/{id}/undo");
    send(
        app.router.clone(),
        with(&app.cookie, post(&uri, &json!({}))),
    )
    .await
}

/// The drift scenario on day 2: the checking account flagged. Resolves to the fake, the app and
/// the account's id.
async fn flagged() -> (simplefin_fake::FakeServer, App, i64) {
    let (server, app) = claimed_app(Scenario::Drift).await;
    let checking = track(&app, "ACT-demo-checking", false).await;

    // The back-fill derives the opening balance from the bank's: no drift.
    let first = account(&app, checking).await;
    assert_eq!(first["opening_balance_minor"], 10_800);
    assert_eq!(
        first["drift"]["bank_comparison"],
        json!({ "as_of": days_ago(2), "bank_minor": 115_758, "app_minor": 115_758, "drift_minor": 0 })
    );
    assert_eq!(first["drift"]["flag"], Value::Null);

    // Day 1: a $45.00 row, and the bank's balance $54.50 lower. Off, but not flagged yet.
    server.next_day();
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    let stepped = account(&app, checking).await;
    assert_eq!(stepped["drift"]["bank_comparison"]["drift_minor"], -950);
    assert_eq!(stepped["drift"]["bank_comparison"]["as_of"], days_ago(1));
    assert_eq!(stepped["drift"]["flag"], Value::Null);
    let (status, refused) = accept(&app, checking, &json!({ "amount_minor": -950 })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "no_drift");

    // Day 2: the second snapshot at the same drift flags it, with the stepping run's row.
    server.next_day();
    sync_now(&app).await;
    let flagged = account(&app, checking).await;
    let hardware: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = 'TRN-demo-hardware-1'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        flagged["drift"]["flag"],
        json!({
            "drift_minor": -950,
            "since": days_ago(1),
            "suspect_rows": [{
                "transaction_id": hardware,
                "change": "added",
                "date": days_ago(1),
                "amount_minor": -4_500,
                "description": "DEMO HARDWARE",
            }],
        })
    );
    assert_eq!(snapshot_count(&app, checking).await, 3);
    (server, app, checking)
}

#[tokio::test]
async fn the_drift_scenario_flags_after_the_second_snapshot() {
    flagged().await;
}

#[tokio::test]
async fn accepting_the_flagged_drift_then_undoing_it_restores_it_and_keeps_the_record() {
    let (_server, app, checking) = flagged().await;

    // Accepting another amount than the flag's is refused; so is an overlong note.
    let (status, refused) = accept(&app, checking, &json!({ "amount_minor": -900 })).await;
    assert_eq!(
        (status, &refused["error"]["code"]),
        (StatusCode::CONFLICT, &json!("no_drift"))
    );
    let long = "x".repeat(501);
    let (status, refused) = accept(
        &app,
        checking,
        &json!({ "amount_minor": -950, "note": long }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert!(refused["error"]["fields"]["note"].is_string());

    // Accept: the flag clears and the app's balance includes the adjustment.
    let (status, accepted) = accept(
        &app,
        checking,
        &json!({ "amount_minor": -950, "note": "  Monthly fee  " }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{accepted}");
    assert_eq!(accepted["drift"]["flag"], Value::Null);
    assert_eq!(accepted["drift"]["bank_comparison"]["drift_minor"], 0);
    assert_eq!(accepted["app_balance_minor"], 110_308);
    let adjustments = accepted["drift"]["adjustments"].as_array().unwrap();
    assert_eq!(adjustments.len(), 1);
    let adjustment = &adjustments[0];
    assert_eq!(
        (
            &adjustment["date"],
            &adjustment["amount_minor"],
            &adjustment["note"],
            &adjustment["accepted_by"],
            &adjustment["undone_at"]
        ),
        (
            &json!(days_ago(0)),
            &json!(-950),
            &json!("Monthly fee"),
            &json!("Sam"),
            &Value::Null
        )
    );
    let id = adjustment["id"].as_i64().unwrap();

    // Undo: the drift and the flag come back, and the adjustment is kept, marked undone.
    let (status, undone) = undo(&app, id).await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(undone["drift"]["flag"]["drift_minor"], -950);
    assert_eq!(undone["drift"]["flag"]["since"], days_ago(1));
    assert_eq!(undone["app_balance_minor"], 110_308 + 950);
    let kept = undone["drift"]["adjustments"].as_array().unwrap();
    assert_eq!(kept.len(), 1);
    assert!(kept[0]["undone_at"].is_string());
    let (status, again) = undo(&app, id).await;
    assert_eq!(
        (status, &again["error"]["code"]),
        (StatusCode::CONFLICT, &json!("already_undone"))
    );
    let (status, missing) = undo(&app, 9_999).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    let (status, missing) = accept(&app, 9_999, &json!({ "amount_minor": -950 })).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");

    // The list carries the flag too.
    let (_, list) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/accounts")),
    )
    .await;
    let listed = list["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == checking)
        .unwrap();
    assert_eq!(listed["drift"]["flag"]["drift_minor"], -950);
}

#[tokio::test]
async fn a_balance_date_that_doesnt_move_writes_no_snapshot() {
    // The steady scenario's `balance-date` stays put on the fake's fixed clock.
    let (_server, app) = claimed_app(Scenario::Steady).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    assert_eq!(snapshot_count(&app, checking).await, 1);
    let run: i64 = sqlx::query_scalar("SELECT sync_run_id FROM balance_snapshots")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sync_now(&app).await;
    sync_now(&app).await;
    assert_eq!(snapshot_count(&app, checking).await, 1);
    let still: i64 = sqlx::query_scalar("SELECT sync_run_id FROM balance_snapshots")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(still, run);
}

#[tokio::test]
async fn a_failed_account_writes_no_snapshot() {
    let (server, app) = claimed_app(Scenario::Drift).await;
    let card = track(&app, "ACT-demo-card", false).await;
    assert_eq!(snapshot_count(&app, card).await, 1);
    // The next day the card fails: its `balance-date` moved, but nothing is written.
    server.next_day();
    server.set_scenario(Scenario::MissingData);
    sync_now(&app).await;
    assert_eq!(snapshot_count(&app, card).await, 1);
}

#[tokio::test]
async fn the_wrong_sign_scenario_is_flipped_and_marked() {
    let (server, app) = claimed_app(Scenario::WrongSign).await;
    let card = track(&app, "ACT-demo-card", false).await;
    let first = account(&app, card).await;
    assert_eq!(first["balance_minor"], -22_900);
    assert_eq!(first["drift"]["sign_corrected"], false);

    // From day 1 the bank says +229.00 while the rows say it owes: flipped, marked, no drift.
    server.next_day();
    sync_now(&app).await;
    let flipped = account(&app, card).await;
    assert_eq!(flipped["balance_minor"], -22_900);
    assert_eq!(flipped["drift"]["sign_corrected"], true);
    assert_eq!(flipped["drift"]["bank_comparison"]["drift_minor"], 0);
    assert_eq!(flipped["drift"]["flag"], Value::Null);
}

#[tokio::test]
async fn a_balances_only_account_gets_snapshots_only() {
    let (server, app) = claimed_app(Scenario::Drift).await;
    let savings = track(&app, "ACT-demo-savings", true).await;
    server.next_day();
    sync_now(&app).await;
    let read = account(&app, savings).await;
    assert_eq!(read["balance_minor"], 500_318);
    assert_eq!(read["opening_balance_minor"], Value::Null);
    assert_eq!(read["drift"]["bank_comparison"], Value::Null);
    assert_eq!(read["drift"]["flag"], Value::Null);
    assert_eq!(snapshot_count(&app, savings).await, 2);
    let apps: Vec<Option<i64>> = sqlx::query_scalar(
        "SELECT app_balance_posted_minor FROM balance_snapshots WHERE account_id = ?",
    )
    .bind(savings)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(apps, [None, None]);
}
