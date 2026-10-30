//! Import from Actual, step 1, through the API: uploading the fake exports in `fixtures/`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};

use super::{App, app, finish, send, send_full, session_cookie, set_up, with};

const UPLOAD: &str = "/api/v1/imports/actual";

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    std::fs::read(&path).unwrap()
}

fn zip_request(body: Body) -> Request<Body> {
    Request::post(UPLOAD)
        .header(header::CONTENT_TYPE, "application/zip")
        .body(body)
        .unwrap()
}

async fn upload(app: &App, bytes: Vec<u8>) -> (StatusCode, Value) {
    send(
        app.router.clone(),
        with(&app.cookie, zip_request(Body::from(bytes))),
    )
    .await
}

/// The uploads waiting in the data dir.
fn uploads(app: &App) -> Vec<PathBuf> {
    match std::fs::read_dir(app.dir.path().join("imports")) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Every table's row count, to show an upload writes nothing to the database.
async fn row_counts(app: &App) -> Vec<(String, i64)> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    let mut counts = Vec::new();
    for table in tables {
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM \"{table}\""
        )))
        .fetch_one(&app.pool)
        .await
        .unwrap();
        counts.push((table, count));
    }
    counts
}

#[tokio::test]
async fn the_fake_export_is_read_and_nothing_is_imported() {
    let app = set_up().await;
    let before = row_counts(&app).await;
    assert!(before.iter().any(|(table, _)| table == "categories"));

    let (status, body) = upload(&app, fixture("actual-export.zip")).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().unwrap();
    assert_eq!(id.len(), 64);
    assert_eq!(
        body,
        json!({
            "id": id,
            "budget_name": "Example Household",
            "accounts": 5,
            "transactions": 80,
            "categories": 7,
            "first_date": "2025-01-01",
            "last_date": "2026-01-05",
        })
    );

    assert_eq!(row_counts(&app).await, before);
    let waiting = app.dir.path().join("imports").join(id);
    assert!(waiting.join("db.sqlite").is_file());
    assert!(waiting.join("metadata.json").is_file());
    assert!(!waiting.join("upload.zip").exists());
}

#[tokio::test]
async fn a_shared_folder_and_an_older_export_are_read_too() {
    let app = set_up().await;
    for file in ["actual-export-in-a-folder.zip", "actual-export-older.zip"] {
        let (status, body) = upload(&app, fixture(file)).await;
        assert_eq!(status, StatusCode::CREATED, "{file}: {body}");
        assert_eq!(body["transactions"], 80, "{file}");
    }
}

#[tokio::test]
async fn a_zip_that_isnt_an_actual_export_is_refused_and_not_kept() {
    let app = set_up().await;
    for bytes in [
        fixture("not-an-actual-export.zip"),
        b"not even a zip".to_vec(),
    ] {
        let (status, body) = upload(&app, bytes).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "not_an_actual_export");
        assert_eq!(
            body["error"]["message"],
            "This isn't an Actual export. In Actual, use Settings › Export data, and upload the .zip it saves."
        );
    }
    assert_eq!(uploads(&app), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn a_file_over_100_mb_is_refused() {
    let app = set_up().await;
    // 101 MB, streamed without a length, so the server has to count.
    let chunk = Bytes::from(vec![0u8; 1024 * 1024]);
    let chunks = (0..101).map(move |_| Ok::<_, std::io::Error>(chunk.clone()));
    let request = zip_request(Body::from_stream(futures_util::stream::iter(chunks)));
    let (status, body) = send(app.router.clone(), with(&app.cookie, request)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "file_too_large");
    assert_eq!(
        body["error"]["message"],
        "This file is over 100 MB, more than an Actual export should be."
    );
    assert_eq!(uploads(&app), Vec::<PathBuf>::new());

    // With its length said up front, it's refused before it's read.
    let mut request = zip_request(Body::empty());
    request.headers_mut().insert(
        header::CONTENT_LENGTH,
        (101u64 * 1024 * 1024).to_string().parse().unwrap(),
    );
    let (status, body) = send(app.router.clone(), with(&app.cookie, request)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "file_too_large");
}

#[tokio::test]
async fn the_upload_is_the_zip_itself() {
    let app = set_up().await;
    let request = Request::post(UPLOAD)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let (status, body) = send(app.router.clone(), with(&app.cookie, request)).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(body["error"]["code"], "zip_required");

    // Anywhere else, a zip is still not JSON.
    let request = Request::post("/api/v1/category-groups")
        .header(header::CONTENT_TYPE, "application/zip")
        .body(Body::from("{}"))
        .unwrap();
    let (status, body) = send(app.router.clone(), with(&app.cookie, request)).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(body["error"]["code"], "json_required");
}

#[tokio::test]
async fn uploading_needs_a_login() {
    let app = set_up().await;
    let request = zip_request(Body::from(fixture("actual-export.zip")));
    let (status, body) = send(app.router.clone(), request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "login_required");
}

#[tokio::test]
async fn a_currency_without_cents_is_refused() {
    let mut app = app().await;
    let (status, headers, _) = send_full(
        app.router.clone(),
        finish(&app, &json!({ "currency": "JPY" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    app.cookie = session_cookie(&headers).unwrap();

    let (status, body) = upload(&app, fixture("actual-export.zip")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "currency_not_supported");
    assert_eq!(
        body["error"]["message"],
        "Actual keeps amounts in cents, so its exports only fit a household currency with 2 decimal places, and JPY has 0."
    );
}

#[tokio::test]
async fn an_earlier_import_not_undone_is_refused() {
    let app = set_up().await;
    // The commit step's table, as far as this check reads it.
    for sql in [
        "CREATE TABLE import_batches (id INTEGER PRIMARY KEY, format TEXT NOT NULL, undone_at TEXT)",
        "INSERT INTO import_batches (format) VALUES ('actual')",
    ] {
        sqlx::query(sql).execute(&app.pool).await.unwrap();
    }
    let (status, body) = upload(&app, fixture("actual-export.zip")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "earlier_import");
    assert_eq!(
        body["error"]["message"],
        "Undo the earlier import from Actual first."
    );
}

#[tokio::test]
async fn uploads_older_than_a_day_are_deleted() {
    let app = set_up().await;
    let (status, first) = upload(&app, fixture("actual-export.zip")).await;
    assert_eq!(status, StatusCode::CREATED);
    let first = app
        .dir
        .path()
        .join("imports")
        .join(first["id"].as_str().unwrap());
    let day_ago = SystemTime::now() - Duration::from_hours(25);
    std::fs::File::open(&first)
        .unwrap()
        .set_modified(day_ago)
        .unwrap();

    let (status, second) = upload(&app, fixture("actual-export.zip")).await;
    assert_eq!(status, StatusCode::CREATED);
    let second = app
        .dir
        .path()
        .join("imports")
        .join(second["id"].as_str().unwrap());
    assert_eq!(uploads(&app), vec![second]);
}
