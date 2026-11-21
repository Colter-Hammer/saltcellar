//! Import from Actual over a synced account, through the API against the fake SimpleFIN server
//! (docs/spec/m2-bank-data.md#import-from-actual-becomes-overlap-aware): the overlap export's
//! accounts and rows carry the fake Bridge's ids, so a sync then an import (or an import then a
//! sync) keeps one copy of each row.

use axum::http::StatusCode;
use serde_json::{Value, json};
use simplefin_fake::Scenario;

use super::import::{as_started, commit, fixture, plan, upload};
use super::sync::{claimed_app, sync_now, track};
use super::{App, post, send, with};

/// The overlap export's ids for its categories, as the fake builder makes them.
fn category(n: u32) -> String {
    format!("{:08x}-0000-4000-8000-{n:012x}", 3)
}

/// App categories for the overlap export's: Groceries, Rent, Utilities, Paycheck, Interest.
async fn categories(app: &App) -> Vec<Value> {
    sqlx::query(
        "INSERT INTO category_groups (id, name) VALUES (80, 'Everyday');
         INSERT INTO categories (id, group_id, name) VALUES
           (81, 80, 'Groceries'), (82, 80, 'Rent'), (83, 80, 'Utilities'),
           (84, 80, 'Paycheck'), (85, 80, 'Interest');",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    (1..=5)
        .map(|n| json!({ "actual_id": category(n), "goes_to": "category", "category_id": 80 + n }))
        .collect()
}

/// Uploads the overlap export and reads its accounts back.
async fn overlap_plan(app: &App) -> Value {
    let (status, upload) = upload(app, fixture("actual-export-overlap.zip")).await;
    assert_eq!(status, StatusCode::CREATED, "{upload}");
    let (status, body) = plan(app, upload["id"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn preview(app: &App, id: &str, body: &Value) -> (StatusCode, Value) {
    let uri = format!("/api/v1/imports/actual/{id}/preview");
    send(app.router.clone(), with(&app.cookie, post(&uri, body))).await
}

/// `(bank id, date, amount, text, category, how, notes, payee)` of an account's rows, by date then
/// id.
type Row = (
    Option<String>,
    String,
    i64,
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

async fn rows(app: &App, account: i64) -> Vec<Row> {
    sqlx::query_as(
        "SELECT t.external_id, t.date, t.amount_minor, t.raw_description, t.category_id,
                t.categorized_by, t.memo, p.name
         FROM transactions t LEFT JOIN payees p ON p.id = t.payee_id
         WHERE t.account_id = ? ORDER BY t.date, t.id",
    )
    .bind(account)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

async fn synced(app: &App) -> i64 {
    let checking = track(app, "ACT-demo-checking", false).await;
    let (status, run) = sync_now(app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    checking
}

#[tokio::test]
async fn importing_after_a_sync_lands_actuals_fields_on_the_synced_rows_without_duplicates() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    let checking = synced(&app).await;
    let goes_to = categories(&app).await;
    // The household set the rent row's category already.
    sqlx::query(
        "UPDATE transactions SET category_id = 85, categorized_by = 'user'
         WHERE external_id = 'TRN-demo-rent-1'",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let before = rows(&app, checking).await;
    assert_eq!(before.len(), 4);

    // Checking is pre-matched to the synced account by its id; savings, still waiting in the
    // prompt, to that bank account.
    let plan = overlap_plan(&app).await;
    let accounts = plan["accounts"].as_array().unwrap();
    assert_eq!(accounts[0]["same_as"], checking);
    assert_eq!(accounts[1]["same_as"], Value::Null);
    assert_eq!(accounts[1]["same_as_waiting"], "ACT-demo-savings");
    let waiting: Vec<&Value> = plan["waiting_accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|account| &account["external_account_id"])
        .collect();
    assert_eq!(
        waiting,
        [&json!("ACT-demo-card"), &json!("ACT-demo-savings")]
    );

    let id = plan["id"].as_str().unwrap();
    let body = json!({
        "file_name": "overlap.zip",
        "accounts": as_started(&plan),
        "categories": goes_to,
    });
    let (status, previewed) = preview(&app, id, &body).await;
    assert_eq!(status, StatusCode::OK, "{previewed}");
    assert_eq!(
        previewed,
        json!({ "new": 13, "already": 4, "check": [] }),
        "twelve older checking rows and the savings row are new"
    );

    let (status, done) = commit(&app, id, &body).await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    assert_eq!(done["matched"], 4);
    assert_eq!(done["accounts_created"], 1);
    let after = rows(&app, checking).await;
    assert_eq!(after.len(), 16);
    let shared: Vec<&Row> = after
        .iter()
        .filter(|row| row.1.as_str() >= "2026-10-01")
        .collect();
    // One copy of each synced row: the bank's date, amount and text; Actual's category, notes
    // and payee; the household's category on the rent row.
    let with_actuals = |row: &Row, category, how: &str, notes: Option<&str>, payee: &str| {
        let mut row = row.clone();
        row.4 = Some(category);
        row.5 = Some(how.to_owned());
        row.6 = notes.map(str::to_owned);
        row.7 = Some(payee.to_owned());
        row
    };
    assert_eq!(
        shared.into_iter().cloned().collect::<Vec<Row>>(),
        [
            with_actuals(
                &before[0],
                84,
                "import",
                Some("October pay"),
                "Demo Employer Ltd"
            ),
            with_actuals(&before[1], 85, "user", None, "Demo Properties"),
            with_actuals(
                &before[2],
                81,
                "import",
                Some("Weekly shop #food"),
                "Neighbourhood Grocer"
            ),
            with_actuals(&before[3], 83, "import", Some("#bills"), "Demo Power"),
        ]
    );

    // Savings is tracked now, with the role chosen, and the next sync finds its imported row by id.
    let savings = done["accounts"][1]["id"].as_i64().unwrap();
    let (role, decision): (String, String) = sqlx::query_as(
        "SELECT a.role, ca.decision FROM accounts a
         JOIN connection_accounts ca ON ca.account_id = a.id WHERE a.id = ?",
    )
    .bind(savings)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!((role.as_str(), decision.as_str()), ("budgeted", "tracked"));
    let (status, run) = sync_now(&app).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(rows(&app, savings).await.len(), 1);
    assert_eq!(rows(&app, checking).await.len(), 16);
}

#[tokio::test]
async fn a_second_import_adds_nothing_and_undo_restores_the_synced_rows_exactly() {
    let (_server, app) = claimed_app(Scenario::Steady).await;
    let checking = synced(&app).await;
    let goes_to = categories(&app).await;
    let before = rows(&app, checking).await;

    let import = |accounts: Vec<Value>| json!({ "file_name": "overlap.zip", "accounts": accounts, "categories": goes_to });
    let first = overlap_plan(&app).await;
    let (status, done) = commit(
        &app,
        first["id"].as_str().unwrap(),
        &import(as_started(&first)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{done}");
    let imported = rows(&app, checking).await;

    // The second time, savings is the account the first import tracked.
    let second = overlap_plan(&app).await;
    let accounts = as_started(&second);
    assert!(
        accounts.iter().all(|a| a["action"] == "same"),
        "{accounts:?}"
    );
    let (status, again) = commit(&app, second["id"].as_str().unwrap(), &import(accounts)).await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert_eq!(again["matched"], 17);
    assert!(
        again["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["rows"] == 0),
        "{again}"
    );
    assert_eq!(rows(&app, checking).await, imported);

    // Undo the second, then the first: the synced rows are as the bank left them.
    for batch in [&again["batch_id"], &done["batch_id"]] {
        let uri = format!("/api/v1/imports/{batch}/undo");
        let (status, body) = send(
            app.router.clone(),
            with(&app.cookie, post(&uri, &json!({ "confirm": true }))),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    assert_eq!(rows(&app, checking).await, before);
}
