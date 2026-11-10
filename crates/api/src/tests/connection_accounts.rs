//! The new accounts prompt through the API, after claiming against the fake SimpleFIN server.

use axum::http::StatusCode;
use base64::Engine;
use serde_json::{Value, json};
use simplefin_fake::{FakeServer, Scenario};

use super::{App, get, post, send, set_up_with_bank, with};
use crate::{BankConfig, ClaimPolicy, SecretKey};

const URI: &str = "/api/v1/connection-accounts";

/// An install in EUR with the secret key set, and a fake Bridge whose accounts report EUR.
async fn connected_app(scenario: Scenario) -> (FakeServer, App) {
    let server = FakeServer::start(scenario).await.unwrap();
    server.set_currency("EUR");
    let key = base64::engine::general_purpose::STANDARD.encode([7_u8; 32]);
    let app = set_up_with_bank(BankConfig {
        secret_key: Some(SecretKey::from_base64(&key).unwrap()),
        claim_policy: ClaimPolicy::only_hosts([server.host()]),
    })
    .await;
    (server, app)
}

async fn claim(server: &FakeServer, app: &App) {
    let request = axum::http::Request::put("/api/v1/bank-connection")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({ "setup_token": server.setup_token() }).to_string(),
        ))
        .unwrap();
    let (status, body) = send(app.router.clone(), with(&app.cookie, request)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn prompt(app: &App) -> Value {
    let (status, body) = send(app.router.clone(), with(&app.cookie, get(URI))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn names(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect()
}

async fn act(app: &App, id: &str, action: &str, body: &Value) -> (StatusCode, Value) {
    let uri = format!("{URI}/{id}/{action}");
    send(app.router.clone(), with(&app.cookie, post(&uri, body))).await
}

async fn accounts(app: &App) -> Vec<Value> {
    let (_, body) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/accounts")),
    )
    .await;
    body["accounts"].as_array().unwrap().clone()
}

#[tokio::test]
async fn claiming_fills_the_prompt_and_track_ignore_and_revive_work() {
    let (server, app) = connected_app(Scenario::Steady).await;
    assert_eq!(
        prompt(&app).await,
        json!({ "pending": [], "ignored": [], "refused": [] })
    );
    claim(&server, &app).await;
    let seen = server.requests();
    assert_eq!(seen.len(), 2, "{seen:?}");
    let window = simplefin_fake::NOW - 45 * 86_400;
    assert_eq!(
        seen[1].query,
        format!("version=2&pending=1&start-date={window}")
    );

    let body = prompt(&app).await;
    assert_eq!(
        body["pending"][0],
        json!({
            "external_account_id": "ACT-demo-checking",
            "name": "Demo Everyday Checking",
            "institution": "First Demo Bank",
            "balances_only_suggested": false,
            "same_as_account_id": null,
        })
    );
    assert_eq!(
        names(&body["pending"]),
        [
            "Demo Everyday Checking",
            "Demo Rewards Card",
            "Demo Savings"
        ]
    );
    // Nothing counts until decided.
    assert_eq!(accounts(&app).await.len(), 0);

    let budgeted = json!({ "role": "budgeted", "balances_only": false });
    let (status, tracked) = act(&app, "ACT-demo-checking", "track", &budgeted).await;
    assert_eq!(status, StatusCode::OK, "{tracked}");
    let tracking = json!({ "role": "tracking", "balances_only": true });
    assert_eq!(
        act(&app, "ACT-demo-card", "track", &tracking).await.0,
        StatusCode::OK
    );
    assert_eq!(
        act(&app, "ACT-demo-savings", "ignore", &json!({})).await.0,
        StatusCode::NO_CONTENT
    );
    let (status, again) = act(&app, "ACT-demo-card", "track", &budgeted).await;
    assert_eq!(
        (status, again["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("already_tracked"))
    );
    let (status, _) = act(&app, "ACT-nope", "ignore", &json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let body = prompt(&app).await;
    assert_eq!(names(&body["pending"]), Vec::<&str>::new());
    assert_eq!(names(&body["ignored"]), ["Demo Savings"]);
    let listed = accounts(&app).await;
    let read: Vec<(&str, &str, &str, bool)> = listed
        .iter()
        .map(|a| {
            (
                a["name"].as_str().unwrap(),
                a["type"].as_str().unwrap(),
                a["role"].as_str().unwrap(),
                a["balances_only"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            ("Demo Everyday Checking", "checking", "budgeted", false),
            ("Demo Rewards Card", "credit_card", "tracking", true),
        ]
    );
    assert_eq!(listed[0]["id"], tracked["account_id"]);
    assert!(listed.iter().all(|a| a["connection_id"].is_i64()));

    // Start tracking… the ignored one.
    assert_eq!(
        act(&app, "ACT-demo-savings", "track", &tracking).await.0,
        StatusCode::OK
    );
    assert_eq!(
        prompt(&app).await,
        json!({ "pending": [], "ignored": [], "refused": [] })
    );
    assert_eq!(accounts(&app).await.len(), 3);
}

#[tokio::test]
async fn an_imported_account_with_the_provider_id_skips_the_prompt() {
    let (server, app) = connected_app(Scenario::Steady).await;
    sqlx::query(
        "INSERT INTO accounts (name, type, role, external_account_id)
         VALUES ('Imported card', 'credit_card', 'tracking', 'ACT-demo-card')",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    claim(&server, &app).await;
    assert_eq!(
        names(&prompt(&app).await["pending"]),
        ["Demo Everyday Checking", "Demo Savings"]
    );
    let listed = accounts(&app).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (&listed[0]["name"], &listed[0]["role"]),
        (&json!("Imported card"), &json!("tracking"))
    );
    assert!(listed[0]["connection_id"].is_i64());
}

#[tokio::test]
async fn same_as_moves_a_manual_account_onto_the_feed() {
    let (server, app) = connected_app(Scenario::Steady).await;
    let new = json!({ "name": "Card", "type": "credit_card", "role": "budgeted", "balance_minor": -5000 });
    let (status, made) = send(
        app.router.clone(),
        with(&app.cookie, post("/api/v1/accounts", &new)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    claim(&server, &app).await;
    let same =
        json!({ "role": "budgeted", "balances_only": false, "same_as_account_id": made["id"] });
    let (status, tracked) = act(&app, "ACT-demo-card", "track", &same).await;
    assert_eq!(status, StatusCode::OK, "{tracked}");
    assert_eq!(tracked["account_id"], made["id"]);
    let listed = accounts(&app).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["opening_balance_minor"], -5000);
    assert!(listed[0]["connection_id"].is_i64());

    let missing = json!({ "role": "budgeted", "balances_only": false, "same_as_account_id": 999 });
    let (status, _) = act(&app, "ACT-demo-savings", "track", &missing).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_account_in_another_currency_is_refused_with_a_message() {
    let (server, app) = connected_app(Scenario::ForeignCurrency).await;
    claim(&server, &app).await;
    let body = prompt(&app).await;
    assert_eq!(names(&body["pending"]).len(), 3);
    assert_eq!(
        body["refused"],
        json!([{
            "external_account_id": "ACT-demo-travel",
            "name": "Demo Travel Card",
            "institution": "Demo Credit Union",
            "currency": "CAD",
            "message": "Demo Travel Card reports its money in CAD, and this household keeps EUR, so it can't be tracked.",
        }])
    );
    let (status, refused) = act(
        &app,
        "ACT-demo-travel",
        "track",
        &json!({ "role": "budgeted", "balances_only": false }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused["error"]["code"], "foreign_currency");
}

#[tokio::test]
async fn a_revoked_feed_after_the_claim_pauses_the_connection() {
    let (server, app) = connected_app(Scenario::Revoked).await;
    claim(&server, &app).await;
    let (_, state) = send(
        app.router.clone(),
        with(&app.cookie, get("/api/v1/bank-connection")),
    )
    .await;
    assert_eq!(state["connection"]["status"], "token_revoked");
    assert_eq!(prompt(&app).await["pending"], json!([]));
}
