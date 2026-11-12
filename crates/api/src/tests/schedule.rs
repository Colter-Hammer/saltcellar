//! The sync schedule on a fake clock, against the fake SimpleFIN server: the request cap with
//! the request held back for the scheduled run, the Sync now cooldown, retries, the catch-up
//! after a restart, single-flight, the run history and the kept raw responses. The household is
//! in Europe/Berlin (UTC+1 in late October) and the clock starts at 09:00 UTC.

use axum::http::{Request, StatusCode, header};
use base64::Engine;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use simplefin_fake::{FakeServer, Scenario};

use super::sync::{claimed_app, claimed_app_with_cooldown, sync_now, track};
use super::{App, get, send, with};
use crate::SecretKey;

fn mins(n: i64) -> SignedDuration {
    SignedDuration::from_mins(n)
}

/// How many `GET /accounts` the fake has answered.
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

async fn set_sync(app: &App, time: &str, keep_raw: bool) -> (StatusCode, Value) {
    let request = Request::put("/api/v1/bank-connection/sync-settings")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({ "sync_time": time, "keep_raw_responses": keep_raw }).to_string(),
        ))
        .unwrap();
    send(app.router.clone(), with(&app.cookie, request)).await
}

async fn tick(app: &App) {
    crate::schedule::tick(&app.state).await.unwrap();
}

/// `(trigger, requests)` of every run, oldest first.
async fn runs(app: &App) -> Vec<(String, i64)> {
    sqlx::query_as("SELECT trigger, requests FROM sync_runs ORDER BY id")
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

fn now(app: &App) -> Timestamp {
    *app.now.lock().unwrap()
}

#[tokio::test]
async fn the_cap_holds_one_request_back_for_the_scheduled_run() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await; // the back-fill: 2 requests at 09:00
    assert_eq!(screen(&app).await["sync"]["requests_left"], 10);
    for _ in 0..9 {
        app.wait(mins(31));
        let (status, run) = sync_now(&app).await;
        assert_eq!(status, StatusCode::OK, "{run}");
    }
    let state = screen(&app).await;
    assert_eq!(
        (&state["sync"]["requests_left"], &state["sync"]["request_cap"]),
        (&json!(1), &json!(12))
    );
    // The last request is the scheduled run's: the back-fill's two leave the window at 09:00
    // tomorrow, 10:00 in Berlin.
    assert_eq!(state["sync"]["limit_until"], "2026-10-29T09:00:00Z");
    let before = feed_requests(&server);
    app.wait(mins(31));
    let (status, refused) = sync_now(&app).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "sync_limit_reached");
    assert_eq!(
        refused["error"]["message"],
        "Sync limit reached, try again after 10:00 am."
    );
    assert_eq!(feed_requests(&server), before);

    // It is 14:10 UTC, 15:10 in Berlin. The scheduled run at 15:20 takes the held request.
    assert_eq!(now(&app), "2026-10-28T14:10:00Z".parse().unwrap());
    let (status, body) = set_sync(&app, "15:20", false).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sync"]["time"], "15:20");
    tick(&app).await;
    app.wait(mins(9));
    tick(&app).await;
    assert_eq!(feed_requests(&server), before);
    app.wait(mins(1));
    tick(&app).await;
    assert_eq!(feed_requests(&server), before + 1);
    assert_eq!(
        runs(&app).await.last().unwrap(),
        &("scheduled".to_owned(), 1)
    );
    assert_eq!(screen(&app).await["sync"]["requests_left"], 0);
    // Once a day.
    app.wait(mins(5));
    tick(&app).await;
    assert_eq!(feed_requests(&server), before + 1);
}

#[tokio::test]
async fn sync_now_cools_down_for_thirty_minutes_after_a_sync() {
    let (server, app) = claimed_app_with_cooldown(Scenario::Steady, mins(30)).await;
    track(&app, "ACT-demo-checking", false).await;
    // The back-fill doesn't count: Sync now goes at once.
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    app.wait(mins(12));
    let before = feed_requests(&server);
    let (status, refused) = sync_now(&app).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(
        (&refused["error"]["code"], &refused["error"]["message"]),
        (
            &json!("sync_cooling_down"),
            &json!("Synced 12 min ago. Sync now works again in 18 min.")
        )
    );
    assert_eq!(feed_requests(&server), before);
    let state = screen(&app).await;
    assert_eq!(
        (
            &state["sync"]["last_synced_at"],
            &state["sync"]["cooldown_until"]
        ),
        (
            &json!("2026-10-28T09:00:00Z"),
            &json!("2026-10-28T09:30:00Z")
        )
    );
    app.wait(mins(18));
    assert_eq!(screen(&app).await["sync"]["cooldown_until"], Value::Null);
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
}

#[tokio::test]
async fn two_sync_now_presses_at_once_make_one_request() {
    // With the cooldown, a press that came after the run would be refused, not joined.
    let (server, app) = claimed_app_with_cooldown(Scenario::Steady, mins(30)).await;
    track(&app, "ACT-demo-checking", false).await;
    let before = feed_requests(&server);
    let ((first, a), (second, b)) = tokio::join!(sync_now(&app), sync_now(&app));
    assert_eq!((first, second), (StatusCode::OK, StatusCode::OK), "{a} {b}");
    assert_eq!(a["id"], b["id"]);
    assert_eq!(feed_requests(&server), before + 1);
}

#[tokio::test]
async fn a_transient_failure_is_retried_at_twenty_minutes_and_two_hours() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    track(&app, "ACT-demo-card", false).await;
    server.set_scenario(Scenario::MissingData); // the card: act.missingdata
    // 09:00 UTC is 10:00 in Berlin; the scheduled run is at 10:05.
    set_sync(&app, "10:05", false).await;
    app.wait(mins(5));
    tick(&app).await;
    assert_eq!(
        runs(&app).await.last().unwrap(),
        &("scheduled".to_owned(), 1)
    );
    let failed: Timestamp = "2026-10-28T09:05:00Z".parse().unwrap();
    assert_eq!(app.state.retry_due(), Some(failed + mins(20)));

    let before = feed_requests(&server);
    app.wait(mins(19));
    tick(&app).await;
    assert_eq!(feed_requests(&server), before);
    app.wait(mins(1));
    tick(&app).await;
    assert_eq!(feed_requests(&server), before + 1);
    assert_eq!(runs(&app).await.last().unwrap(), &("retry".to_owned(), 1));
    // Still failing: the second retry is two hours after the failure.
    assert_eq!(app.state.retry_due(), Some(failed + mins(120)));
    app.wait(mins(99));
    tick(&app).await;
    assert_eq!(feed_requests(&server), before + 1);
    app.wait(mins(1));
    tick(&app).await;
    assert_eq!(feed_requests(&server), before + 2);
    // Then it waits for the next day.
    assert_eq!(app.state.retry_due(), None);
    // Retries count against the cap: two back-fills of 2, a run and two retries.
    assert_eq!(screen(&app).await["sync"]["requests_left"], 12 - 4 - 3);
}

#[tokio::test]
async fn a_retry_that_works_ends_the_retries_and_other_failures_get_none() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    track(&app, "ACT-demo-card", false).await;
    server.set_scenario(Scenario::MissingData);
    set_sync(&app, "10:05", false).await;
    app.wait(mins(5));
    tick(&app).await;
    server.set_scenario(Scenario::Steady);
    app.wait(mins(20));
    tick(&app).await;
    assert_eq!(runs(&app).await.last().unwrap(), &("retry".to_owned(), 1));
    assert_eq!(app.state.retry_due(), None);

    // A revoked access URL is never retried.
    server.set_scenario(Scenario::Revoked);
    app.wait(SignedDuration::from_hours(24) - mins(20));
    tick(&app).await;
    assert_eq!(
        runs(&app).await.last().unwrap(),
        &("scheduled".to_owned(), 1)
    );
    assert_eq!(app.state.retry_due(), None);
}

/// The same install started again, by the same clock.
async fn restart(app: &App) -> crate::AppState {
    let clock = std::sync::Arc::clone(&app.now);
    crate::AppState::load(app.pool.clone(), app.dir.path())
        .await
        .unwrap()
        .with_bank((*app.state.bank).clone())
        .await
        .unwrap()
        .with_clock(move || *clock.lock().unwrap())
}

#[tokio::test]
async fn a_restart_catches_up_past_twenty_hours_and_not_before() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await; // a successful sync at 09:00
    // Out of the scheduled run's way: 23:41 in Berlin.
    set_sync(&app, "23:41", false).await;

    app.wait(SignedDuration::from_hours(20));
    let started = restart(&app).await;
    crate::schedule::plan_catch_up(&started).await.unwrap();
    assert_eq!(started.catch_up_due(), None);

    app.wait(mins(1));
    let started = restart(&app).await;
    crate::schedule::plan_catch_up(&started).await.unwrap();
    let boot = now(&app);
    assert_eq!(started.catch_up_due(), Some(boot + mins(3)));
    let before = feed_requests(&server);
    app.wait(mins(2));
    crate::schedule::tick(&started).await.unwrap();
    assert_eq!(feed_requests(&server), before);
    app.wait(mins(1));
    crate::schedule::tick(&started).await.unwrap();
    assert_eq!(feed_requests(&server), before + 1);
    assert_eq!(
        runs(&app).await.last().unwrap(),
        &("catch_up".to_owned(), 1)
    );
    app.wait(mins(5));
    crate::schedule::tick(&started).await.unwrap();
    assert_eq!(feed_requests(&server), before + 1);
}

#[tokio::test]
async fn the_history_lists_each_run_with_its_accounts() {
    let (server, app) = claimed_app(Scenario::Steady).await;
    let checking = track(&app, "ACT-demo-checking", false).await;
    let card = track(&app, "ACT-demo-card", false).await;
    server.set_scenario(Scenario::MissingData);
    app.wait(mins(31));
    sync_now(&app).await;
    let (status, history) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/bank-connection/sync-runs")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let listed: Vec<(&str, i64)> = history["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["run"]["trigger"].as_str().unwrap(),
                r["run"]["requests"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(listed, [("manual", 1), ("backfill", 2), ("backfill", 2)]);
    let accounts = &history["runs"][0]["accounts"];
    assert_eq!(accounts.as_array().unwrap().len(), 2);
    let result = |id: i64| {
        accounts
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["account_id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(
        (result(checking)["ok"].clone(), result(checking)["error_code"].clone()),
        (json!(true), Value::Null)
    );
    assert_eq!(
        (result(card)["ok"].clone(), result(card)["error_code"].clone()),
        (json!(false), json!("act.missingdata"))
    );
    assert!(result(card)["error"].is_string());
}

async fn raw_responses(app: &App) -> Vec<Vec<u8>> {
    sqlx::query_scalar("SELECT body FROM sync_raw_responses ORDER BY received_at, seq")
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn raw_responses_are_kept_only_when_asked_sealed_and_for_seven_days() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    track(&app, "ACT-demo-checking", false).await;
    assert_eq!(raw_responses(&app).await.len(), 0);
    let (status, body) = set_sync(&app, "06:13", true).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sync"]["keep_raw_responses"], true);

    app.wait(mins(31));
    sync_now(&app).await;
    let kept = raw_responses(&app).await;
    assert_eq!(kept.len(), 1);
    let key = SecretKey::from_base64(&base64::engine::general_purpose::STANDARD.encode([7_u8; 32]))
        .unwrap();
    for clear in [&b"DEMO GROCER"[..], b"2400.00", b"ACT-demo-checking"] {
        assert!(!kept[0].windows(clear.len()).any(|w| w == clear));
    }
    let opened = String::from_utf8(simplefin::open_raw_response(&key, &kept[0]).unwrap()).unwrap();
    assert!(opened.contains("DEMO GROCER"), "{opened}");

    // A week later the first is pruned.
    app.wait(SignedDuration::from_hours(7 * 24) + mins(1));
    sync_now(&app).await;
    let later = raw_responses(&app).await;
    assert_eq!(later.len(), 1);
    assert_ne!(later[0], kept[0]);

    // Turning it off deletes them and keeps no more.
    set_sync(&app, "06:13", false).await;
    app.wait(mins(31));
    sync_now(&app).await;
    assert_eq!(raw_responses(&app).await.len(), 0);
}

#[tokio::test]
async fn a_sync_time_must_be_hh_mm() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    assert_eq!(screen(&app).await["sync"]["time"], "06:13");
    let (status, body) = set_sync(&app, "6 am", false).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["fields"]["sync_time"], "Enter a time like 06:13.");
}
