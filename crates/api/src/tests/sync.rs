//! The first sync through the API, against the fake SimpleFIN server: tracking back-fills, Sync
//! now updates what the bank changed and nothing the household owns.

use axum::http::StatusCode;
use base64::Engine;
use serde_json::{Value, json};
use simplefin_fake::{FakeServer, NOW, Scenario};

use super::{App, get, post, send, set_up_with_bank, with};
use crate::{BankConfig, ClaimPolicy, SecretKey};

pub(super) const DAY: i64 = 86_400;

/// An install in EUR, claimed against a fake Bridge whose accounts report EUR. Sync now has no
/// cooldown, so tests can press it again at once; [`claimed_app_with_cooldown`] has one.
pub(super) async fn claimed_app(scenario: Scenario) -> (FakeServer, App) {
    claimed_app_with_cooldown(scenario, jiff::SignedDuration::ZERO).await
}

/// [`claimed_app`], with Sync now waiting `cooldown` after a sync.
pub(super) async fn claimed_app_with_cooldown(
    scenario: Scenario,
    cooldown: jiff::SignedDuration,
) -> (FakeServer, App) {
    let server = FakeServer::start(scenario).await.unwrap();
    server.set_currency("EUR");
    let key = base64::engine::general_purpose::STANDARD.encode([7_u8; 32]);
    let app = set_up_with_bank(BankConfig {
        secret_key: Some(SecretKey::from_base64(&key).unwrap()),
        claim_policy: ClaimPolicy::only_hosts([server.host()]),
        sync_cooldown: cooldown,
    })
    .await;
    let request = axum::http::Request::put("/api/v1/bank-connection")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({ "setup_token": server.setup_token() }).to_string(),
        ))
        .unwrap();
    let (status, body) = send(app.router.clone(), with(&app.cookie, request)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (server, app)
}

pub(super) async fn track(app: &App, id: &str, balances_only: bool) -> i64 {
    let uri = format!("/api/v1/connection-accounts/{id}/track");
    let body = json!({ "role": "budgeted", "balances_only": balances_only });
    let (status, tracked) = send(app.router.clone(), with(&app.cookie, post(&uri, &body))).await;
    assert_eq!(status, StatusCode::OK, "{tracked}");
    tracked["account_id"].as_i64().unwrap()
}

pub(super) async fn sync_now(app: &App) -> (StatusCode, Value) {
    let request = post("/api/v1/bank-connection/sync", &json!({}));
    send(app.router.clone(), with(&app.cookie, request)).await
}

/// The queries of the fake's `GET /accounts` requests after the first `skip`.
fn feed_queries(server: &FakeServer, skip: usize) -> Vec<String> {
    server
        .requests()
        .into_iter()
        .filter(|seen| seen.method == "GET")
        .skip(skip)
        .map(|seen| seen.query)
        .collect()
}

/// `(external id, date, amount, status, source)` of an account's rows, by id.
pub(super) async fn rows(app: &App, account: i64) -> Vec<(String, String, i64, String, String)> {
    sqlx::query_as(
        "SELECT external_id, date, amount_minor, status, source FROM transactions
         WHERE account_id = ? ORDER BY external_id",
    )
    .bind(account)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

/// The UTC date `days` before the fake's now.
pub(super) fn days_ago(days: i64) -> String {
    jiff::Timestamp::from_second(NOW - days * DAY)
        .unwrap()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string()
}

#[tokio::test]
async fn tracking_an_account_back_fills_it_in_two_requests() {
    let (server, app) = claimed_app(Scenario::PendingPosts).await;
    let before = feed_queries(&server, 0).len();
    let checking = track(&app, "ACT-demo-checking", false).await;

    let split = NOW - 45 * DAY;
    assert_eq!(
        feed_queries(&server, before),
        [
            format!("version=2&pending=1&start-date={split}"),
            format!(
                "version=2&pending=1&start-date={}&end-date={split}",
                split - 45 * DAY
            ),
        ]
    );
    let read = rows(&app, checking).await;
    assert_eq!(read.len(), 5, "{read:?}");
    let pending = read
        .iter()
        .find(|r| r.0 == "TRN-demo-pend-1")
        .unwrap()
        .clone();
    assert_eq!(
        pending,
        (
            "TRN-demo-pend-1".to_owned(),
            days_ago(1),
            -2340,
            "pending".to_owned(),
            "provider".to_owned()
        )
    );
    let payroll = read.iter().find(|r| r.0 == "TRN-demo-pay-1").unwrap();
    assert_eq!(
        (payroll.1.as_str(), payroll.2),
        (days_ago(20).as_str(), 240_000)
    );

    // The transactions list shows them, the pending one as pending.
    let uri = format!("/api/v1/transactions?account={checking}");
    let (status, page) = send(app.router.clone(), with(&app.cookie, get(&uri))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let listed = page["transactions"].as_array().unwrap();
    assert_eq!(listed.len(), 5);
    assert!(listed.iter().any(|t| t["status"] == "pending"));

    // The run is on the connection screen.
    let (_, screen) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/bank-connection")),
    )
    .await;
    let last = &screen["connection"]["last_sync"];
    assert_eq!(
        (
            &last["trigger"],
            &last["requests"],
            &last["added"],
            &last["updated"]
        ),
        (&json!("backfill"), &json!(2), &json!(5), &json!(0))
    );
    assert_eq!(last["error"], Value::Null);
    let results: Vec<(i64, i64)> =
        sqlx::query_as("SELECT account_id, ok FROM sync_account_results")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(results, [(checking, 1)]);

    // Tracking another one back-fills it; the first isn't back-filled again but still syncs.
    let before = feed_queries(&server, 0).len();
    let card = track(&app, "ACT-demo-card", false).await;
    assert_eq!(feed_queries(&server, before).len(), 2);
    assert_eq!(rows(&app, card).await.len(), 2);
    assert_eq!(rows(&app, checking).await.len(), 5);
}

#[tokio::test]
async fn sync_now_updates_what_the_bank_changed_and_nothing_the_household_set() {
    let (server, app) = claimed_app(Scenario::PendingPosts).await;
    let checking = track(&app, "ACT-demo-checking", false).await;

    // The household categorizes the pending row, renames its payee, adds a memo and a tag.
    for sql in [
        "INSERT INTO category_groups (id, name) VALUES (90, 'Health')",
        "INSERT INTO categories (id, group_id, name) VALUES (91, 90, 'Pharmacy')",
        "INSERT INTO payees (id, name) VALUES (92, 'Corner Pharmacy')",
        "INSERT INTO tags (id, name) VALUES (93, 'prescriptions')",
        "UPDATE transactions SET category_id = 91, categorized_by = 'user', memo = 'refill',
           payee_id = 92 WHERE external_id = 'TRN-demo-pend-1'",
        "INSERT INTO transaction_tags (transaction_id, tag_id)
           SELECT id, 93 FROM transactions WHERE external_id = 'TRN-demo-pend-1'",
    ] {
        sqlx::query(sql).execute(&app.pool).await.unwrap();
    }

    // Next day the bank posts it under the same id.
    server.next_day();
    let before = feed_queries(&server, 0).len();
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(
        (
            &run["trigger"],
            &run["requests"],
            &run["added"],
            &run["updated"]
        ),
        (&json!("manual"), &json!(1), &json!(0), &json!(1))
    );
    assert_eq!(
        feed_queries(&server, before),
        [format!("version=2&pending=1&start-date={}", NOW - 30 * DAY)]
    );
    let posted = rows(&app, checking)
        .await
        .into_iter()
        .find(|r| r.0 == "TRN-demo-pend-1")
        .unwrap();
    assert_eq!((posted.1, posted.3), (days_ago(1), "posted".to_owned()));
    let household: (i64, String, String, i64, i64) = sqlx::query_as(
        "SELECT category_id, categorized_by, memo, payee_id,
           (SELECT tag_id FROM transaction_tags tt WHERE tt.transaction_id = t.id)
         FROM transactions t WHERE external_id = 'TRN-demo-pend-1'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        household,
        (91, "user".to_owned(), "refill".to_owned(), 92, 93)
    );

    // Nothing changed since: a second Sync now adds and updates nothing.
    let (status, again) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!((&again["added"], &again["updated"]), (&json!(0), &json!(0)));
    assert_eq!(rows(&app, checking).await.len(), 5);
}

#[tokio::test]
async fn a_balances_only_account_gets_no_rows_and_a_deleted_row_stays_deleted() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    let card = track(&app, "ACT-demo-card", true).await;
    assert_eq!(rows(&app, card).await, []);

    let checking = track(&app, "ACT-demo-checking", false).await;
    sqlx::query("DELETE FROM transactions WHERE external_id = 'TRN-demo-rent-1'")
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO deleted_bank_rows (account_id, external_id) VALUES (?, 'TRN-demo-rent-1')",
    )
    .bind(checking)
    .execute(&app.pool)
    .await
    .unwrap();
    let (_, run) = sync_now(&app).await;
    assert_eq!(run["added"], 0, "{run}");
    assert_eq!(rows(&app, checking).await.len(), 3);
    assert_eq!(rows(&app, card).await, []);
    drop(server);
}

#[tokio::test]
async fn an_account_missing_from_the_response_fails_for_the_run_and_keeps_its_rows() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    let savings = track(&app, "ACT-demo-savings", false).await;
    assert_eq!(rows(&app, savings).await.len(), 1);
    server.set_scenario(Scenario::AccountMissing);
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    let result: (i64, String) = sqlx::query_as(
        "SELECT ok, error_code FROM sync_account_results
         WHERE sync_run_id = (SELECT MAX(id) FROM sync_runs)",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(result, (0, "absent".to_owned()));
    assert_eq!(rows(&app, savings).await.len(), 1);
}

#[tokio::test]
async fn a_revoked_access_url_fails_the_run_and_pauses_sync() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    server.set_scenario(Scenario::Revoked);
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert!(run["error"].as_str().unwrap().starts_with("Sync failed"));
    let (_, screen) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/bank-connection")),
    )
    .await;
    assert_eq!(screen["connection"]["status"], "token_revoked");
    let (status, refused) = sync_now(&app).await;
    assert_eq!(
        (status, refused["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("sync_paused"))
    );
}

#[tokio::test]
async fn sync_now_needs_a_connection() {
    let app = set_up_with_bank(BankConfig::default()).await;
    let (status, body) = sync_now(&app).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("not_connected"))
    );
}
