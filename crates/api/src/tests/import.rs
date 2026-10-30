//! Import from Actual through the API: uploading the fake exports in `fixtures/` (step 1),
//! reading their accounts and categories back (steps 2 and 3), and committing them (step 4).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};

use super::{App, app, finish, get, post, send, send_full, session_cookie, set_up, with};

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
    sqlx::query(
        "INSERT INTO import_batches (format, file_name, added, matched, skipped)
         VALUES ('actual', 'export.zip', 0, 0, 0)",
    )
    .execute(&app.pool)
    .await
    .unwrap();
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

/// `GET` the accounts and categories of the upload `id`.
async fn plan(app: &App, id: &str) -> (StatusCode, Value) {
    send(
        app.router.clone(),
        with(&app.cookie, get(&format!("{UPLOAD}/{id}"))),
    )
    .await
}

/// Uploads the fake export and reads its accounts and categories back.
async fn fake_plan(app: &App) -> Value {
    let (status, upload) = upload(app, fixture("actual-export.zip")).await;
    assert_eq!(status, StatusCode::CREATED, "{upload}");
    let id = upload["id"].as_str().unwrap();
    let (status, body) = plan(app, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], id);
    body
}

/// The fake export's ids for its accounts (kind 1) and categories (kind 3).
fn fake_id(kind: u32, n: u32) -> String {
    format!("{kind:08x}-0000-4000-8000-{n:012x}")
}

#[tokio::test]
async fn the_fake_exports_accounts_come_with_their_figures_and_defaults() {
    let app = set_up().await;
    let before = row_counts(&app).await;
    let body = fake_plan(&app).await;
    assert_eq!(body["budget_name"], "Example Household");
    assert_eq!(body["app_accounts"], json!([]));
    let account = |n, name, closed, off_budget, rows, balance: i64, kind, role| {
        json!({
            "id": fake_id(1, n),
            "name": name,
            "closed": closed,
            "off_budget": off_budget,
            "rows": rows,
            "balance_minor": balance,
            "type": kind,
            "role": role,
            "same_as": null,
        })
    };
    assert_eq!(
        body["accounts"],
        json!([
            account(
                1,
                "Everyday checking",
                false,
                false,
                43,
                2_145_801,
                "checking",
                "budgeted"
            ),
            account(
                2,
                "Rewards card",
                false,
                false,
                30,
                -136_335,
                "credit_card",
                "budgeted"
            ),
            account(
                3,
                "Rainy day savings",
                false,
                false,
                2,
                400_000,
                "savings",
                "budgeted"
            ),
            account(
                4,
                "Brokerage",
                false,
                true,
                2,
                1_100_000,
                "investment",
                "tracking"
            ),
            account(5, "Old checking", true, false, 3, 0, "checking", "budgeted"),
        ])
    );
    // Reading it back writes nothing either.
    assert_eq!(row_counts(&app).await, before);
}

#[tokio::test]
async fn the_categories_are_those_rows_use_without_starting_balances() {
    let app = set_up().await;
    let body = fake_plan(&app).await;
    let rows = |uses: &[(u32, u64)]| {
        uses.iter()
            .map(|(n, rows)| json!({ "account_id": fake_id(1, *n), "rows": rows }))
            .collect::<Vec<_>>()
    };
    let category = |n, name, group, income, uses: &[(u32, u64)]| {
        json!({
            "id": fake_id(3, n),
            "name": name,
            "group": group,
            "income": income,
            "rows": rows(uses),
        })
    };
    assert_eq!(
        body["categories"],
        json!([
            category(1, "Groceries", "Everyday", false, &[(2, 14), (5, 1)]),
            category(2, "Dining out", "Everyday", false, &[(2, 2)]),
            category(3, "Fuel", "Everyday", false, &[(1, 1), (2, 12)]),
            category(4, "Rent", "Home", false, &[(1, 12)]),
            category(5, "Utilities", "Home", false, &[(1, 12)]),
            category(6, "Paycheck", "Income", true, &[(1, 12)]),
        ])
    );
}

#[tokio::test]
async fn same_as_starts_on_the_app_account_with_the_same_bank_sync_id() {
    let app = set_up().await;
    sqlx::query(
        "INSERT INTO accounts (name, type, role, external_account_id) VALUES
           ('Shared card', 'credit_card', 'budgeted', 'fake-bank-0002'),
           ('Wallet', 'cash', 'budgeted', NULL)",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let body = fake_plan(&app).await;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM accounts ORDER BY id")
        .fetch_all(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        body["app_accounts"],
        json!([
            { "id": ids[0], "name": "Shared card" },
            { "id": ids[1], "name": "Wallet" },
        ])
    );
    let same_as: Vec<&Value> = body["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|account| &account["same_as"])
        .collect();
    assert_eq!(
        same_as,
        [
            &json!(null),
            &json!(ids[0]),
            &json!(null),
            &json!(null),
            &json!(null)
        ]
    );
}

#[tokio::test]
async fn an_upload_that_isnt_there_is_not_found() {
    let app = set_up().await;
    let unknown = "0".repeat(64);
    for id in [unknown.as_str(), "..", "ABC", &"g".repeat(64)] {
        let (status, body) = plan(&app, id).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{id}");
        assert_eq!(body["error"]["code"], "upload_not_found", "{id}");
        assert_eq!(
            body["error"]["message"],
            "This upload is gone: uploads wait a day. Upload the export again."
        );
    }
}

#[tokio::test]
async fn an_upload_whose_database_went_bad_is_unreadable() {
    let app = set_up().await;
    let (_, upload) = upload(&app, fixture("actual-export.zip")).await;
    let id = upload["id"].as_str().unwrap();
    let dir = app.dir.path().join("imports").join(id);
    std::fs::write(dir.join("db.sqlite"), b"not a database").unwrap();
    let (status, body) = plan(&app, id).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "not_an_actual_export");
}

#[tokio::test]
async fn reading_an_upload_needs_a_login() {
    let app = set_up().await;
    let (status, body) = send(
        app.router.clone(),
        get(&format!("{UPLOAD}/{}", "0".repeat(64))),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "login_required");
}

// ── Step 4: the commit ──

/// The upload `id`'s live rows, as the reader has them.
async fn read_back(app: &App, id: &str) -> domain::actual::Export {
    let dir = app.dir.path().join("imports").join(id);
    let mut export = actual::Export::open(&dir).await.unwrap();
    let read = export.read().await.unwrap();
    export.close().await;
    read
}

/// Every account on Create new as step 2 starts it.
fn create_all(plan: &Value) -> Vec<Value> {
    plan["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|account| {
            json!({
                "actual_id": account["id"],
                "action": "create",
                "name": account["name"],
                "type": account["type"],
                "role": account["role"],
                "balances_only": false,
                "same_as": null,
            })
        })
        .collect()
}

async fn commit(app: &App, id: &str, body: &Value) -> (StatusCode, Value) {
    send(
        app.router.clone(),
        with(&app.cookie, post(&format!("{UPLOAD}/{id}/commit"), body)),
    )
    .await
}

/// A category, Groceries, in a group Food.
async fn groceries(app: &App) -> i64 {
    sqlx::query("INSERT INTO category_groups (id, name) VALUES (1, 'Food')")
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query_scalar(
        "INSERT INTO categories (group_id, name) VALUES (1, 'Groceries') RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn the_fake_export_comes_in_with_every_balance_matching_actual() {
    let app = set_up().await;
    let category = groceries(&app).await;
    let plan = fake_plan(&app).await;
    let id = plan["id"].as_str().unwrap().to_owned();
    let export = read_back(&app, &id).await;
    let (status, done) = commit(
        &app,
        &id,
        &json!({
            "file_name": "actual-export.zip",
            "accounts": create_all(&plan),
            "categories": [
                { "actual_id": fake_id(3, 1), "goes_to": "category", "category_id": category },
                { "actual_id": fake_id(3, 2), "goes_to": "transfer", "category_id": null },
                { "actual_id": fake_id(3, 3), "goes_to": "later", "category_id": null },
            ],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{done}");

    // Each account's rows are the reader's but for starting balances.
    let starting: Vec<&str> = export
        .transactions
        .iter()
        .filter(|t| t.starting_balance)
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(starting.len(), 5);
    let expected_rows: Vec<(String, u64)> = export
        .accounts
        .iter()
        .map(|account| {
            let starting = export
                .transactions
                .iter()
                .filter(|t| t.account == account.id && t.starting_balance)
                .count() as u64;
            (account.name.clone(), account.rows - starting)
        })
        .collect();
    let rows: Vec<(String, u64)> = done["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["name"].as_str().unwrap().to_owned(),
                a["rows"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(rows, expected_rows);
    assert_eq!(done["accounts_created"], 5);
    assert_eq!(done["linked_transfers"], 0);
    let total: u64 = expected_rows.iter().map(|(_, rows)| rows).sum();
    let counts: Vec<u64> = ["mapped", "undecided", "transfer", "uncategorized"]
        .iter()
        .map(|count| done[count].as_u64().unwrap())
        .collect();
    assert_eq!(counts.iter().sum::<u64>(), total);
    // Groceries's plain rows; Fuel, Rent, Utilities, Paycheck and the splits; Dining out and the
    // transfer legs; rows with no category.
    assert_eq!(counts, [14, 50, 7, 4]);

    // Every account's app balance (opening + rows) is its Actual balance, and so is its snapshot.
    let balances = db::account_balances(&app.pool).await.unwrap();
    assert_eq!(balances.len(), 5);
    for (balance, actual) in balances.iter().zip(&export.accounts) {
        assert_eq!(balance.name, actual.name);
        assert_eq!(balance.app_balance, Some(actual.balance), "{}", actual.name);
        assert_eq!(balance.snapshot, Some(actual.balance), "{}", actual.name);
    }

    // Starting balances never become transactions, so never income either.
    let refs: Vec<String> = sqlx::query_scalar("SELECT import_ref FROM import_batch_rows")
        .fetch_all(&app.pool)
        .await
        .unwrap();
    assert_eq!(refs.len() as u64, total);
    assert!(refs.iter().all(|r| !starting.contains(&r.as_str())));
    let transactions: i64 = sqlx::query_scalar("SELECT count(*) FROM transactions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(u64::try_from(transactions).unwrap(), total);

    // The upload is gone, and a new one waits for this import to be undone.
    assert_eq!(uploads(&app), Vec::<PathBuf>::new());
    let (status, body) = upload(&app, fixture("actual-export.zip")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "earlier_import");

    // A category the rows use can only be hidden now.
    let delete = Request::delete(format!("/api/v1/categories/{category}"))
        .header(header::HOST, "localhost:8080")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send(app.router.clone(), with(&app.cookie, delete)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "category_in_use");
}

#[tokio::test]
async fn a_second_commit_waits_for_the_first_to_be_undone() {
    let app = set_up().await;
    let first = fake_plan(&app).await;
    let second = fake_plan(&app).await;
    let body = |plan: &Value| json!({ "file_name": "a.zip", "accounts": create_all(plan), "categories": [] });
    let (status, _) = commit(&app, first["id"].as_str().unwrap(), &body(&first)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, refused) = commit(&app, second["id"].as_str().unwrap(), &body(&second)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        refused["error"]["message"],
        "Undo the earlier import from Actual first."
    );
    let batches: i64 = sqlx::query_scalar("SELECT count(*) FROM import_batches")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(batches, 1);
}

#[tokio::test]
async fn choices_that_cant_go_ahead_are_refused() {
    let app = set_up().await;
    let plan = fake_plan(&app).await;
    let id = plan["id"].as_str().unwrap();
    let mut accounts = create_all(&plan);
    let refused = |accounts: Vec<Value>, categories: Value| json!({ "file_name": "a.zip", "accounts": accounts, "categories": categories });

    let (status, body) = commit(&app, id, &refused(accounts[1..].to_vec(), json!([]))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_choices");
    assert_eq!(
        body["error"]["message"],
        "Choose what happens to Everyday checking."
    );

    accounts[0]["action"] = json!("same");
    let (_, body) = commit(&app, id, &refused(accounts.clone(), json!([]))).await;
    assert_eq!(
        body["error"]["message"],
        "Choose the account Everyday checking is the same as."
    );

    accounts[0]["action"] = json!("create");
    let gone = json!([{ "actual_id": fake_id(3, 1), "goes_to": "category", "category_id": 999 }]);
    let (_, body) = commit(&app, id, &refused(accounts.clone(), gone)).await;
    assert_eq!(
        body["error"]["message"],
        "Groceries goes to a category that isn't there any more."
    );
    let none = json!([{ "actual_id": fake_id(3, 1), "goes_to": "category", "category_id": null }]);
    let (_, body) = commit(&app, id, &refused(accounts.clone(), none)).await;
    assert_eq!(body["error"]["code"], "invalid_choices");

    let unknown = "0".repeat(64);
    let (status, body) = commit(&app, &unknown, &refused(accounts, json!([]))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "upload_not_found");

    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(accounts, 0);
}

// ── Accounts, after the commit ──

#[tokio::test]
async fn accounts_show_the_imported_balances() {
    let app = set_up().await;
    let (status, list) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/accounts")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list, json!({ "accounts": [] }));

    let plan = fake_plan(&app).await;
    let id = plan["id"].as_str().unwrap().to_owned();
    let export = read_back(&app, &id).await;
    // Brokerage comes in balances only.
    let mut accounts = create_all(&plan);
    for account in &mut accounts {
        if account["name"] == "Brokerage" {
            account["balances_only"] = json!(true);
        }
    }
    let body = json!({ "file_name": "actual-export.zip", "accounts": accounts, "categories": [] });
    let (status, done) = commit(&app, &id, &body).await;
    assert_eq!(status, StatusCode::CREATED, "{done}");

    let (status, list) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/accounts")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let listed = list["accounts"].as_array().unwrap();
    assert_eq!(listed.len(), export.accounts.len());
    for (account, actual) in listed.iter().zip(&export.accounts) {
        let name = &actual.name;
        assert_eq!(account["name"], json!(name));
        assert_eq!(account["balance_minor"], actual.balance.minor(), "{name}");
        assert_eq!(account["closed"], actual.closed, "{name}");
        let role = if actual.off_budget {
            "tracking"
        } else {
            "budgeted"
        };
        assert_eq!(account["role"], role, "{name}");
        if name == "Brokerage" {
            assert_eq!(account["balances_only"], true);
            assert_eq!(account["app_balance_minor"], Value::Null);
            assert_eq!(account["opening_date"], Value::Null);
            assert_eq!(account["rows"], 0);
        } else {
            assert_eq!(account["balances_only"], false, "{name}");
            assert_eq!(
                account["app_balance_minor"], account["balance_minor"],
                "{name}"
            );
            assert!(account["opening_date"].is_string(), "{name}");
        }
    }

    let first = &listed[0];
    let (status, one) = send(
        app.router.clone(),
        with(
            &app.cookie,
            get(&format!("/api/v1/accounts/{}", first["id"])),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&one, first);
    assert!(one["last_transaction_date"].is_string());

    for missing in ["/api/v1/accounts/999", "/api/v1/accounts/nope"] {
        let (status, body) = send(app.router.clone(), with(&app.cookie, get(missing))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
        assert_eq!(body["error"]["code"], "not_found");
    }
    let (status, _) = send(app.router.clone(), get("/api/v1/accounts")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
