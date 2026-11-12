//! The household's answers to sync flags, Needs a look and Recently dropped by bank, through the
//! API against the fake SimpleFIN server (docs/spec/m2-bank-data.md#sync-health-in-the-ui and
//! #rows-the-bank-stops-sending): every action clears its flag and does what it says.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use simplefin_fake::{FakeServer, NOW, Scenario};

use super::sync::{DAY, claimed_app, days_ago, sync_now, track};
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

async fn ok_sync(app: &App) {
    let (status, run) = sync_now(app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(run["error"], Value::Null, "{run}");
}

/// Two categories (91 Eating out, 92 Groceries) and a tag (93 #work) to edit rows with.
async fn categories(app: &App) {
    sqlx::query(
        "INSERT INTO category_groups (id, name) VALUES (90, 'Out');
         INSERT INTO categories (id, group_id, name) VALUES (91, 90, 'Eating out');
         INSERT INTO categories (id, group_id, name) VALUES (92, 90, 'Groceries');
         INSERT INTO tags (id, name) VALUES (93, 'work');",
    )
    .execute(&app.pool)
    .await
    .unwrap();
}

/// The household's category, memo and tag on the row with bank id `external_id`.
async fn edit(app: &App, external_id: &str) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "UPDATE transactions SET category_id = 91, categorized_by = 'user', memo = 'team lunch'
         WHERE external_id = ? RETURNING id",
    )
    .bind(external_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO transaction_tags (transaction_id, tag_id) VALUES (?, 93)")
        .bind(id)
        .execute(&app.pool)
        .await
        .unwrap();
    id
}

async fn id_of(app: &App, external_id: &str) -> Option<i64> {
    sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = ?")
        .bind(external_id)
        .fetch_optional(&app.pool)
        .await
        .unwrap()
}

async fn resolve(app: &App, id: i64, action: &str) -> (StatusCode, Value) {
    let uri = format!("/api/v1/transactions/{id}/resolve");
    call(app, "POST", &uri, &json!({ "action": action })).await
}

async fn transaction(app: &App, id: i64) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!("/api/v1/transactions/{id}"),
        &Value::Null,
    )
    .await
}

/// The ids Needs a look lists.
async fn needs_a_look(app: &App) -> Vec<i64> {
    let (status, page) = call(
        app,
        "GET",
        "/api/v1/transactions?needs_a_look=true",
        &Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let rows = page["transactions"].as_array().unwrap();
    assert!(rows.iter().all(|row| row["sync_flag"] != Value::Null));
    let mut ids: Vec<i64> = rows.iter().map(|row| row["id"].as_i64().unwrap()).collect();
    ids.sort_unstable();
    ids
}

/// Two equal rows a day apart, re-issued together: the new ones are possible duplicates of the
/// older stored one.
async fn possible_duplicates() -> (FakeServer, App, i64, i64, i64) {
    let (server, app) = claimed_app(Scenario::IdsReissuedTogether).await;
    track(&app, "ACT-demo-checking", false).await;
    categories(&app).await;
    let older = edit(&app, "TRN-demo-park-1").await;
    server.next_day();
    ok_sync(&app).await;
    let flagged = id_of(&app, "TRN-demo-park-3").await.unwrap();
    let other = id_of(&app, "TRN-demo-park-4").await.unwrap();
    (server, app, older, flagged, other)
}

#[tokio::test]
async fn needs_a_look_lists_exactly_the_flagged_rows() {
    let (_server, app, _older, flagged, other) = possible_duplicates().await;
    assert_eq!(needs_a_look(&app).await, [flagged, other]);
    let (_, row) = transaction(&app, flagged).await;
    assert_eq!(row["sync_flag"], "possible_duplicate");

    resolve(&app, flagged, "keep_both").await;
    assert_eq!(needs_a_look(&app).await, [other]);
    let (status, _) = call(
        &app,
        "GET",
        "/api/v1/transactions?needs_a_look=maybe",
        &Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn merging_a_possible_duplicate_keeps_the_newer_row_with_the_household_s_fields() {
    let (_server, app, older, flagged, other) = possible_duplicates().await;
    let (_, before) = transaction(&app, flagged).await;
    assert_eq!(before["flagged_against"]["id"], older, "{before}");

    let (status, merged) = resolve(&app, flagged, "merge").await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    assert_eq!(merged["id"], flagged);
    assert_eq!(merged["sync_flag"], Value::Null);
    assert_eq!(merged["flagged_against"], Value::Null);
    assert_eq!(merged["category"], "Eating out");
    assert_eq!(merged["memo"], "team lunch");
    assert_eq!(merged["tags"], json!(["work"]));
    assert_eq!(transaction(&app, older).await.0, StatusCode::NOT_FOUND);
    // The merged-away id never comes back.
    let deleted: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM deleted_bank_rows WHERE external_id = 'TRN-demo-park-1'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(deleted, 1);
    // The other row pointed at the deleted one: now it points at the other stored row, and
    // merging it leaves one row for each.
    assert_eq!(needs_a_look(&app).await, [other]);
    let second = id_of(&app, "TRN-demo-park-2").await.unwrap();
    let (_, row) = transaction(&app, other).await;
    assert_eq!(row["flagged_against"]["id"], second, "{row}");
    assert_eq!(resolve(&app, other, "merge").await.0, StatusCode::OK);
    assert_eq!(needs_a_look(&app).await, Vec::<i64>::new());
    assert_eq!(transaction(&app, second).await.0, StatusCode::NOT_FOUND);
    // Asked again, it has no flag to answer.
    assert_eq!(
        resolve(&app, flagged, "merge").await.1["error"]["code"],
        "not_flagged"
    );
}

#[tokio::test]
async fn keeping_both_possible_duplicates_only_clears_the_flag() {
    let (_server, app, older, flagged, _other) = possible_duplicates().await;
    // The action must fit the flag.
    let (status, body) = resolve(&app, flagged, "accept_match").await;
    assert_eq!(
        (status, &body["error"]["code"]),
        (StatusCode::CONFLICT, &json!("not_flagged"))
    );

    let (status, kept) = resolve(&app, flagged, "keep_both").await;
    assert_eq!(status, StatusCode::OK, "{kept}");
    assert_eq!(kept["sync_flag"], Value::Null);
    assert_eq!(kept["category"], Value::Null);
    assert_eq!(transaction(&app, older).await.0, StatusCode::OK);
    assert_eq!(
        resolve(&app, 9999, "keep_both").await.0,
        StatusCode::NOT_FOUND
    );
}

/// A €40.00 manual entry, with a payee, category, memo and tag, and the €46.00 bank row
/// flagged as maybe it.
async fn possible_manual_match() -> (App, i64, i64) {
    let (server, app) = claimed_app(Scenario::ManualRowsArrive).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    categories(&app).await;
    let entry = json!({
        "account_id": checking, "date": days_ago(1), "amount_minor": -4000,
        "payee": "Fake Sitter", "category_id": 92, "memo": "friday", "tags": ["work"],
    });
    let (status, manual) = call(&app, "POST", "/api/v1/transactions", &entry).await;
    assert_eq!(status, StatusCode::CREATED, "{manual}");
    server.next_day();
    ok_sync(&app).await;
    let bank = id_of(&app, "TRN-demo-sitter-1").await.unwrap();
    (app, manual["id"].as_i64().unwrap(), bank)
}

#[tokio::test]
async fn yes_to_a_manual_match_keeps_the_bank_amount_and_the_household_s_fields() {
    let (app, manual, bank) = possible_manual_match().await;
    let (_, before) = transaction(&app, bank).await;
    assert_eq!(before["sync_flag"], "possible_manual_match");
    assert_eq!(before["flagged_against"]["amount_minor"], -4000);

    let (status, merged) = resolve(&app, bank, "accept_match").await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    assert_eq!(merged["amount_minor"], -4600);
    assert_eq!(merged["sync_flag"], Value::Null);
    assert_eq!(merged["payee"], "Fake Sitter");
    assert_eq!(merged["category"], "Groceries");
    assert_eq!(merged["memo"], "friday");
    assert_eq!(merged["tags"], json!(["work"]));
    assert_eq!(merged["manual"], false);
    assert_eq!(transaction(&app, manual).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn no_to_a_manual_match_keeps_both_rows() {
    let (app, manual, bank) = possible_manual_match().await;
    let (status, kept) = resolve(&app, bank, "reject_match").await;
    assert_eq!(status, StatusCode::OK, "{kept}");
    assert_eq!(kept["sync_flag"], Value::Null);
    assert_eq!(kept["amount_minor"], -4600);
    let (_, entry) = transaction(&app, manual).await;
    assert_eq!(entry["manual"], true);
    assert_eq!(needs_a_look(&app).await, Vec::<i64>::new());
}

#[tokio::test]
async fn deleting_a_manual_row_a_bank_row_points_at_unflags_it() {
    let (app, manual, bank) = possible_manual_match().await;
    let uri = format!("/api/v1/transactions/{manual}");
    let (status, body) = call(&app, "DELETE", &uri, &Value::Null).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(transaction(&app, bank).await.1["sync_flag"], Value::Null);
}

/// A posted bank row the bank stopped sending, flagged once unseen for a week.
async fn not_seen() -> (FakeServer, App, i64) {
    let (server, app) = claimed_app(Scenario::PostedDropped).await;
    track(&app, "ACT-demo-checking", false).await;
    server.next_day();
    *app.now.lock().unwrap() = jiff::Timestamp::from_second(NOW + 8 * DAY).unwrap();
    ok_sync(&app).await;
    let gym = id_of(&app, "TRN-demo-gym-1").await.unwrap();
    assert_eq!(needs_a_look(&app).await, [gym]);
    (server, app, gym)
}

#[tokio::test]
async fn keeping_a_row_not_seen_at_the_bank_stops_sync_asking() {
    let (_server, app, gym) = not_seen().await;
    let (status, kept) = resolve(&app, gym, "keep").await;
    assert_eq!(status, StatusCode::OK, "{kept}");
    assert_eq!(kept["sync_flag"], Value::Null);

    *app.now.lock().unwrap() = jiff::Timestamp::from_second(NOW + 9 * DAY).unwrap();
    ok_sync(&app).await;
    assert_eq!(needs_a_look(&app).await, Vec::<i64>::new());
}

#[tokio::test]
async fn deleting_a_row_not_seen_at_the_bank_removes_it_for_good() {
    let (_server, app, gym) = not_seen().await;
    let uri = format!("/api/v1/transactions/{gym}");
    let (status, body) = call(&app, "DELETE", &uri, &Value::Null).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(transaction(&app, gym).await.0, StatusCode::NOT_FOUND);
    assert_eq!(needs_a_look(&app).await, Vec::<i64>::new());
}

#[tokio::test]
async fn keeping_a_manual_row_the_bank_never_sent_stops_sync_asking() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    categories(&app).await;
    let entry = json!({
        "account_id": checking, "date": days_ago(20), "amount_minor": -999,
        "category_id": 91,
    });
    let (_, manual) = call(&app, "POST", "/api/v1/transactions", &entry).await;
    let id = manual["id"].as_i64().unwrap();
    server.next_day();
    ok_sync(&app).await;
    assert_eq!(needs_a_look(&app).await, [id]);

    resolve(&app, id, "keep").await;
    server.next_day();
    ok_sync(&app).await;
    assert_eq!(needs_a_look(&app).await, Vec::<i64>::new());
}

#[tokio::test]
async fn fixing_a_split_whose_amount_changed_clears_its_flag() {
    let (server, app) = claimed_app(Scenario::PendingAmountChanges).await;
    track(&app, "ACT-demo-checking", false).await;
    categories(&app).await;
    let tab = id_of(&app, "TRN-demo-tab-1").await.unwrap();
    let amount: i64 = sqlx::query_scalar("SELECT amount_minor FROM transactions WHERE id = ?")
        .bind(tab)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    // Split to the pending amount; the bank posts it with a tip.
    sqlx::query(
        "INSERT INTO transaction_splits (transaction_id, category_id, amount_minor)
         VALUES (?1, 91, ?2 + 1000), (?1, 92, -1000)",
    )
    .bind(tab)
    .bind(amount)
    .execute(&app.pool)
    .await
    .unwrap();
    server.next_day();
    ok_sync(&app).await;
    let (_, row) = transaction(&app, tab).await;
    assert_eq!(row["sync_flag"], "amount_changed_split", "{row}");
    let posted = row["amount_minor"].as_i64().unwrap();
    assert_ne!(posted, amount);

    let uri = format!("/api/v1/transactions/{tab}/splits");
    let wrong = json!({ "lines": [
        { "category_id": 91, "amount_minor": amount + 1000 },
        { "category_id": 92, "amount_minor": -1000 },
    ]});
    let (status, refused) = call(&app, "PUT", &uri, &wrong).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert!(refused["error"]["fields"]["lines"].is_string());

    let fixed = json!({ "lines": [
        { "category_id": 91, "amount_minor": posted + 1000 },
        { "category_id": 92, "amount_minor": -1000, "memo": "snacks" },
    ]});
    let (status, row) = call(&app, "PUT", &uri, &fixed).await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["sync_flag"], Value::Null);
    assert_eq!(row["splits"][1]["memo"], "snacks");
    assert_eq!(needs_a_look(&app).await, Vec::<i64>::new());

    // One line: no longer split.
    let one = json!({ "lines": [{ "category_id": 92, "amount_minor": posted }] });
    let (_, row) = call(&app, "PUT", &uri, &one).await;
    assert_eq!(
        (row["split"].clone(), row["category"].clone()),
        (json!(false), json!("Groceries"))
    );
    let (status, _) = call(&app, "PUT", &uri, &one).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// The hotel hold, edited by the household, dropped after two syncs without it.
async fn dropped() -> (FakeServer, App, i64) {
    let (server, app) = claimed_app(Scenario::PendingDropped).await;
    track(&app, "ACT-demo-checking", false).await;
    categories(&app).await;
    let hold = edit(&app, "TRN-demo-hold-1").await;
    server.next_day();
    ok_sync(&app).await;
    ok_sync(&app).await;
    assert_eq!(transaction(&app, hold).await.0, StatusCode::NOT_FOUND);
    (server, app, hold)
}

async fn dropped_list(app: &App) -> Vec<Value> {
    let (status, list) = call(app, "GET", "/api/v1/dropped", &Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    list["rows"].as_array().unwrap().clone()
}

#[tokio::test]
async fn restore_brings_a_dropped_row_back_with_its_edits_and_sync_keeps_it() {
    let (_server, app, hold) = dropped().await;
    let listed = dropped_list(&app).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["category"], "Eating out");
    assert_eq!(listed[0]["memo"], "team lunch");

    let uri = format!("/api/v1/dropped/{}/restore", listed[0]["id"]);
    let (status, row) = call(&app, "POST", &uri, &json!({})).await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["id"], hold, "its old id was free");
    assert_eq!(row["status"], "pending");
    assert_eq!(row["category"], "Eating out");
    assert_eq!(row["memo"], "team lunch");
    assert_eq!(row["tags"], json!(["work"]));
    assert_eq!(dropped_list(&app).await, Vec::<Value>::new());
    assert_eq!(
        call(&app, "POST", &uri, &json!({})).await.0,
        StatusCode::NOT_FOUND
    );

    // Still missing at the bank, it isn't dropped again.
    ok_sync(&app).await;
    ok_sync(&app).await;
    assert_eq!(transaction(&app, hold).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_dropped_row_is_gone_after_30_days() {
    let (_server, app, _hold) = dropped().await;
    let id = dropped_list(&app).await[0]["id"].clone();
    // Dropped 30 days and a minute ago (moving the clock on would end the session).
    sqlx::query("UPDATE dropped_transactions SET dropped_at = ?")
        .bind("2026-09-28T08:59:00.000Z")
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(dropped_list(&app).await, Vec::<Value>::new());
    let uri = format!("/api/v1/dropped/{id}/restore");
    let (status, body) = call(&app, "POST", &uri, &json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // The next sync prunes it.
    ok_sync(&app).await;
    let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM dropped_transactions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(kept, 0);
}
