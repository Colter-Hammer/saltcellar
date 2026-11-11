//! Manual entry through the API: entering, editing, deleting and entering again, and transfers.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};

use super::{App, get, send, set_up, with};

async fn call(app: &App, method: &str, uri: &str, body: &Value) -> (StatusCode, Value) {
    let request = if method == "GET" {
        get(uri)
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    send(app.router.clone(), with(&app.cookie, request)).await
}

/// Two accounts that hold transactions, one balances-only, and two categories.
async fn ledger() -> App {
    let app = set_up().await;
    sqlx::query(
        r"INSERT INTO accounts (id, name, type, role, has_transactions) VALUES
            (1, 'Wallet', 'cash', 'budgeted', 1), (2, 'Savings', 'savings', 'tracking', 1),
            (3, 'Fund', 'investment', 'tracking', 0);
          INSERT INTO category_groups (id, name) VALUES (1, 'Food');
          INSERT INTO categories (id, group_id, name) VALUES (1, 1, 'Groceries'), (2, 1, 'Dining out');
          INSERT INTO payees (id, name) VALUES (1, 'Corner Market');",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    app
}

fn groceries() -> Value {
    json!({
        "account_id": 1, "date": "2026-10-27", "amount_minor": -1250, "payee": "Corner Market",
        "category_id": 1, "memo": "milk", "tags": ["#weekly"],
    })
}

#[tokio::test]
async fn entering_a_row_lists_it_with_the_manual_badge_and_its_category() {
    let app = ledger().await;
    let (status, row) = call(&app, "POST", "/api/v1/transactions", &groceries()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(row["manual"], true);
    assert_eq!(row["source"], "manual");
    assert_eq!(row["status"], "posted");
    assert_eq!(row["category"], "Groceries");
    assert_eq!(row["payee"], "Corner Market");
    assert_eq!(row["tags"], json!(["weekly"]));
    assert_eq!(row["categorization_reason"], "Set by Sam");

    let (_, page) = call(&app, "GET", "/api/v1/transactions", &Value::Null).await;
    assert_eq!(page["transactions"][0]["id"], row["id"]);
    assert_eq!(page["transactions"][0]["manual"], true);
}

#[tokio::test]
async fn a_category_is_required_unless_its_a_transfer() {
    let app = ledger().await;
    let mut body = groceries();
    body["category_id"] = Value::Null;
    let (status, error) = call(&app, "POST", "/api/v1/transactions", &body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error["error"]["fields"]["category_id"],
        "Choose a category."
    );

    // Missing references are named by field too.
    for (key, value, field) in [
        ("category_id", json!(9), "category_id"),
        ("account_id", json!(3), "account_id"),
        ("account_id", json!(9), "account_id"),
    ] {
        let mut body = groceries();
        body[key] = value;
        let (status, error) = call(&app, "POST", "/api/v1/transactions", &body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{key}");
        assert!(error["error"]["fields"][field].is_string(), "{key}");
    }
}

#[tokio::test]
async fn transfer_to_writes_one_leg_and_names_the_other_account() {
    let app = ledger().await;
    let body = json!({
        "account_id": 1, "date": "2026-10-27", "amount_minor": -5000, "transfer_account_id": 2,
    });
    let (status, row) = call(&app, "POST", "/api/v1/transactions", &body).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(row["category_id"], Value::Null);
    assert_eq!(row["transfer_account"], "Savings");
    assert_eq!(row["transfer_account_id"], 2);
    let legs: (i64, Option<i64>, String) =
        sqlx::query_as("SELECT from_txn_id, to_txn_id, matched_by FROM transfers")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        legs,
        (row["id"].as_i64().unwrap(), None, "manual".to_owned())
    );
}

#[tokio::test]
async fn edit_changes_every_field_and_delete_then_enter_again_brings_it_back() {
    let app = ledger().await;
    let (_, row) = call(&app, "POST", "/api/v1/transactions", &groceries()).await;
    let uri = format!("/api/v1/transactions/{}", row["id"]);

    let changed = json!({
        "account_id": 2, "date": "2026-10-20", "amount_minor": 300, "payee": "Fake Bakery",
        "category_id": 2, "memo": null, "tags": ["gift"],
    });
    let (status, edited) = call(&app, "PUT", &uri, &changed).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(edited["account"], "Savings");
    assert_eq!(edited["date"], "2026-10-20");
    assert_eq!(edited["amount_minor"], 300);
    assert_eq!(edited["payee"], "Fake Bakery");
    assert_eq!(edited["category"], "Dining out");
    assert_eq!(edited["memo"], Value::Null);
    assert_eq!(edited["tags"], json!(["gift"]));
    let (_, payees) = call(&app, "GET", "/api/v1/payees", &Value::Null).await;
    assert_eq!(payees["payees"][1]["name"], "Fake Bakery");

    let (status, _) = call(&app, "DELETE", &uri, &Value::Null).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, "GET", &uri, &Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Undo enters the same fields again.
    let (status, back) = call(&app, "POST", "/api/v1/transactions", &changed).await;
    assert_eq!(status, StatusCode::CREATED);
    for key in [
        "account",
        "date",
        "amount_minor",
        "payee",
        "category",
        "memo",
        "tags",
    ] {
        assert_eq!(back[key], edited[key], "{key}");
    }
}

#[tokio::test]
async fn only_manual_rows_can_be_edited_or_deleted() {
    let app = ledger().await;
    sqlx::query(
        "INSERT INTO transactions (id, account_id, date, amount_minor, raw_description, source, dedupe_key)
           VALUES (40, 1, '2026-10-01', -100, 'FEE', 'import', 'k')",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let (status, error) = call(&app, "PUT", "/api/v1/transactions/40", &groceries()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "not_manual");
    let (status, _) = call(&app, "DELETE", "/api/v1/transactions/40", &Value::Null).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(&app, "DELETE", "/api/v1/transactions/99", &Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, row) = call(&app, "GET", "/api/v1/transactions/40", &Value::Null).await;
    assert_eq!(row["manual"], false);
}
