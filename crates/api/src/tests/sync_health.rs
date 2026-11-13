//! Errors, reconnecting a bank, a paused connection and stale data, against the fake SimpleFIN
//! server on a fake clock: each failure scenario produces its state, 403, 402 and `gen.auth`
//! stop scheduled calls until a new setup token, **Done, sync now** clears needs reconnecting,
//! and stale and needs attention follow the bank's `balance-date` at the 2- and 5-day edges.

use axum::http::{Request, StatusCode, header};
use jiff::SignedDuration;
use serde_json::{Value, json};
use simplefin_fake::{FakeServer, Scenario};

use super::sync::{claimed_app, claimed_app_with_cooldown, sync_now, track};
use super::{App, get, post, send, with};

fn mins(n: i64) -> SignedDuration {
    SignedDuration::from_mins(n)
}

fn hours(n: i64) -> SignedDuration {
    SignedDuration::from_hours(n)
}

fn feed_requests(server: &FakeServer) -> usize {
    server
        .requests()
        .iter()
        .filter(|seen| seen.method == "GET")
        .count()
}

async fn screen(app: &App) -> Value {
    let (status, body) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/bank-connection")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// The bank called `name` on the connection screen.
async fn bank(app: &App, name: &str) -> Value {
    let screen = screen(app).await;
    screen["connection"]["banks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|bank| bank["name"] == name)
        .unwrap_or_else(|| panic!("no bank {name} in {screen}"))
        .clone()
}

/// One account as the accounts list gives it.
async fn account(app: &App, id: i64) -> Value {
    let (status, body) = send(
        app.router.clone(),
        with(&app.cookie, get(&format!("/api/v1/accounts/{id}"))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn reconnected(app: &App) -> (StatusCode, Value) {
    let request = post("/api/v1/bank-connection/sync-after-reconnect", &json!({}));
    send(app.router.clone(), with(&app.cookie, request)).await
}

async fn tick(app: &App) {
    crate::schedule::tick(&app.state).await.unwrap();
}

async fn balance(app: &App, id: i64, minor: i64) -> (StatusCode, Value) {
    let request = post(
        &format!("/api/v1/accounts/{id}/balance"),
        &json!({ "balance_minor": minor }),
    );
    send(app.router.clone(), with(&app.cookie, request)).await
}

#[tokio::test]
async fn a_bank_that_needs_auth_needs_reconnecting_and_is_not_retried() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    let savings = track(&app, "ACT-demo-savings", false).await;
    assert_eq!(bank(&app, "Demo Credit Union").await["state"], "ok");

    server.set_scenario(Scenario::NeedsAuth);
    app.wait(mins(31));
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");

    let credit_union = bank(&app, "Demo Credit Union").await;
    assert_eq!(credit_union["state"], "needs_reconnecting");
    assert_eq!(credit_union["fix"], true);
    assert_eq!(credit_union["accounts"], 1);
    assert_eq!(
        credit_union["message"],
        "Demo Credit Union needs you to sign in again."
    );
    let first = bank(&app, "First Demo Bank").await;
    assert_eq!(
        (&first["state"], &first["fix"]),
        (&json!("ok"), &json!(false))
    );

    let sync = &account(&app, savings).await["sync"];
    assert_eq!(sync["state"], "needs_reconnecting");
    assert_eq!(sync["fix"], true);
    assert_eq!(
        sync["message"],
        "Demo Credit Union needs you to sign in again."
    );
    // The sync itself went through: the connection isn't paused.
    assert_eq!(screen(&app).await["connection"]["paused"], false);

    // Not retried, scheduled or otherwise: the next morning's run (06:13 in Berlin) is the only
    // call.
    let before = feed_requests(&server);
    app.wait(hours(19) + mins(50));
    tick(&app).await;
    assert_eq!(feed_requests(&server), before + 1);
    assert_eq!(app.state.retry_due(), None);
}

#[tokio::test]
async fn done_sync_now_skips_the_cooldown_and_clears_needs_reconnecting() {
    let (server, app) = claimed_app_with_cooldown(Scenario::Steady, mins(30)).await;
    let savings = track(&app, "ACT-demo-savings", false).await;
    server.set_scenario(Scenario::NeedsAuth);
    app.wait(mins(31));
    sync_now(&app).await;
    assert_eq!(
        bank(&app, "Demo Credit Union").await["state"],
        "needs_reconnecting"
    );

    // Fixed at SimpleFIN a minute later: Sync now still waits, Done, sync now doesn't.
    server.set_scenario(Scenario::Steady);
    app.wait(mins(1));
    let (status, refused) = sync_now(&app).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "sync_cooling_down");
    let before = feed_requests(&server);
    let left_before = screen(&app).await["sync"]["requests_left"]
        .as_i64()
        .unwrap();

    let (status, run) = reconnected(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(run["trigger"], "manual");
    assert_eq!(feed_requests(&server), before + 1);
    assert_eq!(bank(&app, "Demo Credit Union").await["state"], "ok");
    assert_eq!(account(&app, savings).await["sync"]["state"], "ok");
    // It counts against the cap.
    assert_eq!(
        screen(&app).await["sync"]["requests_left"]
            .as_i64()
            .unwrap(),
        left_before - 1
    );
}

#[tokio::test]
async fn done_sync_now_still_stops_at_the_request_cap() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await; // the back-fill's two requests
    for _ in 0..9 {
        app.wait(mins(31));
        assert_eq!(sync_now(&app).await.0, StatusCode::OK);
    }
    app.wait(mins(31));
    let (status, refused) = reconnected(&app).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "sync_limit_reached");
}

#[tokio::test]
async fn done_sync_now_on_a_paused_connection_is_refused() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    server.set_scenario(Scenario::Revoked);
    app.wait(mins(31));
    sync_now(&app).await;
    let before = feed_requests(&server);
    let (status, refused) = reconnected(&app).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "sync_paused");
    assert_eq!(feed_requests(&server), before);
}

/// Runs the scenario that pauses, checks the pause and that nothing asks the Bridge again, then
/// replaces the setup token.
async fn pauses_until_a_new_token(scenario: Scenario, status_name: &str, code: &str) {
    let (server, app) = claimed_app(Scenario::Steady).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    let rows_before = super::sync::rows(&app, checking).await;

    server.set_scenario(scenario);
    app.wait(mins(31));
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert!(run["error"].is_string(), "{run}");
    let paused = screen(&app).await;
    assert_eq!(paused["connection"]["status"], status_name, "{paused}");
    assert_eq!(paused["connection"]["paused"], true);
    let sqlite_code: String =
        sqlx::query_scalar("SELECT error_code FROM sync_runs ORDER BY id DESC LIMIT 1")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(sqlite_code, code);
    assert_eq!(super::sync::rows(&app, checking).await, rows_before);

    // A fake clock through three days of scheduled slots and a restart's catch-up: nothing asks.
    let before = feed_requests(&server);
    for _ in 0..3 {
        app.wait(hours(24));
        tick(&app).await;
    }
    crate::schedule::plan_catch_up(&app.state).await.unwrap();
    assert_eq!(app.state.catch_up_due(), None);
    assert_eq!(feed_requests(&server), before);
    // Nor does a person pressing Sync now.
    let (status, refused) = sync_now(&app).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "sync_paused");
    assert_eq!(feed_requests(&server), before);

    // Replace setup token… clears the pause, keeping the accounts.
    server.set_scenario(Scenario::Steady);
    let request = Request::put("/api/v1/bank-connection")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({ "setup_token": server.setup_token() }).to_string(),
        ))
        .unwrap();
    let (status, body) = send(app.router.clone(), with(&app.cookie, request)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["connection"]["status"], "ok");
    assert_eq!(body["connection"]["paused"], false);
    app.wait(mins(31));
    assert_eq!(sync_now(&app).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_403_pauses_the_connection_until_a_new_token() {
    pauses_until_a_new_token(Scenario::Revoked, "token_revoked", "403").await;
}

#[tokio::test]
async fn a_402_pauses_the_connection_as_unpaid() {
    pauses_until_a_new_token(Scenario::Unpaid, "unpaid", "402").await;
}

#[tokio::test]
async fn gen_auth_in_the_error_list_pauses_like_a_403() {
    pauses_until_a_new_token(Scenario::GenAuth, "token_revoked", "gen.auth").await;
}

#[tokio::test]
async fn a_scheduled_run_that_meets_a_403_pauses_and_the_next_day_asks_nothing() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    server.set_scenario(Scenario::Revoked);
    // 06:13 in Berlin is 05:13 UTC: the next morning's slot.
    app.wait(hours(21));
    let before = feed_requests(&server);
    tick(&app).await;
    assert_eq!(feed_requests(&server), before + 1);
    assert_eq!(screen(&app).await["connection"]["status"], "token_revoked");
    assert_eq!(app.state.retry_due(), None);
    app.wait(hours(24));
    tick(&app).await;
    assert_eq!(feed_requests(&server), before + 1);
}

#[tokio::test]
async fn other_bank_link_errors_and_failed_reads_need_attention_and_are_retried() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    let savings = track(&app, "ACT-demo-savings", false).await;
    server.set_scenario(Scenario::ConError);
    app.wait(mins(31));
    sync_now(&app).await;

    let sync = &account(&app, savings).await["sync"];
    assert_eq!(sync["state"], "needs_attention");
    assert_eq!(
        sync["message"],
        "Demo Credit Union is having trouble right now."
    );
    // The bank link isn't re-authorizable by the household: no Fix at SimpleFIN.
    assert_eq!(sync["fix"], false);
    let credit_union = bank(&app, "Demo Credit Union").await;
    assert_eq!(credit_union["state"], "needs_attention");
    assert_eq!(credit_union["fix"], false);

    // A scheduled run that meets it (06:13 in Berlin the next morning) plans a retry, as for any
    // transient failure.
    app.wait(hours(19) + mins(50));
    tick(&app).await;
    assert!(app.state.retry_due().is_some());
}

#[tokio::test]
async fn a_developer_notice_is_logged_and_shows_nothing() {
    let (server, app) = claimed_app(Scenario::GenApi).await;
    track(&app, "ACT-demo-checking", false).await;
    track(&app, "ACT-demo-savings", false).await;
    app.wait(mins(31));
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(run["error"], Value::Null);
    let screen = screen(&app).await;
    assert_eq!(screen["connection"]["paused"], false);
    for bank in screen["connection"]["banks"].as_array().unwrap() {
        assert_eq!(bank["state"], "ok", "{bank}");
    }
    assert!(feed_requests(&server) > 0);
}

#[tokio::test]
async fn data_goes_stale_after_two_days_and_needs_attention_after_five() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    let savings = track(&app, "ACT-demo-savings", false).await;
    // The bank refreshed an hour before the sync that read it. Moving the clock without another
    // sync ages that date: the edges are exactly 48 and 120 hours.
    let sync_of = |app: &App| {
        let app_router = app.router.clone();
        let cookie = app.cookie.clone();
        async move {
            let (_, body) = send(
                app_router,
                with(&cookie, get(&format!("/api/v1/accounts/{savings}"))),
            )
            .await;
            body["sync"].clone()
        }
    };
    assert_eq!(sync_of(&app).await["state"], "ok");

    app.wait(hours(47)); // 48 hours old
    assert_eq!(sync_of(&app).await["state"], "ok");
    app.wait(SignedDuration::from_secs(1));
    let stale = sync_of(&app).await;
    assert_eq!(stale["state"], "stale");
    assert_eq!(stale["days"], 2);
    assert_eq!(stale["message"], "Data is 2 days old.");
    assert_eq!(stale["fix"], false);
    let bank = bank(&app, "Demo Credit Union").await;
    assert_eq!(bank["state"], "stale");
    assert_eq!(bank["message"], "Bank hasn't updated in 2 days.");

    app.wait(hours(72) - SignedDuration::from_secs(1)); // 120 hours old
    let still_stale = sync_of(&app).await;
    assert_eq!(
        (&still_stale["state"], &still_stale["days"]),
        (&json!("stale"), &json!(5))
    );
    app.wait(SignedDuration::from_secs(1));
    let attention = sync_of(&app).await;
    assert_eq!(attention["state"], "needs_attention");
    assert_eq!(attention["days"], 5);
    assert_eq!(attention["fix"], true);
    assert_eq!(attention["message"], "Data is 5 days old.");
}

#[tokio::test]
async fn data_the_bank_sends_three_days_old_is_stale_and_six_needs_attention() {
    let (server, app) = claimed_app(Scenario::StaleData).await;
    track(&app, "ACT-demo-checking", false).await;
    let savings = track(&app, "ACT-demo-savings", false).await;
    let sync = &account(&app, savings).await["sync"];
    assert_eq!(
        (&sync["state"], &sync["days"]),
        (&json!("stale"), &json!(3))
    );
    assert_eq!(bank(&app, "First Demo Bank").await["state"], "ok");

    server.set_scenario(Scenario::NeedsAuth);
    app.wait(mins(31));
    sync_now(&app).await;
    // con.auth wins over the age.
    assert_eq!(
        account(&app, savings).await["sync"]["state"],
        "needs_reconnecting"
    );
}

#[tokio::test]
async fn a_tracked_account_missing_over_five_days_needs_attention() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    let savings = track(&app, "ACT-demo-savings", false).await;
    server.set_scenario(Scenario::AccountMissing);
    app.wait(mins(31));
    sync_now(&app).await;
    // Missing never means closed: quiet at first.
    assert_eq!(account(&app, savings).await["sync"]["state"], "ok");

    app.wait(hours(6 * 24));
    sync_now(&app).await;
    let sync = &account(&app, savings).await["sync"];
    assert_eq!(sync["state"], "needs_attention");
    assert_eq!(
        sync["message"],
        "The bank hasn't sent this account in 6 days."
    );
}

#[tokio::test]
async fn a_balances_only_account_at_a_broken_link_takes_a_manual_balance() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    let savings = track(&app, "ACT-demo-savings", true).await;
    assert_eq!(account(&app, savings).await["balance_entry"], "bank");
    let (status, refused) = balance(&app, savings, 100_000).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "account_connected");

    server.set_scenario(Scenario::NeedsAuth);
    app.wait(mins(31));
    sync_now(&app).await;
    let broken = account(&app, savings).await;
    assert_eq!(broken["balance_entry"], "manual");
    assert_eq!(
        balance(&app, savings, 100_000).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(account(&app, savings).await["balance_minor"], 100_000);
    // An account with rows keeps its balance from the bank, and so does one at a working link.
    assert_eq!(account(&app, checking).await["balance_entry"], "bank");
    assert_eq!(balance(&app, checking, 1).await.0, StatusCode::CONFLICT);

    // Fixed: the bank sends it again.
    server.set_scenario(Scenario::Steady);
    reconnected(&app).await;
    assert_eq!(account(&app, savings).await["balance_entry"], "bank");
}

#[tokio::test]
async fn a_paused_connection_lets_a_balances_only_account_take_a_manual_balance() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    let savings = track(&app, "ACT-demo-savings", true).await;
    server.set_scenario(Scenario::Revoked);
    app.wait(mins(31));
    sync_now(&app).await;
    assert_eq!(account(&app, savings).await["balance_entry"], "manual");
    assert_eq!(
        balance(&app, savings, 250_000).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn the_banks_list_says_balances_only_and_counts_tracked_accounts() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    track(&app, "ACT-demo-card", false).await;
    track(&app, "ACT-demo-savings", true).await;
    let first = bank(&app, "First Demo Bank").await;
    assert_eq!(
        (&first["state"], &first["accounts"]),
        (&json!("ok"), &json!(2))
    );
    assert!(first["last_refreshed_at"].is_string());
    assert_eq!(
        bank(&app, "Demo Credit Union").await["state"],
        "balances_only"
    );
}
