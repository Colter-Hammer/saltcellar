//! Settings › Categories through the API.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};

use super::{App, get, send, set_up, with};

fn json_request(method: &str, uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Sends `method uri` with `body` as the logged-in login.
async fn call(app: &App, method: &str, uri: &str, body: &Value) -> (StatusCode, Value) {
    let request = if method == "GET" {
        get(uri)
    } else {
        json_request(method, uri, body)
    };
    send(app.router.clone(), with(&app.cookie, request)).await
}

async fn groups(app: &App) -> Vec<Value> {
    let (status, body) = call(app, "GET", "/api/v1/categories", &Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    body["groups"].as_array().unwrap().clone()
}

/// Each group as `name: category, category`, hidden ones marked with a trailing `*`.
async fn outline(app: &App) -> Vec<String> {
    let mark = |v: &Value| {
        let name = v["name"].as_str().unwrap().to_owned();
        if v["hidden"] == json!(true) {
            format!("{name}*")
        } else {
            name
        }
    };
    groups(app)
        .await
        .iter()
        .map(|g| {
            let categories: Vec<String> = g["categories"]
                .as_array()
                .unwrap()
                .iter()
                .map(mark)
                .collect();
            format!("{}: {}", mark(g), categories.join(", "))
        })
        .collect()
}

async fn add_group(app: &App, name: &str, kind: &str) -> i64 {
    let (status, body) = call(
        app,
        "POST",
        "/api/v1/category-groups",
        &json!({ "name": name, "kind": kind }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_i64().unwrap()
}

async fn add_category(app: &App, group_id: i64, name: &str) -> i64 {
    let (status, body) = call(
        app,
        "POST",
        "/api/v1/categories",
        &json!({ "group_id": group_id, "name": name }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_i64().unwrap()
}

#[tokio::test]
async fn categories_need_a_login() {
    let app = set_up().await;
    let (status, body) = send(app.router.clone(), get("/api/v1/categories")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "login_required");
}

#[tokio::test]
async fn the_starter_set_is_twelve_groups_in_order_with_income_last() {
    let app = set_up().await;
    assert_eq!(groups(&app).await, Vec::<Value>::new());
    let (status, _) = call(&app, "POST", "/api/v1/categories/starter", &json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let groups = groups(&app).await;
    assert_eq!(groups.len(), 12);
    assert_eq!(
        groups[0],
        json!({
            "id": groups[0]["id"],
            "name": "Housing",
            "kind": "expense",
            "hidden": false,
            "categories": [
                { "id": groups[0]["categories"][0]["id"], "name": "Mortgage & Rent", "hidden": false },
                { "id": groups[0]["categories"][1]["id"], "name": "Utilities", "hidden": false },
                { "id": groups[0]["categories"][2]["id"], "name": "Internet & Phone", "hidden": false },
                { "id": groups[0]["categories"][3]["id"], "name": "Home Maintenance", "hidden": false },
            ],
        })
    );
    let kinds: Vec<(&str, &str)> = groups
        .iter()
        .map(|g| (g["name"].as_str().unwrap(), g["kind"].as_str().unwrap()))
        .filter(|(_, kind)| *kind == "income")
        .collect();
    assert_eq!(kinds, [("Income", "income")]);
    assert_eq!(groups[11]["name"], "Income");

    let always: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payees WHERE review_mode = 'always'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(always, 9);

    // Only for an empty list.
    let (status, body) = call(&app, "POST", "/api/v1/categories/starter", &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "categories_exist");
}

#[tokio::test]
async fn groups_and_categories_are_added_renamed_and_hidden() {
    let app = set_up().await;
    let bills = add_group(&app, "  Bills ", "expense").await;
    let pay = add_group(&app, "Pay", "income").await;
    let water = add_category(&app, bills, "Water").await;
    add_category(&app, pay, "Salary").await;
    assert_eq!(outline(&app).await, ["Bills: Water", "Pay: Salary"]);
    assert_eq!(groups(&app).await[1]["kind"], "income");

    let uri = format!("/api/v1/categories/{water}");
    let (status, _) = call(&app, "PATCH", &uri, &json!({ "name": "Sewer" })).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, "PATCH", &uri, &json!({ "hidden": true })).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let uri = format!("/api/v1/category-groups/{pay}");
    let (status, _) = call(
        &app,
        "PATCH",
        &uri,
        &json!({ "name": "Income", "hidden": true }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(outline(&app).await, ["Bills: Sewer*", "Income*: Salary"]);
}

#[tokio::test]
async fn names_are_checked() {
    let app = set_up().await;
    let bills = add_group(&app, "Bills", "expense").await;
    add_category(&app, bills, "Water").await;
    let power = add_category(&app, bills, "Power").await;

    let power_uri = format!("/api/v1/categories/{power}");
    let long = "x".repeat(65);
    for (method, uri, body, message) in [
        (
            "POST",
            "/api/v1/category-groups",
            json!({ "name": " ", "kind": "expense" }),
            "Enter a name.",
        ),
        (
            "POST",
            "/api/v1/category-groups",
            json!({ "name": "Bills", "kind": "income" }),
            "There's already a group called Bills.",
        ),
        (
            "POST",
            "/api/v1/categories",
            json!({ "group_id": bills, "name": long }),
            "Use at most 64 characters.",
        ),
        (
            "PATCH",
            power_uri.as_str(),
            json!({ "name": "Water" }),
            "This group already has a category called Water.",
        ),
    ] {
        let (status, body) = call(&app, method, uri, &body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{uri} {body}");
        assert_eq!(body["error"]["fields"]["name"], message, "{uri}");
    }
    assert_eq!(outline(&app).await, ["Bills: Water, Power"]);
}

#[tokio::test]
async fn unknown_ids_and_kinds_are_refused() {
    let app = set_up().await;
    for (method, uri, body) in [
        ("PATCH", "/api/v1/categories/99", json!({ "hidden": true })),
        ("DELETE", "/api/v1/categories/99", json!({})),
        (
            "PATCH",
            "/api/v1/category-groups/99",
            json!({ "name": "X" }),
        ),
        ("DELETE", "/api/v1/category-groups/99", json!({})),
        ("DELETE", "/api/v1/category-groups/abc", json!({})),
        (
            "POST",
            "/api/v1/categories",
            json!({ "group_id": 99, "name": "X" }),
        ),
    ] {
        let (status, body) = call(&app, method, uri, &body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        assert_eq!(body["error"]["code"], "not_found", "{method} {uri}");
    }
    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/category-groups",
        &json!({ "name": "Odd", "kind": "savings" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_body");
}

#[tokio::test]
async fn a_group_cant_be_deleted_while_it_holds_categories() {
    let app = set_up().await;
    let bills = add_group(&app, "Bills", "expense").await;
    let water = add_category(&app, bills, "Water").await;
    let group_uri = format!("/api/v1/category-groups/{bills}");
    let (status, body) = call(&app, "DELETE", &group_uri, &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "group_not_empty");

    // Nothing uses a category yet, so any can be deleted; then the empty group can be.
    let uri = format!("/api/v1/categories/{water}");
    let (status, _) = call(&app, "DELETE", &uri, &json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, "DELETE", &group_uri, &json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(outline(&app).await, Vec::<String>::new());
}

#[tokio::test]
async fn the_order_is_saved_moving_categories_between_groups() {
    let app = set_up().await;
    let bills = add_group(&app, "Bills", "expense").await;
    let fun = add_group(&app, "Fun", "expense").await;
    let water = add_category(&app, bills, "Water").await;
    let power = add_category(&app, bills, "Power").await;
    let games = add_category(&app, fun, "Games").await;

    let order = json!({ "groups": [
        { "id": fun, "categories": [games, power] },
        { "id": bills, "categories": [water] },
    ] });
    let (status, _) = call(&app, "PUT", "/api/v1/categories/order", &order).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(outline(&app).await, ["Fun: Games, Power", "Bills: Water"]);

    // A category in two groups, or one left out, changes nothing.
    for groups in [
        json!([{ "id": fun, "categories": [games, power] }, { "id": bills, "categories": [water, power] }]),
        json!([{ "id": fun, "categories": [games] }, { "id": bills, "categories": [water] }]),
    ] {
        let (status, body) = call(
            &app,
            "PUT",
            "/api/v1/categories/order",
            &json!({ "groups": groups }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "categories_changed");
    }
    assert_eq!(outline(&app).await, ["Fun: Games, Power", "Bills: Water"]);

    // Moving a category onto a name its new group already has is refused.
    let water_in_fun = add_category(&app, fun, "Water").await;
    let order = json!({ "groups": [
        { "id": fun, "categories": [games, power, water_in_fun, water] },
        { "id": bills, "categories": [] },
    ] });
    let (status, body) = call(&app, "PUT", "/api/v1/categories/order", &order).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "name_taken");
}
