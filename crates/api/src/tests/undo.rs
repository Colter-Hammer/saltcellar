//! Settings › Imports: the batches and Undo, on the imported fake export.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::import::{
    commit, create_all, fake_id, fake_plan, get_json, imported, listed, put, row_counts,
};
use super::{App, get, post, send, set_up, with};

/// Imports the fake export again, with `category` as Groceries.
async fn import_again(app: &App, category: i64) -> (StatusCode, Value) {
    let plan = fake_plan(app).await;
    let id = plan["id"].as_str().unwrap().to_owned();
    let body = json!({
        "file_name": "actual-export.zip",
        "accounts": create_all(&plan),
        "categories": [
            { "actual_id": fake_id(3, 1), "goes_to": "category", "category_id": category },
            { "actual_id": fake_id(3, 2), "goes_to": "transfer", "category_id": null },
        ],
    });
    commit(app, &id, &body).await
}

async fn undo(app: &App, batch: i64, body: &Value) -> (StatusCode, Value) {
    send(
        app.router.clone(),
        with(
            &app.cookie,
            post(&format!("/api/v1/imports/{batch}/undo"), body),
        ),
    )
    .await
}

/// The first batch's id.
async fn batch_id(app: &App) -> i64 {
    sqlx::query_scalar("SELECT id FROM import_batches ORDER BY id LIMIT 1")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

/// Row counts for the tables an import writes, which an undo returns to what they were: not the
/// categories (they stay) nor the batch itself (it stays, undone).
fn undone_tables(counts: Vec<(String, i64)>) -> Vec<(String, i64)> {
    counts
        .into_iter()
        .filter(|(table, _)| {
            !["categories", "category_groups", "import_batches"].contains(&table.as_str())
        })
        .collect()
}

#[tokio::test]
async fn undoing_the_import_leaves_no_rows_accounts_payees_snapshots_or_transfers() {
    let app = set_up().await;
    let before = undone_tables(row_counts(&app).await);
    let (_, total) = imported(&app).await;
    assert_ne!(undone_tables(row_counts(&app).await), before);
    let batch = batch_id(&app).await;
    let (status, body) = undo(&app, batch, &json!({ "confirm": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rows_deleted"], total);
    assert_eq!(body["accounts_deleted"], 5);
    assert_eq!(undone_tables(row_counts(&app).await), before);
    // Categories stay, and the batch is kept, undone.
    let categories: i64 = sqlx::query_scalar("SELECT count(*) FROM categories")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(categories, 1);
    let (status, list) = get_json(&app, "/api/v1/imports").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let batches = list["batches"].as_array().unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0]["state"], "undone");
    assert_eq!(batches[0]["format"], "actual");
    assert_eq!(batches[0]["file_name"], "actual-export.zip");
    // Undoing twice, or a batch that isn't there, or without a login.
    let (status, body) = undo(&app, batch, &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "already_undone");
    let (status, _) = undo(&app, 999, &json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let uri = format!("/api/v1/imports/{batch}/undo");
    let (status, _) = send(app.router.clone(), post(&uri, &json!({}))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_batches_list_says_who_and_how_many() {
    let app = set_up().await;
    let (status, list) = get_json(&app, "/api/v1/imports").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["batches"], json!([]));
    let (_, total) = imported(&app).await;
    let (_, list) = get_json(&app, "/api/v1/imports").await;
    let batch = &list["batches"][0];
    assert_eq!(batch["state"], "active");
    assert_eq!(batch["added"], total);
    assert_eq!(batch["matched"], 0);
    assert!(batch["imported_by"].as_str().is_some());
    assert!(batch["imported_at"].as_str().unwrap().ends_with('Z'));
    let (status, _) = send(app.router.clone(), get("/api/v1/imports")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A transaction's category and who set it.
async fn category_of(app: &App, id: i64) -> (Option<i64>, Option<String>) {
    sqlx::query_as("SELECT category_id, categorized_by FROM transactions WHERE id = ?")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn undoing_after_a_row_was_moved_asks_first_and_cancelling_changes_nothing() {
    let app = set_up().await;
    let (category, _) = imported(&app).await;
    let batch = batch_id(&app).await;
    let before = row_counts(&app).await;
    let id = listed(&app, "undecided=true").await[0];
    let uri = format!("/api/v1/transactions/{id}/category");
    let body = json!({ "category_id": category });
    let (status, _) = send(app.router.clone(), with(&app.cookie, put(&uri, &body))).await;
    assert_eq!(status, StatusCode::OK);
    let edited = category_of(&app, id).await;

    for body in [json!({}), json!({ "confirm": false })] {
        let (status, error) = undo(&app, batch, &body).await;
        assert_eq!(status, StatusCode::CONFLICT, "{error}");
        assert_eq!(error["error"]["code"], "rows_edited");
        let message = error["error"]["message"].as_str().unwrap();
        assert!(message.starts_with("1 of its rows"), "{message}");
    }
    assert_eq!(row_counts(&app).await, before);
    assert_eq!(category_of(&app, id).await, edited);

    let (status, body) = undo(&app, batch, &json!({ "confirm": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM transactions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

/// The accounts' names and opening balances.
async fn openings(app: &App) -> Vec<(String, Option<i64>, Option<String>)> {
    sqlx::query_as("SELECT name, opening_balance_minor, opening_date FROM accounts ORDER BY name")
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn import_undo_import_gives_the_first_result_again() {
    let app = set_up().await;
    let (category, _) = imported(&app).await;
    let first = undone_tables(row_counts(&app).await);
    let first_openings = openings(&app).await;
    let batch = batch_id(&app).await;
    let (status, _) = undo(&app, batch, &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    // The importer takes a new upload again.
    let (status, body) = import_again(&app, category).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(undone_tables(row_counts(&app).await), first);
    assert_eq!(openings(&app).await, first_openings);
    let (_, list) = get_json(&app, "/api/v1/imports").await;
    let kinds: Vec<&str> = list["batches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|batch| batch["state"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["active", "undone"]);
}
