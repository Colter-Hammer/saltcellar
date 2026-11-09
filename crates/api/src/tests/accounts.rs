//! Creating, editing and ordering accounts, and manual balances, through the API.

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

async fn names(app: &App) -> Vec<String> {
    let (_, body) = call(app, "GET", "/api/v1/accounts", &Value::Null).await;
    body["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn edits_an_account_and_refuses_a_name_taken_in_any_case() {
    let app = set_up().await;
    for (name, kind) in [("Checking", "checking"), ("Savings", "savings")] {
        sqlx::query("INSERT INTO accounts (name, type, role) VALUES (?, ?, 'budgeted')")
            .bind(name)
            .bind(kind)
            .execute(&app.pool)
            .await
            .unwrap();
    }
    let (_, body) = call(&app, "GET", "/api/v1/accounts", &Value::Null).await;
    let id = body["accounts"][0]["id"].as_i64().unwrap();
    let uri = format!("/api/v1/accounts/{id}");

    let (status, body) = call(&app, "PATCH", &uri, &json!({"name": "savings"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation_failed");
    assert!(body["error"]["fields"]["name"].is_string());

    let change = json!({
        "name": "Everyday", "institution": "Fake Bank", "type": "cash", "role": "tracking",
        "balances_only": true, "in_net_worth": false, "closed": true,
    });
    let (status, _) = call(&app, "PATCH", &uri, &change).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, account) = call(&app, "GET", &uri, &Value::Null).await;
    assert_eq!(account["name"], "Everyday");
    assert_eq!(account["institution"], "Fake Bank");
    assert_eq!(account["type"], "cash");
    assert_eq!(account["role"], "tracking");
    assert_eq!(account["balances_only"], true);
    assert_eq!(account["in_net_worth"], false);
    assert_eq!(account["closed"], true);

    let (status, _) = call(
        &app,
        "PATCH",
        "/api/v1/accounts/999",
        &json!({"closed": false}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn orders_accounts_only_by_listing_every_one_once() {
    let app = set_up().await;
    for name in ["A", "B"] {
        sqlx::query("INSERT INTO accounts (name, type, role) VALUES (?, 'checking', 'budgeted')")
            .bind(name)
            .execute(&app.pool)
            .await
            .unwrap();
    }
    let (_, body) = call(&app, "GET", "/api/v1/accounts", &Value::Null).await;
    let ids: Vec<i64> = body["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_i64().unwrap())
        .collect();
    let (status, body) = call(
        &app,
        "PUT",
        "/api/v1/accounts/order",
        &json!({"accounts": [ids[0]]}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "accounts_changed");
    let (status, _) = call(
        &app,
        "PUT",
        "/api/v1/accounts/order",
        &json!({"accounts": [ids[1], ids[0]]}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(names(&app).await, ["B", "A"]);
}

#[tokio::test]
async fn a_manual_account_starts_from_whats_in_it_today_and_takes_new_balances() {
    let app = set_up().await;
    let wallet =
        json!({"name": "Wallet", "type": "cash", "role": "budgeted", "balance_minor": 4200});
    let (status, account) = call(&app, "POST", "/api/v1/accounts", &wallet).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(account["name"], "Wallet");
    assert_eq!(account["connection_id"], Value::Null);
    assert_eq!(account["balance_minor"], 4200);
    // Today in Berlin is 2026-10-28; the opening balance is dated the day before.
    assert_eq!(account["opening_balance_minor"], 4200);
    assert_eq!(account["opening_date"], "2026-10-27");
    assert_eq!(names(&app).await, ["Wallet"]);

    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/accounts",
        &json!({
            "name": "WALLET", "type": "cash", "role": "budgeted", "balance_minor": 0,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["name"].is_string());

    let id = account["id"].as_i64().unwrap();
    let uri = format!("/api/v1/accounts/{id}/balance");
    let (status, _) = call(&app, "POST", &uri, &json!({"balance_minor": 3150})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, account) = call(&app, "GET", &format!("/api/v1/accounts/{id}"), &Value::Null).await;
    assert_eq!(account["balance_minor"], 3150);
    // No rows, so the app's own balance is the opening one: drift of -10.50.
    assert_eq!(account["app_balance_minor"], 4200);
}

#[tokio::test]
async fn a_manual_balance_is_refused_on_a_connected_account() {
    let app = set_up().await;
    let connection = db::save_connection(&app.pool, b"sealed").await.unwrap();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO accounts (name, type, role, connection_id) VALUES ('Synced', 'checking', 'budgeted', ?)
         RETURNING id",
    )
    .bind(connection)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let body = json!({"balance_minor": 100});
    let (status, error) = call(
        &app,
        "POST",
        &format!("/api/v1/accounts/{id}/balance"),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "account_connected");
    let (_, account) = call(&app, "GET", &format!("/api/v1/accounts/{id}"), &Value::Null).await;
    assert_eq!(account["connection_id"], connection);
    assert_eq!(account["balance_minor"], Value::Null);
    let (status, _) = call(&app, "POST", "/api/v1/accounts/999/balance", &body).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
