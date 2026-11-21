//! OFX import: reading, pre-selecting the account, previewing, committing and undoing, on the
//! fake OFX 1.x (QFX) and 2.x fixtures.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{App, post, send, set_up, with};

const V1: &str = include_str!("../../../../fixtures/bank-v1.qfx");
const V2: &str = include_str!("../../../../fixtures/bank-v2.ofx");

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
            post(&format!("/api/v1/imports/ofx/{step}"), body),
        ),
    )
    .await
}

async fn import(app: &App, account: i64, content: &str) -> Value {
    let (status, done) = call(
        app,
        "commit",
        &json!({ "account_id": account, "file_name": "demo.qfx", "content": content, "keep": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    done
}

type Row = (String, i64, String, Option<String>, String);

async fn rows(app: &App, account: i64) -> Vec<Row> {
    sqlx::query_as(
        "SELECT date, amount_minor, raw_description, memo, source FROM transactions
         WHERE account_id = ? ORDER BY date, id",
    )
    .bind(account)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

fn row(date: &str, minor: i64, text: &str, memo: Option<&str>) -> Row {
    (
        date.to_owned(),
        minor,
        text.to_owned(),
        memo.map(str::to_owned),
        "import".to_owned(),
    )
}

#[tokio::test]
async fn both_versions_import_with_their_amounts_dates_and_descriptions() {
    let app = set_up().await;
    let card = account(&app, "Demo card 0042").await;
    let done = import(&app, card, V1).await;
    assert_eq!(
        (done["added"].as_u64(), done["matched"].as_u64()),
        (Some(3), Some(0))
    );
    assert_eq!(
        rows(&app, card).await,
        [
            row("2026-09-03", -1234, "DEMO GROCER & DELI", None),
            row("2026-09-10", -4500, "DEMO FUEL STOP", Some("PUMP 4")),
            row("2026-09-20", 20000, "PAYMENT THANK YOU", None),
        ]
    );

    let checking = account(&app, "Demo checking 0077").await;
    import(&app, checking, V2).await;
    assert_eq!(
        rows(&app, checking).await,
        [
            row("2026-09-01", 150_000, "DEMO PAYROLL", None),
            row("2026-09-05", -8025, "DEMO LANDSCAPING", Some("CHECK 101")),
            row("2026-09-15", -30000, "TRANSFER TO SAVINGS", None),
            row("2026-09-20", -450, "DEMO CAFE", None),
        ]
    );
    let batch: (String, Option<i64>, String) =
        sqlx::query_as("SELECT format, mapping_id, file_name FROM import_batches WHERE id = ?")
            .bind(done["batch_id"].as_i64())
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(batch, ("ofx".to_owned(), None, "demo.qfx".to_owned()));
    let refs: Vec<String> = sqlx::query_scalar(
        "SELECT import_ref FROM import_batch_rows WHERE batch_id = ? ORDER BY import_ref",
    )
    .bind(done["batch_id"].as_i64())
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(refs, ["DEMO-V1-0001", "DEMO-V1-0002", "DEMO-V1-0003"]);
}

#[tokio::test]
async fn a_second_import_of_the_same_file_adds_nothing() {
    let app = set_up().await;
    let card = account(&app, "Demo card 0042").await;
    import(&app, card, V1).await;
    let body = json!({ "account_id": card, "content": V1 });
    let (status, preview) = call(&app, "preview", &body).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        (&preview["new"], &preview["already"], &preview["to_check"]),
        (&json!(0), &json!(3), &json!(0))
    );
    let again = import(&app, card, V1).await;
    assert_eq!(
        (again["added"].as_u64(), again["matched"].as_u64()),
        (Some(0), Some(3))
    );
    assert_eq!(rows(&app, card).await.len(), 3);
}

#[tokio::test]
async fn a_synced_row_is_matched_and_keeps_its_own_fields() {
    let app = set_up().await;
    let card = account(&app, "Demo card 0042").await;
    // The bank's own row for the first transaction, a day off and in its words.
    sqlx::query(
        "INSERT INTO transactions (account_id, date, amount_minor, raw_description, source, dedupe_key)
         VALUES (?, '2026-09-04', -1234, 'GROCER DELI 0001', 'provider', 'k')",
    )
    .bind(card)
    .execute(&app.pool)
    .await
    .unwrap();
    let (_, preview) = call(
        &app,
        "preview",
        &json!({ "account_id": card, "content": V1 }),
    )
    .await;
    assert_eq!(
        (&preview["new"], &preview["already"]),
        (&json!(2), &json!(1))
    );
    import(&app, card, V1).await;
    let stored = rows(&app, card).await;
    assert_eq!(stored.len(), 3);
    assert_eq!(stored[0].2, "GROCER DELI 0001");
    assert_eq!(stored[0].4, "provider");
}

#[tokio::test]
async fn read_preselects_the_account_by_its_last_four_digits() {
    let app = set_up().await;
    account(&app, "Demo checking 0077").await;
    let card = account(&app, "Demo card 0042").await;
    let (status, read) = call(&app, "read", &json!({ "content": V1 })).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(
        read,
        json!({
            "last_four": "0042", "account_id": card, "transactions": 3,
            "first_date": "2026-09-03", "last_date": "2026-09-20",
        })
    );
    // Two accounts ending the same way: the household chooses.
    account(&app, "Old card 0042").await;
    let (_, read) = call(&app, "read", &json!({ "content": V1 })).await;
    assert_eq!(read["account_id"], Value::Null);
    assert_eq!(read["last_four"], "0042");
}

#[tokio::test]
async fn undo_removes_the_rows_the_import_added() {
    let app = set_up().await;
    let checking = account(&app, "Demo checking 0077").await;
    let done = import(&app, checking, V2).await;
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
    assert_eq!(rows(&app, checking).await, []);
    // Undone, the file is new again.
    let again = import(&app, checking, V2).await;
    assert_eq!(again["added"], 4);
}

#[tokio::test]
async fn bad_files_are_refused() {
    let app = set_up().await;
    let id = account(&app, "Checking").await;
    let code = |body: Value| body["error"]["code"].as_str().map(str::to_owned);
    let (status, body) = call(&app, "read", &json!({ "content": "Date,Amount\n" })).await;
    assert_eq!(
        (status, code(body).as_deref()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("not_ofx"))
    );
    let (_, body) = call(&app, "read", &json!({ "content": "<OFX></OFX>" })).await;
    assert_eq!(code(body).as_deref(), Some("empty_file"));
    let several = "<OFX><STMTRS></STMTRS><CCSTMTRS></CCSTMTRS></OFX>";
    let (_, body) = call(&app, "read", &json!({ "content": several })).await;
    assert_eq!(code(body).as_deref(), Some("several_accounts"));
    let unreadable = "<OFX><STMTRS><STMTTRN><DTPOSTED>x<TRNAMT>1</STMTTRN></STMTRS></OFX>";
    let (status, body) = call(
        &app,
        "commit",
        &json!({ "account_id": id, "file_name": "", "content": unreadable, "keep": [] }),
    )
    .await;
    assert_eq!(
        (status, code(body).as_deref()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("nothing_readable"))
    );
    let (status, _) = call(
        &app,
        "preview",
        &json!({ "account_id": 999, "content": V1 }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        app.router.clone(),
        post("/api/v1/imports/ofx/read", &json!({ "content": V1 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
