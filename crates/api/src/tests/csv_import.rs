//! CSV import: reading a file, previewing, committing and undoing, on the fake fixtures.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{App, post, send, set_up, with};

const SIGNED: &str = include_str!("../../../../fixtures/bank-signed.csv");
const DEBIT_CREDIT: &str = include_str!("../../../../fixtures/bank-debit-credit.csv");
const DAY_FIRST: &str = include_str!("../../../../fixtures/bank-day-first.csv");
const CATEGORY: &str = include_str!("../../../../fixtures/bank-category.csv");

async fn account(app: &App, name: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO accounts (name, type, role) VALUES (?, 'checking', 'budgeted') RETURNING id",
    )
    .bind(name)
    .fetch_one(&app.pool)
    .await
    .unwrap()
}

async fn call(app: &App, step: &str, body: &Value) -> (StatusCode, Value) {
    send(
        app.router.clone(),
        with(
            &app.cookie,
            post(&format!("/api/v1/imports/csv/{step}"), body),
        ),
    )
    .await
}

/// Reads `content`, then commits it with the mapping the read answered (the saved one, else the
/// guess).
async fn import(app: &App, account: i64, content: &str) -> Value {
    let (status, read) = call(
        app,
        "read",
        &json!({ "account_id": account, "content": content }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    let mapping = if read["saved"].is_null() {
        &read["guess"]
    } else {
        &read["saved"]
    };
    let (status, done) = call(
        app,
        "commit",
        &json!({
            "account_id": account, "file_name": "demo.csv", "content": content,
            "mapping": mapping, "keep": [],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    done
}

type Row = (String, i64, String, Option<String>, Option<String>);

async fn rows(app: &App, account: i64) -> Vec<Row> {
    sqlx::query_as(
        "SELECT date, amount_minor, raw_description, bank_category, memo FROM transactions
         WHERE account_id = ? ORDER BY date, id",
    )
    .bind(account)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

fn row(date: &str, minor: i64, text: &str) -> Row {
    (date.to_owned(), minor, text.to_owned(), None, None)
}

#[tokio::test]
async fn each_fixture_imports_with_the_right_signs_and_dates() {
    let app = set_up().await;
    let signed = account(&app, "Signed").await;
    let done = import(&app, signed, SIGNED).await;
    assert_eq!(
        (done["added"].as_u64(), done["matched"].as_u64()),
        (Some(5), Some(0))
    );
    assert_eq!(
        rows(&app, signed).await,
        [
            row("2026-10-01", -450, "DEMO CAFE #12"),
            row("2026-10-02", 125_000, "DEMO PAYROLL"),
            row("2026-10-03", -6218, "DEMO GROCERY"),
            row("2026-10-03", -6218, "DEMO GROCERY"),
            row("2026-10-05", -1800, "DEMO BOOKS, ONLINE"),
        ]
    );

    let split = account(&app, "Debit credit").await;
    import(&app, split, DEBIT_CREDIT).await;
    assert_eq!(
        rows(&app, split).await,
        [
            row("2026-10-02", -3840, "DEMO GAS STATION"),
            row("2026-10-04", 1299, "DEMO REFUND"),
            row("2026-10-06", -120_400, "DEMO UTILITY"),
        ]
    );

    let day_first = account(&app, "Day first").await;
    import(&app, day_first, DAY_FIRST).await;
    assert_eq!(
        rows(&app, day_first).await,
        [
            row("2026-10-25", -1425, "DEMO PHARMACY"),
            row("2026-10-26", 150_000, "DEMO TRANSFER IN"),
            row("2026-10-27", -300, "DEMO LIBRARY"),
        ]
    );

    let category = account(&app, "Category").await;
    import(&app, category, CATEGORY).await;
    let with_category = rows(&app, category).await;
    assert_eq!(with_category.len(), 3);
    assert_eq!(
        with_category[0],
        (
            "2026-09-01".to_owned(),
            -450,
            "DEMO CAFE #12".to_owned(),
            Some("Dining".to_owned()),
            Some("morning coffee".to_owned())
        )
    );
    assert_eq!(with_category[1].3.as_deref(), Some("Home"));
    // Imported rows are the import's and posted; these are under 90 days old, so not quiet.
    let sources: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT DISTINCT source, status, review_quiet FROM transactions WHERE account_id = ?",
    )
    .bind(category)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(sources, [("import".to_owned(), "posted".to_owned(), false)]);
}

#[tokio::test]
async fn a_second_file_with_the_same_header_finds_the_saved_mapping() {
    let app = set_up().await;
    let one = account(&app, "One").await;
    let other = account(&app, "Other").await;
    let body = |account: i64, content: &str| json!({ "account_id": account, "content": content });
    let (_, before) = call(&app, "read", &body(one, SIGNED)).await;
    assert!(before["saved"].is_null());
    assert_eq!(before["columns"], 3);
    assert_eq!(before["record_count"], 6);
    assert_eq!(
        before["records"][0],
        json!(["Date", "Description", "Amount"])
    );

    // Saved with the household's change to the guess.
    let mut mapping = before["guess"].clone();
    mapping["flip_sign"] = json!(true);
    let (status, _) = call(
        &app,
        "commit",
        &json!({ "account_id": one, "file_name": "a.csv", "content": SIGNED, "mapping": mapping, "keep": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let newer = "date,AMOUNT ,description\n2026-11-01,-1.00,x\n";
    let later = "Date,Description,Amount\n2026-11-01,DEMO NEW,-9.00\n";
    let (_, same) = call(&app, "read", &body(one, later)).await;
    assert_eq!(same["saved"], mapping);
    // Another account, or another header, starts from the guess.
    let (_, elsewhere) = call(&app, "read", &body(other, later)).await;
    assert!(elsewhere["saved"].is_null());
    let (_, changed) = call(&app, "read", &body(one, newer)).await;
    assert!(changed["saved"].is_null());
    // The flipped sign reached the rows.
    let (_, preview) = call(
        &app,
        "preview",
        &json!({ "account_id": one, "content": later, "mapping": same["saved"] }),
    )
    .await;
    assert_eq!(preview["sample"][0]["amount_minor"], 900);
}

#[tokio::test]
async fn the_preview_counts_what_import_writes_and_a_second_import_adds_nothing() {
    let app = set_up().await;
    let id = account(&app, "Checking").await;
    let (_, read) = call(
        &app,
        "read",
        &json!({ "account_id": id, "content": SIGNED }),
    )
    .await;
    let request = json!({ "account_id": id, "content": SIGNED, "mapping": read["guess"] });
    let (status, preview) = call(&app, "preview", &request).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        (
            preview["new"].as_u64(),
            preview["already"].as_u64(),
            preview["to_check"].as_u64()
        ),
        (Some(5), Some(0), Some(0))
    );
    assert_eq!(preview["sample"][0]["date"], "2026-10-01");
    assert_eq!(rows(&app, id).await.len(), 0, "a preview writes nothing");

    let done = import(&app, id, SIGNED).await;
    assert_eq!(done["added"], preview["new"]);
    let (_, again) = call(&app, "preview", &request).await;
    assert_eq!(
        (again["new"].as_u64(), again["already"].as_u64()),
        (Some(0), Some(5))
    );
    let second = import(&app, id, SIGNED).await;
    assert_eq!(
        (second["added"].as_u64(), second["matched"].as_u64()),
        (Some(0), Some(5))
    );
    assert_eq!(rows(&app, id).await.len(), 5);
}

#[tokio::test]
async fn rows_to_check_wait_for_the_household_and_a_bank_row_counts_as_there() {
    let app = set_up().await;
    let id = account(&app, "Checking").await;
    for (text, source, status) in [
        ("A", "manual", "posted"),
        ("B", "manual", "posted"),
        ("DEMO PAYROLL", "provider", "pending"),
    ] {
        sqlx::query(
            "INSERT INTO transactions (account_id, date, amount_minor, status, raw_description, source, dedupe_key)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind("2026-10-01")
        .bind(if text == "DEMO PAYROLL" { 125_000 } else { -450 })
        .bind(status)
        .bind(text)
        .bind(source)
        .bind(text)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    let (_, read) = call(
        &app,
        "read",
        &json!({ "account_id": id, "content": SIGNED }),
    )
    .await;
    let request = json!({ "account_id": id, "content": SIGNED, "mapping": read["guess"] });
    let (_, preview) = call(&app, "preview", &request).await;
    // Payroll matches the pending bank row; the cafe row could be A or B.
    assert_eq!(
        (
            preview["new"].as_u64(),
            preview["already"].as_u64(),
            preview["to_check"].as_u64()
        ),
        (Some(3), Some(1), Some(1))
    );
    assert_eq!(preview["check"][0]["description"], "DEMO CAFE #12");
    assert_eq!(preview["check"][0]["could_be"]["description"], "A");
    let row = preview["check"][0]["row"].clone();

    // Kept, it's added; left out, it's skipped.
    let commit = |keep: Value| {
        json!({
            "account_id": id, "file_name": "demo.csv", "content": SIGNED, "mapping": read["guess"], "keep": keep,
        })
    };
    let (_, done) = call(&app, "commit", &commit(json!([row]))).await;
    assert_eq!(
        (
            done["added"].as_u64(),
            done["matched"].as_u64(),
            done["skipped"].as_u64()
        ),
        (Some(4), Some(1), Some(0))
    );
    let (_, again) = call(&app, "commit", &commit(json!([]))).await;
    assert_eq!(again["added"], 0);
}

#[tokio::test]
async fn undo_removes_the_rows_and_bank_categories_the_import_added() {
    let app = set_up().await;
    let id = account(&app, "Checking").await;
    sqlx::query(
        "INSERT INTO transactions (account_id, date, amount_minor, raw_description, source, dedupe_key)
         VALUES (?, '2026-09-01', -450, 'DEMO CAFE', 'provider', 'k')",
    )
    .bind(id)
    .execute(&app.pool)
    .await
    .unwrap();
    let done = import(&app, id, CATEGORY).await;
    assert_eq!(done["matched"], 1);
    let count = || async {
        let rows: Vec<(i64, Option<String>)> = sqlx::query_as(
            "SELECT id, bank_category FROM transactions WHERE account_id = ? ORDER BY id",
        )
        .bind(id)
        .fetch_all(&app.pool)
        .await
        .unwrap();
        rows
    };
    let during = count().await;
    assert_eq!(during.len(), 3);
    assert_eq!(during[0].1.as_deref(), Some("Dining"));

    let (status, _) = send(
        app.router.clone(),
        with(
            &app.cookie,
            post(
                &format!("/api/v1/imports/{}/undo", done["batch_id"]),
                &json!({}),
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count().await, [(during[0].0, None)]);
}

#[tokio::test]
async fn bad_requests_are_refused() {
    let app = set_up().await;
    let id = account(&app, "Checking").await;
    let (_, read) = call(
        &app,
        "read",
        &json!({ "account_id": id, "content": SIGNED }),
    )
    .await;
    let guess = read["guess"].clone();

    let (status, body) = call(
        &app,
        "read",
        &json!({ "account_id": id + 1, "content": SIGNED }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found"))
    );
    for content in ["", " \n\n"] {
        let (status, body) = call(
            &app,
            "read",
            &json!({ "account_id": id, "content": content }),
        )
        .await;
        assert_eq!(
            (status, body["error"]["code"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("empty_file"))
        );
    }
    let (status, body) = call(
        &app,
        "preview",
        &json!({ "account_id": id, "content": "", "mapping": guess }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("empty_file"))
    );

    let mut far = guess.clone();
    far["description"] = json!(7);
    let (status, body) = call(
        &app,
        "preview",
        &json!({ "account_id": id, "content": SIGNED, "mapping": far }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_mapping"))
    );
    let mut skipping = guess.clone();
    skipping["skip_rows"] = json!(40);
    let (status, _) = call(
        &app,
        "preview",
        &json!({ "account_id": id, "content": SIGNED, "mapping": skipping }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A date format the dates don't fit: nothing reads, and nothing is imported.
    let mut wrong = guess;
    wrong["date_format"] = json!("dmy");
    let request = json!({ "account_id": id, "content": SIGNED, "mapping": wrong, "file_name": " ", "keep": [] });
    let (_, preview) = call(&app, "preview", &request).await;
    assert_eq!(
        (preview["unreadable"].as_u64(), preview["new"].as_u64()),
        (Some(5), Some(0))
    );
    assert_eq!(preview["problems"][0]["row"], 2);
    let (status, body) = call(&app, "commit", &request).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("nothing_readable"))
    );
    assert_eq!(rows(&app, id).await.len(), 0);
}

#[tokio::test]
async fn a_blank_file_name_is_called_file_csv_and_the_import_needs_a_login() {
    let app = set_up().await;
    let id = account(&app, "Checking").await;
    let (_, read) = call(
        &app,
        "read",
        &json!({ "account_id": id, "content": SIGNED }),
    )
    .await;
    let (status, _) = call(
        &app,
        "commit",
        &json!({ "account_id": id, "file_name": " ", "content": SIGNED, "mapping": read["guess"], "keep": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let name: String = sqlx::query_scalar("SELECT file_name FROM import_batches")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(name, "file.csv");
    let (status, _) = send(
        app.router.clone(),
        post(
            "/api/v1/imports/csv/read",
            &json!({ "account_id": id, "content": SIGNED }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
