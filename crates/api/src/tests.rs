//! The API through its router, the way a client sees it.

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

struct App {
    router: Router,
    pool: db::SqlitePool,
    /// The setup code, until setup is finished.
    code: Option<String>,
    _dir: tempfile::TempDir,
}

/// A fresh install, waiting for setup.
async fn app() -> App {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::connect(&dir.path().join(db::FILE_NAME)).await.unwrap();
    db::migrate(&pool).await.unwrap();
    let state = crate::AppState::load(pool.clone()).await.unwrap();
    let code = state.setup_code().map(ToString::to_string);
    App {
        router: crate::router(state),
        pool,
        code,
        _dir: dir,
    }
}

/// An install whose setup is finished.
async fn set_up() -> App {
    let app = app().await;
    let (status, _) = send(app.router.clone(), finish(&app, &json!({}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    app
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
    let again = crate::AppState::load(app.pool.clone()).await.unwrap();
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

    let (status, body) = send(app.router.clone(), get("/api/v1/household")).await;
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

    let restarted = crate::AppState::load(app.pool.clone()).await.unwrap();
    assert!(restarted.setup_code().is_none());
    let (status, _) = send(crate::router(restarted), get("/api/v1/household")).await;
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
