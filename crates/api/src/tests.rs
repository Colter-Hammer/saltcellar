//! The API through its router, the way a client sees it.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use tower::ServiceExt;

struct App {
    router: Router,
    pool: db::SqlitePool,
    /// The setup code, until setup is finished.
    code: Option<String>,
    /// The time the app sees: tests move it on with [`App::wait`].
    now: Arc<Mutex<Timestamp>>,
    /// The first login's session cookie (`session=…`), once setup is finished.
    cookie: String,
    /// The data dir: the database, and uploads waiting for the import.
    dir: tempfile::TempDir,
}

impl App {
    fn wait(&self, duration: SignedDuration) {
        let mut now = self.now.lock().unwrap();
        *now += duration;
    }
}

/// A fresh install, waiting for setup.
async fn app() -> App {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::connect(&dir.path().join(db::FILE_NAME)).await.unwrap();
    db::migrate(&pool).await.unwrap();
    let now = Arc::new(Mutex::new("2026-10-28T09:00:00Z".parse().unwrap()));
    let clock = Arc::clone(&now);
    let state = crate::AppState::load(pool.clone(), dir.path())
        .await
        .unwrap()
        .with_clock(move || *clock.lock().unwrap());
    let code = state.setup_code().map(ToString::to_string);
    App {
        router: crate::router(state),
        pool,
        code,
        now,
        cookie: String::new(),
        dir,
    }
}

/// An install whose setup is finished, by `sam` with the password `correct horse battery`.
async fn set_up() -> App {
    let mut app = app().await;
    let (status, headers, _) = send_full(app.router.clone(), finish(&app, &json!({}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    app.cookie = session_cookie(&headers).unwrap();
    app
}

/// The `session=…` pair from a response's `Set-Cookie`.
fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok()?.split(';').next())
        .find(|pair| pair.starts_with("session="))
        .map(str::to_owned)
}

/// `request`, sent with `cookie`.
fn with(cookie: &str, mut request: Request<Body>) -> Request<Body> {
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().unwrap());
    request
}

async fn send(router: Router, request: Request<Body>) -> (StatusCode, Value) {
    let (status, _, body) = send_full(router, request).await;
    (status, body)
}

async fn send_full(router: Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, headers, body)
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

fn post(uri: &str, body: &Value) -> Request<Body> {
    Request::post(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "localhost:8080")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// `POST /api/v1/setup` with good details, overridden by `changes`.
fn finish(app: &App, changes: &Value) -> Request<Body> {
    let mut body = json!({
        "code": app.code.as_deref().unwrap_or("NOPE-NOPE"),
        "currency": "EUR",
        "timezone": "Europe/Berlin",
        "display_name": "Sam",
        "username": "Sam",
        "password": "correct horse battery",
        "password_again": "correct horse battery",
    });
    for (key, value) in changes.as_object().unwrap() {
        body[key] = value.clone();
    }
    post("/api/v1/setup", &body)
}

fn check_code(code: &str) -> Request<Body> {
    post("/api/v1/setup/code", &json!({ "code": code }))
}

#[tokio::test]
async fn health_is_ok_when_the_database_answers() {
    let app = app().await;
    let (status, body) = send(app.router, get("/api/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
}

#[tokio::test]
async fn health_is_503_when_the_database_doesnt() {
    let app = app().await;
    app.pool.close().await;
    let (status, body) = send(app.router, get("/api/v1/health")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"]["code"], "database_unavailable");
}

#[tokio::test]
async fn unknown_paths_get_the_error_envelope() {
    let app = set_up().await;
    let (status, body) = send(app.router, get("/api/v1/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        json!({ "error": { "code": "not_found", "message": "There's nothing at this address." } })
    );
}

#[tokio::test]
async fn unknown_paths_outside_v1_get_the_error_envelope_too() {
    let app = set_up().await;
    for uri in ["/api", "/api/", "/api/nope", "/api/v2/health"] {
        let (status, body) = send(app.router.clone(), get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(body["error"]["code"], "not_found", "{uri}");
    }
}

#[tokio::test]
async fn wrong_methods_get_the_error_envelope() {
    let app = app().await;
    let request = Request::put("/api/v1/health")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let (status, body) = send(app.router, request).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["error"]["code"], "method_not_allowed");
}

#[tokio::test]
async fn mutating_requests_must_be_json() {
    let app = app().await;
    for (method, content_type) in [
        ("POST", None),
        ("POST", Some("application/x-www-form-urlencoded")),
        ("PUT", Some("multipart/form-data; boundary=x")),
        ("PATCH", Some("text/plain")),
        ("DELETE", None),
    ] {
        let mut request = Request::builder().method(method).uri("/api/v1/health");
        if let Some(content_type) = content_type {
            request = request.header(header::CONTENT_TYPE, content_type);
        }
        let (status, body) =
            send(app.router.clone(), request.body(Body::from("a=1")).unwrap()).await;
        assert_eq!(
            status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{method} {content_type:?}"
        );
        assert_eq!(body["error"]["code"], "json_required");
    }
}

#[tokio::test]
async fn json_mutating_requests_get_through() {
    let app = app().await;
    for content_type in [
        "application/json",
        "application/json; charset=utf-8",
        "Application/JSON",
    ] {
        let request = Request::post("/api/v1/health")
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from("{}"))
            .unwrap();
        let (status, _) = send(app.router.clone(), request).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{content_type}");
    }
}

#[tokio::test]
async fn the_openapi_document_is_served() {
    let app = app().await;
    let (status, body) = send(app.router, get("/api/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::to_value(crate::openapi()).unwrap());
}

// ───────────────────────── Setup ─────────────────────────

#[tokio::test]
async fn before_setup_only_health_and_setup_answer() {
    let app = app().await;
    for uri in ["/api/v1/household", "/api/v1/transactions", "/api/v1/nope"] {
        let (status, body) = send(app.router.clone(), get(uri)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(
            body,
            json!({ "error": {
                "code": "setup_required",
                "message": "Finish setting up the app first.",
            } }),
            "{uri}"
        );
    }
    let (status, _) = send(app.router.clone(), post("/api/v1/household", &json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(app.router.clone(), get("/api/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(app.router.clone(), get("/api/v1/setup")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "done": false }));
    let (status, body) = send(app.router.clone(), get("/api/v1/setup/currencies")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_array().unwrap().contains(&json!("USD")));
    let (status, _) = send(app.router.clone(), get("/api/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn each_start_before_setup_gets_a_new_code() {
    let app = app().await;
    let again = crate::AppState::load(app.pool.clone(), app.dir.path())
        .await
        .unwrap();
    let code = app.code.unwrap();
    assert_eq!(code.len(), 9);
    assert_ne!(again.setup_code().unwrap().to_string(), code);
}

#[tokio::test]
async fn the_setup_code_checks_out_however_it_is_typed() {
    let app = app().await;
    let code = app.code.clone().unwrap();
    for typed in [code.clone(), code.to_lowercase().replace('-', " ")] {
        let (status, body) = send(app.router.clone(), check_code(&typed)).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{typed}");
        assert_eq!(body, Value::Null);
    }
}

#[tokio::test]
async fn a_wrong_code_says_where_to_find_the_right_one() {
    let app = app().await;
    let (status, body) = send(app.router.clone(), check_code("2222-2222")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "setup_code_wrong");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("the app's log")
    );
}

#[tokio::test]
async fn five_wrong_codes_pause_even_the_right_one() {
    let app = app().await;
    for _ in 0..5 {
        let (status, _) = send(app.router.clone(), check_code("2222-2222")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let right = app.code.clone().unwrap();
    let (status, body) = send(app.router.clone(), check_code(&right)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        body["error"],
        json!({
            "code": "too_many_attempts",
            "message": "Too many attempts. Try again in 15 minutes.",
        })
    );
    let (status, _) = send(app.router.clone(), finish(&app, &json!({}))).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn finishing_setup_logs_the_first_login_in() {
    let app = app().await;
    let (status, headers, body) = send_full(app.router.clone(), finish(&app, &json!({}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let cookie = headers[header::SET_COOKIE].to_str().unwrap();
    let token = cookie
        .strip_prefix("session=")
        .and_then(|rest| rest.split(';').next())
        .unwrap();
    assert!(
        cookie.ends_with("; Path=/; Max-Age=2592000; HttpOnly; SameSite=Lax"),
        "{cookie}"
    );

    let stored: String = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(stored, crate::auth::token_hash(token));
    let (username, display_name, hash): (String, String, String) =
        sqlx::query_as("SELECT username, display_name, password_hash FROM users")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!((username.as_str(), display_name.as_str()), ("sam", "Sam"));
    assert!(hash.starts_with("$argon2id$"), "{hash}");

    let cookie = format!("session={token}");
    let (status, body) = send(app.router.clone(), with(&cookie, get("/api/v1/household"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({ "currency": "EUR", "timezone": "Europe/Berlin" })
    );
}

#[tokio::test]
async fn the_session_cookie_is_secure_away_from_localhost() {
    let app = app().await;
    let mut request = finish(&app, &json!({}));
    request
        .headers_mut()
        .insert(header::HOST, "budget.example.test".parse().unwrap());
    let (status, headers, _) = send_full(app.router.clone(), request).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .ends_with("; Secure")
    );
}

#[tokio::test]
async fn after_setup_the_code_and_the_wizard_stop_working() {
    let app = set_up().await;
    let code = app.code.clone().unwrap();

    let (status, body) = send(app.router.clone(), get("/api/v1/setup")).await;
    assert_eq!((status, body), (StatusCode::OK, json!({ "done": true })));
    for request in [
        check_code(&code),
        finish(&app, &json!({ "username": "alex" })),
    ] {
        let (status, body) = send(app.router.clone(), request).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "setup_done");
    }
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(users, 1);

    let restarted = crate::AppState::load(app.pool.clone(), app.dir.path())
        .await
        .unwrap();
    assert!(restarted.setup_code().is_none());
    let (status, _) = send(
        crate::router(restarted),
        with(&app.cookie, get("/api/v1/household")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_short_or_mismatched_password_gets_field_errors() {
    let app = app().await;
    let (status, body) = send(
        app.router.clone(),
        finish(
            &app,
            &json!({ "password": "elevenchars", "password_again": "elevenchars" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"],
        json!({ "password": "Use at least 12 characters." })
    );

    let (status, body) = send(
        app.router.clone(),
        finish(&app, &json!({ "password_again": "correct horse batter" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"],
        json!({ "password_again": "The passwords don't match." })
    );

    let (status, body) = send(app.router.clone(), get("/api/v1/setup")).await;
    assert_eq!((status, body), (StatusCode::OK, json!({ "done": false })));
}

#[tokio::test]
async fn every_wrong_detail_is_named_at_once() {
    let app = app().await;
    let (status, body) = send(
        app.router.clone(),
        finish(
            &app,
            &json!({
                "currency": "XAU",
                "timezone": "Mars/Olympus_Mons",
                "display_name": " ",
                "username": "sam lee",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"],
        json!({
            "code": "validation_failed",
            "message": "Some details need another look.",
            "fields": {
                "currency": "Choose a currency from the list.",
                "timezone": "Choose a timezone from the list.",
                "display_name": "Enter a name.",
                "username": "Use only letters, digits, dots, dashes and underscores.",
            },
        })
    );
}

#[tokio::test]
async fn a_wrong_code_is_checked_before_the_details() {
    let app = app().await;
    let (status, body) = send(
        app.router.clone(),
        finish(&app, &json!({ "code": "2222-2222", "currency": "nope" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "setup_code_wrong");
}

#[tokio::test]
async fn a_body_that_isnt_the_expected_json_gets_the_envelope() {
    let app = app().await;
    for body in ["{", r#"{"code": 5}"#, "{}"] {
        let request = Request::post("/api/v1/setup/code")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let (status, response) = send(app.router.clone(), request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(response["error"]["code"], "invalid_body", "{body}");
    }
}

#[tokio::test]
async fn the_password_never_reaches_the_response_or_the_database_in_clear() {
    let app = set_up().await;
    let dump: Vec<String> = sqlx::query_scalar(
        "SELECT coalesce(password_hash, '') || display_name || username FROM users",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert!(!dump.concat().contains("correct horse"));
}

// ───────────────────────── Login ─────────────────────────

fn log_in(username: &str, password: &str) -> Request<Body> {
    post(
        "/api/v1/session",
        &json!({ "username": username, "password": password }),
    )
}

fn log_out() -> Request<Body> {
    Request::delete("/api/v1/session")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "localhost:8080")
        .body(Body::empty())
        .unwrap()
}

const PASSWORD: &str = "correct horse battery";

#[tokio::test]
async fn logged_out_requests_are_sent_to_log_in() {
    let app = set_up().await;
    for request in [
        get("/api/v1/household"),
        get("/api/v1/session"),
        with("session=nope", get("/api/v1/household")),
        with("theme=dark", get("/api/v1/household")),
    ] {
        let (status, body) = send(app.router.clone(), request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body,
            json!({ "error": { "code": "login_required", "message": "Log in to continue." } })
        );
    }
    // Health, setup and logging in itself need no login.
    for request in [get("/api/v1/health"), get("/api/v1/setup")] {
        let (status, _) = send(app.router.clone(), request).await;
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn logging_in_hands_over_a_session_that_opens_the_api() {
    let app = set_up().await;
    let (status, headers, body) = send_full(app.router.clone(), log_in(" Sam ", PASSWORD)).await;
    assert_eq!((status, body), (StatusCode::NO_CONTENT, Value::Null));
    let cookie = session_cookie(&headers).unwrap();
    assert_ne!(cookie, app.cookie);
    let set_cookie = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(
        set_cookie.ends_with("; Path=/; Max-Age=2592000; HttpOnly; SameSite=Lax"),
        "{set_cookie}"
    );

    let (status, body) = send(app.router.clone(), with(&cookie, get("/api/v1/session"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "username": "sam", "display_name": "Sam" }));
    let (status, _) = send(app.router.clone(), with(&cookie, get("/api/v1/household"))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_cookie_among_others_still_counts() {
    let app = set_up().await;
    let cookies = format!("theme=dark; {}; lang=en", app.cookie);
    let (status, _) = send(app.router.clone(), with(&cookies, get("/api/v1/session"))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_wrong_password_and_an_unknown_username_get_the_same_answer() {
    let app = set_up().await;
    let mut answers = Vec::new();
    for (username, password) in [
        ("sam", "correct horse batter"),
        ("alex", PASSWORD),
        ("not a username", PASSWORD),
        ("sam", ""),
    ] {
        let (status, headers, body) =
            send_full(app.router.clone(), log_in(username, password)).await;
        assert_eq!(session_cookie(&headers), None);
        answers.push((status, body));
    }
    for answer in &answers {
        assert_eq!(
            answer,
            &(
                StatusCode::UNAUTHORIZED,
                json!({ "error": {
                    "code": "login_failed",
                    "message": "Wrong username or password.",
                } })
            )
        );
    }
}

#[tokio::test]
async fn the_sixth_bad_password_is_paused_and_so_is_the_right_one() {
    let app = set_up().await;
    for _ in 0..5 {
        let (status, _) = send(app.router.clone(), log_in("sam", "wrong wrong wrong")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        app.wait(SignedDuration::from_secs(30));
    }
    let paused = json!({ "error": {
        "code": "too_many_attempts",
        "message": "Too many attempts. Try again in 15 minutes.",
    } });
    let (status, body) = send(app.router.clone(), log_in("sam", "wrong wrong wrong")).await;
    assert_eq!(
        (status, body),
        (StatusCode::TOO_MANY_REQUESTS, paused.clone())
    );
    let (status, body) = send(app.router.clone(), log_in("SAM", PASSWORD)).await;
    assert_eq!((status, body), (StatusCode::TOO_MANY_REQUESTS, paused));

    // Another login isn't paused, and the pause ends 15 minutes after the fifth failure.
    let (status, _) = send(app.router.clone(), log_in("alex", PASSWORD)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    app.wait(SignedDuration::from_mins(14));
    let (status, body) = send(app.router.clone(), log_in("sam", PASSWORD)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        body["error"]["message"],
        "Too many attempts. Try again in 1 minute."
    );
    app.wait(SignedDuration::from_secs(30));
    let (status, _) = send(app.router.clone(), log_in("sam", PASSWORD)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn logging_in_forgets_earlier_slips() {
    let app = set_up().await;
    for _ in 0..4 {
        send(app.router.clone(), log_in("sam", "wrong wrong wrong")).await;
    }
    let (status, _) = send(app.router.clone(), log_in("sam", PASSWORD)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(app.router.clone(), log_in("sam", "wrong wrong wrong")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(app.router.clone(), log_in("sam", PASSWORD)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn logging_out_deletes_the_session_and_the_old_cookie_stops_working() {
    let app = set_up().await;
    let (status, headers, _) = send_full(app.router.clone(), with(&app.cookie, log_out())).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cleared = headers[header::SET_COOKIE].to_str().unwrap();
    assert_eq!(
        cleared,
        "session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"
    );

    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
    let (status, body) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/household")),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "login_required");
    let (status, _) = send(app.router.clone(), log_out()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logging_out_leaves_other_sessions_alone() {
    let app = set_up().await;
    let (_, headers, _) = send_full(app.router.clone(), log_in("sam", PASSWORD)).await;
    let phone = session_cookie(&headers).unwrap();
    send(app.router.clone(), with(&app.cookie, log_out())).await;
    let (status, _) = send(app.router.clone(), with(&phone, get("/api/v1/session"))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_session_slides_on_use_and_lapses_after_thirty_idle_days() {
    let app = set_up().await;
    let used = |app: &App| {
        send(
            app.router.clone(),
            with(&app.cookie, get("/api/v1/session")),
        )
    };

    // Used every 29 days, it outlives the 30 days it started with.
    for _ in 0..3 {
        app.wait(SignedDuration::from_hours(29 * 24));
        let (status, headers, _) = send_full(
            app.router.clone(),
            with(&app.cookie, get("/api/v1/session")),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // The cookie's 30 days slide with it.
        assert_eq!(
            session_cookie(&headers).as_deref(),
            Some(app.cookie.as_str())
        );
        assert!(
            headers[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=2592000")
        );
    }
    app.wait(SignedDuration::from_hours(30 * 24));
    let (status, body) = used(&app).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "login_required");
}

#[tokio::test]
async fn before_setup_logging_in_is_setup_required() {
    let app = app().await;
    let (status, body) = send(app.router.clone(), log_in("sam", PASSWORD)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "setup_required");
}

// ───────────────────────── Password reset ─────────────────────────

/// A reset link's token for `username`, issued now by the app's clock, as the CLI does.
async fn issue_reset(app: &App, username: &str) -> String {
    let now = *app.now.lock().unwrap();
    let link = crate::issue_password_reset(&app.pool, username, now)
        .await
        .unwrap();
    let path = link.path();
    path.strip_prefix("/reset-password#").unwrap().to_owned()
}

fn check_reset(token: &str) -> Request<Body> {
    post("/api/v1/password-reset/check", &json!({ "token": token }))
}

fn use_reset(token: &str, password: &str, again: &str) -> Request<Body> {
    post(
        "/api/v1/password-reset",
        &json!({ "token": token, "password": password, "password_again": again }),
    )
}

const NEW_PASSWORD: &str = "a whole new passphrase";

#[tokio::test]
async fn a_reset_link_sets_a_new_password_and_logs_in_once() {
    let app = set_up().await;
    let token = issue_reset(&app, " Sam ").await;
    assert_eq!(token.len(), 64);
    let (status, _) = send(app.router.clone(), check_reset(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, headers, _) = send_full(
        app.router.clone(),
        use_reset(&token, NEW_PASSWORD, NEW_PASSWORD),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = session_cookie(&headers).unwrap();
    let (status, body) = send(app.router.clone(), with(&cookie, get("/api/v1/session"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["username"], "sam");

    // The new password works, the old one doesn't, and the setup session has ended.
    let (status, _) = send(app.router.clone(), log_in("sam", NEW_PASSWORD)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(app.router.clone(), log_in("sam", PASSWORD)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/session")),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Once only.
    let dead = json!({ "error": {
        "code": "reset_link_dead",
        "message": "This reset link has expired or was already used. Ask for a new one.",
    } });
    let (status, body) = send(app.router.clone(), check_reset(&token)).await;
    assert_eq!((status, body), (StatusCode::GONE, dead.clone()));
    let (status, body) = send(
        app.router.clone(),
        use_reset(&token, "yet another passphrase", "yet another passphrase"),
    )
    .await;
    assert_eq!((status, body), (StatusCode::GONE, dead));
}

#[tokio::test]
async fn a_reset_link_is_refused_after_twenty_four_hours() {
    let app = set_up().await;
    let token = issue_reset(&app, "sam").await;
    app.wait(SignedDuration::from_hours(24) - SignedDuration::from_secs(1));
    let (status, _) = send(app.router.clone(), check_reset(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    app.wait(SignedDuration::from_secs(1));
    let (status, body) = send(app.router.clone(), check_reset(&token)).await;
    assert_eq!(status, StatusCode::GONE);
    assert_eq!(body["error"]["code"], "reset_link_dead");
    let (status, _) = send(
        app.router.clone(),
        use_reset(&token, NEW_PASSWORD, NEW_PASSWORD),
    )
    .await;
    assert_eq!(status, StatusCode::GONE);
    let (status, _) = send(app.router.clone(), log_in("sam", PASSWORD)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_new_reset_link_kills_the_older_one() {
    let app = set_up().await;
    let older = issue_reset(&app, "sam").await;
    let newer = issue_reset(&app, "sam").await;
    let (status, _) = send(app.router.clone(), check_reset(&older)).await;
    assert_eq!(status, StatusCode::GONE);
    let (status, _) = send(app.router.clone(), check_reset(&newer)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn an_unknown_login_gets_no_reset_link() {
    let app = set_up().await;
    let now = *app.now.lock().unwrap();
    for name in ["alex", "not a username", ""] {
        let error = crate::issue_password_reset(&app.pool, name, now)
            .await
            .unwrap_err();
        assert!(
            matches!(&error, crate::IssueResetError::UnknownLogin(n) if n == name),
            "{error}"
        );
    }
}

#[tokio::test]
async fn a_reset_link_never_shows_in_debug_output() {
    let app = set_up().await;
    let now = *app.now.lock().unwrap();
    let link = crate::issue_password_reset(&app.pool, "sam", now)
        .await
        .unwrap();
    assert_eq!(format!("{link:?}"), "ResetLink(..)");
}

#[tokio::test]
async fn a_reset_checks_the_new_password_and_keeps_the_link_alive() {
    let app = set_up().await;
    let token = issue_reset(&app, "sam").await;
    let (status, body) = send(
        app.router.clone(),
        use_reset(&token, "elevenchars", "elevenchars"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"],
        json!({ "password": "Use at least 12 characters." })
    );
    let (status, body) = send(
        app.router.clone(),
        use_reset(&token, NEW_PASSWORD, "a whole new passphrasE"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"],
        json!({ "password_again": "The passwords don't match." })
    );
    let (status, _) = send(app.router.clone(), check_reset(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_made_up_token_is_dead_and_the_reset_routes_need_setup() {
    let done = set_up().await;
    let (status, _) = send(done.router.clone(), check_reset("nope")).await;
    assert_eq!(status, StatusCode::GONE);

    let fresh = app().await;
    let (status, body) = send(fresh.router, check_reset("nope")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "setup_required");
}

#[tokio::test]
async fn a_reset_ends_a_throttle_pause() {
    let app = set_up().await;
    for _ in 0..5 {
        send(app.router.clone(), log_in("sam", "wrong wrong wrong")).await;
    }
    let token = issue_reset(&app, "sam").await;
    send(
        app.router.clone(),
        use_reset(&token, NEW_PASSWORD, NEW_PASSWORD),
    )
    .await;
    let (status, _) = send(app.router.clone(), log_in("sam", NEW_PASSWORD)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ───────────────────────── Profile ─────────────────────────

fn put(uri: &str, body: &Value) -> Request<Body> {
    let mut request = post(uri, body);
    *request.method_mut() = axum::http::Method::PUT;
    request
}

fn change_profile(locale: &Value, week_start: &Value) -> Request<Body> {
    put(
        "/api/v1/profile",
        &json!({ "display_name": " Sam Lee ", "display_locale": locale, "week_start": week_start }),
    )
}

fn change_password(current: &str, password: &str, again: &str) -> Request<Body> {
    post(
        "/api/v1/profile/password",
        &json!({ "current_password": current, "password": password, "password_again": again }),
    )
}

fn log_out_everywhere() -> Request<Body> {
    Request::delete("/api/v1/sessions")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "localhost:8080")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn the_profile_starts_out_following_the_browser() {
    let app = set_up().await;
    let (status, body) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/profile")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({ "username": "sam", "display_name": "Sam", "display_locale": null, "week_start": null })
    );
}

#[tokio::test]
async fn the_profile_saves_and_clears_the_locale_and_week_start() {
    let app = set_up().await;
    let (status, body) = send(
        app.router.clone(),
        with(&app.cookie, change_profile(&json!("de-DE"), &json!(1))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({ "username": "sam", "display_name": "Sam Lee", "display_locale": "de-DE", "week_start": 1 })
    );
    let (_, body) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/session")),
    )
    .await;
    assert_eq!(body["display_name"], "Sam Lee");

    let (_, body) = send(
        app.router.clone(),
        with(&app.cookie, change_profile(&Value::Null, &Value::Null)),
    )
    .await;
    assert_eq!(
        (&body["display_locale"], &body["week_start"]),
        (&Value::Null, &Value::Null)
    );
}

#[tokio::test]
async fn the_profile_names_each_field_that_is_wrong() {
    let app = set_up().await;
    let request = put(
        "/api/v1/profile",
        &json!({ "display_name": " ", "display_locale": "en_US", "week_start": 7 }),
    );
    let (status, body) = send(app.router.clone(), with(&app.cookie, request)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"],
        json!({
            "display_name": "Enter a name.",
            "display_locale": "Choose a locale from the list.",
            "week_start": "Choose a day of the week.",
        })
    );
    let (_, body) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/profile")),
    )
    .await;
    assert_eq!(body["display_name"], "Sam");
}

#[tokio::test]
async fn the_profile_needs_a_login() {
    let app = set_up().await;
    for request in [
        get("/api/v1/profile"),
        change_profile(&json!("de-DE"), &json!(1)),
        change_password(PASSWORD, NEW_PASSWORD, NEW_PASSWORD),
        log_out_everywhere(),
    ] {
        let (status, body) = send(app.router.clone(), request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "login_required");
    }
}

#[tokio::test]
async fn changing_the_password_needs_the_current_one() {
    let app = set_up().await;
    let (status, body) = send(
        app.router.clone(),
        with(
            &app.cookie,
            change_password("not my password", NEW_PASSWORD, NEW_PASSWORD),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"],
        json!({ "current_password": "That isn't your current password." })
    );
    let (status, body) = send(
        app.router.clone(),
        with(
            &app.cookie,
            change_password(PASSWORD, NEW_PASSWORD, "something else entirely"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"],
        json!({ "password_again": "The passwords don't match." })
    );

    let (status, _) = send(
        app.router.clone(),
        with(
            &app.cookie,
            change_password(PASSWORD, NEW_PASSWORD, NEW_PASSWORD),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(app.router.clone(), log_in("sam", PASSWORD)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(app.router.clone(), log_in("sam", NEW_PASSWORD)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // This session stays logged in.
    let (status, _) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/session")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn wrong_current_passwords_are_throttled() {
    let app = set_up().await;
    for _ in 0..5 {
        let (status, _) = send(
            app.router.clone(),
            with(
                &app.cookie,
                change_password("not my password", NEW_PASSWORD, NEW_PASSWORD),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let (status, body) = send(
        app.router.clone(),
        with(
            &app.cookie,
            change_password(PASSWORD, NEW_PASSWORD, NEW_PASSWORD),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "too_many_attempts");
}

#[tokio::test]
async fn logging_out_everywhere_ends_every_session_of_the_login() {
    let app = set_up().await;
    let (_, headers, _) = send_full(app.router.clone(), log_in("sam", PASSWORD)).await;
    let phone = session_cookie(&headers).unwrap();

    let (status, headers, _) =
        send_full(app.router.clone(), with(&app.cookie, log_out_everywhere())).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        headers[header::SET_COOKIE],
        "session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"
    );
    for cookie in [&app.cookie, &phone] {
        let (status, _) = send(app.router.clone(), with(cookie, get("/api/v1/session"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

#[test]
fn the_openapi_document_describes_health_and_the_error_envelope() {
    let doc = serde_json::to_value(crate::openapi()).unwrap();
    let health = &doc["paths"]["/api/v1/health"]["get"]["responses"];
    assert_eq!(
        health["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/Health"
    );
    assert_eq!(
        health["503"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ErrorEnvelope"
    );
    let schemas = &doc["components"]["schemas"];
    assert_eq!(schemas["Health"]["required"], json!(["status"]));
    assert_eq!(schemas["ErrorEnvelope"]["required"], json!(["error"]));
    assert_eq!(schemas["ErrorBody"]["required"], json!(["code", "message"]));
}

#[path = "tests/categories.rs"]
mod categories;
mod import;
mod undo;
