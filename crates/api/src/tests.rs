//! The API through its router, the way a client sees it.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

struct App {
    router: Router,
    pool: db::SqlitePool,
    _dir: tempfile::TempDir,
}

async fn app() -> App {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::connect(&dir.path().join(db::FILE_NAME)).await.unwrap();
    db::migrate(&pool).await.unwrap();
    let router = crate::router(crate::AppState { pool: pool.clone() });
    App {
        router,
        pool,
        _dir: dir,
    }
}

async fn send(router: Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
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
    let app = app().await;
    let (status, body) = send(app.router, get("/api/v1/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        json!({ "error": { "code": "not_found", "message": "There's nothing at this address." } })
    );
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
