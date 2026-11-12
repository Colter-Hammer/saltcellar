//! Claiming a setup token, replacing it and disconnecting, through the API and against the fake
//! SimpleFIN server.

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use serde_json::{Value, json};
use simplefin_fake::{FakeServer, Scenario};

use super::{App, get, send, set_up_with_bank, with};
use crate::{BankConfig, ClaimPolicy, SecretKey};

const URI: &str = "/api/v1/bank-connection";

fn key(byte: u8) -> SecretKey {
    SecretKey::from_base64(&base64::engine::general_purpose::STANDARD.encode([byte; 32])).unwrap()
}

fn bank(server: &FakeServer, key: Option<SecretKey>) -> BankConfig {
    BankConfig {
        secret_key: key,
        claim_policy: ClaimPolicy::only_hosts([server.host()]),
        ..BankConfig::default()
    }
}

async fn call(router: &Router, cookie: &str, method: &str, body: &Value) -> (StatusCode, Value) {
    let request = if method == "GET" {
        get(URI)
    } else {
        Request::builder()
            .method(method)
            .uri(URI)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    send(router.clone(), with(cookie, request)).await
}

async fn claim(app: &App, token: &str) -> (StatusCode, Value) {
    call(
        &app.router,
        &app.cookie,
        "PUT",
        &json!({ "setup_token": token }),
    )
    .await
}

async fn state(app: &App) -> Value {
    call(&app.router, &app.cookie, "GET", &Value::Null).await.1
}

/// The screen's sync part before any sync, with nothing set.
fn fresh_sync() -> Value {
    json!({
        "time": "06:13",
        "keep_raw_responses": false,
        "request_cap": 12,
        "requests_left": 12,
        "last_synced_at": null,
        "cooldown_until": null,
        "limit_until": null
    })
}

async fn sealed_secret(app: &App) -> Vec<u8> {
    sqlx::query_scalar("SELECT secret_encrypted FROM connections")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn count(app: &App, table: &str) -> i64 {
    let sql = match table {
        "connections" => "SELECT COUNT(*) FROM connections",
        "accounts" => "SELECT COUNT(*) FROM accounts",
        _ => "SELECT COUNT(*) FROM transactions",
    };
    sqlx::query_scalar(sql).fetch_one(&app.pool).await.unwrap()
}

#[tokio::test]
async fn claiming_stores_an_encrypted_secret_and_the_screen_shows_connected() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, Some(key(1)))).await;
    assert_eq!(
        state(&app).await,
        json!({ "secret_key_set": true, "connection": null, "sync": fresh_sync() })
    );

    let (status, body) = claim(&app, &server.setup_token()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({
            "secret_key_set": true,
            "connection": { "label": "SimpleFIN Bridge", "status": "ok", "paused": false, "last_sync": null },
            "sync": fresh_sync()
        })
    );
    assert_eq!(state(&app).await, body);

    // What is stored is sealed: it doesn't hold the URL's login, and the key opens it.
    let sealed = sealed_secret(&app).await;
    assert!(!sealed.windows(9).any(|w| w == b"demo-pass"));
    assert!(!sealed.windows(9).any(|w| w == b"demo-user"));
    let opened = simplefin::open(&key(1), &sealed).unwrap();
    assert!(
        opened
            .expose()
            .starts_with("http://demo-user-1:demo-pass-1@")
    );
    assert_eq!(
        simplefin::open(&key(2), &sealed).unwrap_err(),
        simplefin::OpenError::WrongKey
    );
}

#[tokio::test]
async fn a_malformed_token_is_refused_and_no_request_is_made() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, Some(key(1)))).await;
    let elsewhere =
        base64::engine::general_purpose::STANDARD.encode("https://bridge.example.test/claim/x");
    for token in [
        "",
        "hello there",
        "!!!",
        "http://nowhere.example.test/claim/x",
        &elsewhere,
    ] {
        let (status, body) = claim(&app, token).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{token:?}");
        assert_eq!(body["error"]["code"], "validation_failed");
        assert!(body["error"]["fields"]["setup_token"].is_string());
    }
    assert!(server.requests().is_empty(), "{:?}", server.requests());
    assert_eq!(count(&app, "connections").await, 0);
}

#[tokio::test]
async fn without_a_secret_key_claiming_is_refused_with_a_clear_message() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, None)).await;
    assert_eq!(
        state(&app).await,
        json!({ "secret_key_set": false, "connection": null, "sync": fresh_sync() })
    );
    let (status, body) = claim(&app, &server.setup_token()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "secret_key_missing");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("BUDGET_SECRET_KEY")
    );
    assert_eq!(server.requests(), []);
    assert_eq!(count(&app, "connections").await, 0);
}

/// The app started again on the same database with `config`.
async fn restart(app: &App, config: BankConfig) -> Router {
    let state = crate::AppState::load(app.pool.clone(), app.dir.path())
        .await
        .unwrap()
        .with_bank(config)
        .await
        .unwrap();
    crate::router(state)
}

#[tokio::test]
async fn with_a_different_key_at_startup_the_app_starts_and_shows_paused() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, Some(key(1)))).await;
    claim(&app, &server.setup_token()).await;

    for other in [Some(key(2)), None] {
        let router = restart(&app, bank(&server, other.clone())).await;
        let (status, body) = call(&router, &app.cookie, "GET", &Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["secret_key_set"], other.is_some());
        assert_eq!(
            body["connection"],
            json!({ "label": "SimpleFIN Bridge", "status": "key_mismatch", "paused": true, "last_sync": null })
        );
    }

    // Nothing else is affected, and the right key brings it back.
    let router = restart(&app, bank(&server, Some(key(1)))).await;
    let (_, body) = call(&router, &app.cookie, "GET", &Value::Null).await;
    assert_eq!(body["connection"]["status"], "ok");
    let (status, _) = send(router, with(&app.cookie, get("/api/v1/accounts"))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_new_token_with_the_new_key_clears_the_pause() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, Some(key(1)))).await;
    claim(&app, &server.setup_token()).await;
    let router = restart(&app, bank(&server, Some(key(2)))).await;
    let (status, body) = call(
        &router,
        &app.cookie,
        "PUT",
        &json!({ "setup_token": server.setup_token() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["connection"]["status"], "ok");
    let opened = simplefin::open(&key(2), &sealed_secret(&app).await).unwrap();
    assert!(opened.expose().contains("demo-user-2"));
}

#[tokio::test]
async fn replacing_keeps_the_connection_and_its_accounts() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, Some(key(1)))).await;
    claim(&app, &server.setup_token()).await;
    sqlx::query(
        "INSERT INTO accounts (name, type, role, connection_id, external_account_id)
         SELECT 'Demo Checking', 'checking', 'budgeted', id, 'ACT-demo-checking' FROM connections",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE connections SET status = 'token_revoked', paused_at = '2026-10-28T09:00:00Z'",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let before = sealed_secret(&app).await;
    assert_eq!(state(&app).await["connection"]["status"], "token_revoked");

    let (status, body) = claim(&app, &server.setup_token()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["connection"]["status"], "ok");
    assert_eq!(count(&app, "connections").await, 1);
    assert_eq!(count(&app, "accounts").await, 1);
    let linked: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE connection_id IS NOT NULL")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(linked, 1);
    let after = sealed_secret(&app).await;
    assert_ne!(before, after);
    assert!(
        simplefin::open(&key(1), &after)
            .unwrap()
            .expose()
            .contains("demo-user-2")
    );
}

#[tokio::test]
async fn a_token_the_bridge_refuses_changes_nothing() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, Some(key(1)))).await;
    let token = server.setup_token();
    assert_eq!(claim(&app, &token).await.0, StatusCode::OK);
    let before = sealed_secret(&app).await;

    // A token works once.
    let (status, body) = claim(&app, &token).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "token_refused");
    assert_eq!(sealed_secret(&app).await, before);
    assert_eq!(state(&app).await["connection"]["status"], "ok");

    // A server that isn't there.
    let gone = server.setup_token();
    let config = bank(&server, Some(key(1)));
    drop(server);
    let app = App {
        router: restart(&app, config).await,
        ..app
    };
    let (status, body) = claim(&app, &gone).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"]["code"], "bridge_unavailable");
    assert_eq!(sealed_secret(&app).await, before);
}

#[tokio::test]
async fn disconnecting_deletes_the_secret_and_leaves_accounts_and_rows() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, Some(key(1)))).await;
    claim(&app, &server.setup_token()).await;
    sqlx::query(
        "INSERT INTO accounts (id, name, type, role, connection_id, external_account_id)
         SELECT 1, 'Demo Checking', 'checking', 'budgeted', id, 'ACT-demo-checking' FROM connections",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transactions (account_id, date, amount_minor, raw_description, source, dedupe_key)
         VALUES (1, '2026-10-20', -500, 'DEMO CAFE', 'provider', 'k')",
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, _) = call(&app.router, &app.cookie, "DELETE", &json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        state(&app).await,
        json!({ "secret_key_set": true, "connection": null, "sync": fresh_sync() })
    );
    assert_eq!(count(&app, "connections").await, 0);
    assert_eq!(
        (
            count(&app, "accounts").await,
            count(&app, "transactions").await
        ),
        (1, 1)
    );
    // The account is a manual one now: it takes a manual balance.
    let request = Request::post("/api/v1/accounts/1/balance")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "balance_minor": 100 }).to_string()))
        .unwrap();
    let (status, _) = send(app.router.clone(), with(&app.cookie, request)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // And doing it again is not an error; claiming a fresh token connects again.
    let (status, _) = call(&app.router, &app.cookie, "DELETE", &json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(claim(&app, &server.setup_token()).await.0, StatusCode::OK);
}

#[tokio::test]
async fn the_screen_needs_a_login() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, Some(key(1)))).await;
    let (status, body) = send(app.router.clone(), get(URI)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "login_required");
}

/// A log the test can read.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn the_access_url_is_in_no_response_and_no_log_line() {
    // The one test that installs a subscriber, and for the whole process: a scoped one misses
    // events when other tests register the same callsites at the same time. So the log holds
    // every parallel test's lines too, which only makes the check stricter.
    static LOG: std::sync::OnceLock<Captured> = std::sync::OnceLock::new();
    let log = LOG.get_or_init(Captured::default).clone();
    let writer = log.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish(),
    )
    .unwrap();

    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let app = set_up_with_bank(bank(&server, Some(key(1)))).await;
    let token = server.setup_token();
    let claim_url = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(&token)
            .unwrap(),
    )
    .unwrap();

    // Every kind of response the screen can give.
    let mut responses = vec![state(&app).await.to_string()];
    for pasted in [token.as_str(), token.as_str(), "nonsense", &claim_url] {
        let (_, body) = claim(&app, pasted).await;
        responses.push(body.to_string());
    }
    responses.push(state(&app).await.to_string());
    let (_, body) = call(&app.router, &app.cookie, "DELETE", &json!({})).await;
    responses.push(body.to_string());
    responses.push(state(&app).await.to_string());

    let claimed = {
        // What the Bridge issued to the first claim; it is what must never show.
        let first = FakeServer::start(Scenario::Steady).await.unwrap();
        let again = set_up_with_bank(bank(&first, Some(key(1)))).await;
        claim(&again, &first.setup_token()).await;
        simplefin::open(&key(1), &sealed_secret(&again).await)
            .unwrap()
            .expose()
            .to_owned()
    };
    let login = claimed
        .strip_prefix("http://")
        .and_then(|rest| rest.split_once('@'))
        .map(|(login, _)| login.to_owned())
        .unwrap();
    let (user, pass) = login.split_once(':').unwrap();

    // A back-fill and a Sync now with rows in them: their log lines carry no amount, bank text
    // or payee, and no access URL.
    let (_synced_bridge, synced) = super::sync::claimed_app(Scenario::Steady).await;
    super::sync::track(&synced, "ACT-demo-checking", false).await;
    let (status, run) = super::sync::sync_now(&synced).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    let synced_url = simplefin::open(
        &SecretKey::from_base64(&base64::engine::general_purpose::STANDARD.encode([7_u8; 32]))
            .unwrap(),
        &sealed_secret(&synced).await,
    )
    .unwrap()
    .expose()
    .to_owned();

    let log = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert!(
        log.contains("bank connection claimed"),
        "the log was captured:\n{log}"
    );
    assert!(
        log.contains("bank sync ran") && log.contains("trigger=\"manual\""),
        "the sync's line was captured:\n{log}"
    );
    for row_text in [
        "2400.00",
        "1200.00",
        "86.12",
        "64.30",
        "DEMO EMPLOYER PAYROLL",
        "DEMO GROCER",
        "Demo Employer",
        "Demo Grocer",
        "Demo Properties",
        synced_url.as_str(),
        "/simplefin",
    ] {
        assert!(
            !log.contains(row_text),
            "{row_text:?} leaked into the log:\n{log}"
        );
    }
    for secret in [
        claimed.as_str(),
        login.as_str(),
        user,
        pass,
        "demo-token",
        &claim_url,
    ] {
        for text in responses.iter().map(String::as_str).chain([log.as_str()]) {
            assert!(!text.contains(secret), "{secret:?} leaked into:\n{text}");
        }
    }
    // The key's own bytes never show either.
    let key_text = base64::engine::general_purpose::STANDARD.encode([1u8; 32]);
    assert!(!log.contains(&key_text));
}
