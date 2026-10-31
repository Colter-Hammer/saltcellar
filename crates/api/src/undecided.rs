//! From Actual, undecided (docs/spec/m1-ledger-import.md#from-actual-undecided): the imported rows
//! whose Actual category was left on "Decide later", grouped by that category, and the category
//! they're given here, one row or a whole group at a time. The only edit milestone 1 allows.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{Body, ErrorEnvelope, Id};
use crate::session::Current;
use crate::transactions::Transaction;
use crate::{ApiError, AppState};

/// The undecided rows that came in under one Actual category.
#[derive(Serialize, ToSchema)]
pub struct UndecidedGroup {
    /// The category's name in Actual; empty when the export kept none.
    actual_category: String,
    rows: u32,
    /// Their amounts summed, in minor units: negative is money out.
    total_minor: i64,
}

/// The groups of undecided rows.
#[derive(Serialize, ToSchema)]
pub struct UndecidedList {
    /// By Actual category name, ignoring case; empty once nothing is left to decide.
    groups: Vec<UndecidedGroup>,
}

/// `GET /api/v1/undecided`: the undecided rows grouped by their Actual category, with each group's
/// count and total.
#[utoipa::path(
    get,
    path = "/api/v1/undecided",
    operation_id = "getUndecided",
    responses(
        (status = 200, description = "The groups.", body = UndecidedList),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<UndecidedList>, ApiError> {
    let groups = db::undecided_groups(&state.pool).await?;
    Ok(Json(UndecidedList {
        groups: groups
            .into_iter()
            .map(|group| UndecidedGroup {
                actual_category: group.actual_category,
                rows: u32::try_from(group.rows).unwrap_or_default(),
                total_minor: group.total.minor(),
            })
            .collect(),
    }))
}

/// Where to put a group's rows, or one row.
#[derive(Deserialize, ToSchema)]
pub struct Decide {
    /// An app category's id.
    category_id: i64,
}

/// Where to put every undecided row of one Actual category.
#[derive(Deserialize, ToSchema)]
pub struct MoveGroup {
    /// The group, as `GET /api/v1/undecided` names it.
    actual_category: String,
    /// An app category's id.
    category_id: i64,
}

/// How many rows moved.
#[derive(Serialize, ToSchema)]
pub struct Moved {
    moved: u32,
}

fn refused(error: db::DecideError) -> ApiError {
    match error {
        db::DecideError::NoSuchCategory => ApiError::validation(
            "That category doesn't exist.",
            BTreeMap::from([(
                "category_id".to_owned(),
                "There's no such category.".to_owned(),
            )]),
        ),
        db::DecideError::NotUndecided => ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There are no such undecided rows.",
        ),
        db::DecideError::Db(error) => error.into(),
    }
}

/// `POST /api/v1/undecided/move`: gives every undecided row of an Actual category the category, in
/// one database transaction. Each row records `categorized_by = 'user'` and the reason "Set by
/// <display name>"; they leave the list.
#[utoipa::path(
    post,
    path = "/api/v1/undecided/move",
    operation_id = "moveUndecided",
    request_body = MoveGroup,
    responses(
        (status = 200, description = "The rows have the category.", body = Moved),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: no undecided rows are left under that Actual category.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: there's no such category.", body = ErrorEnvelope),
    ),
)]
pub async fn move_group(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Body(body): Body<MoveGroup>,
) -> Result<Json<Moved>, ApiError> {
    let decision = db::Decision {
        category_id: body.category_id,
        user_id: current.login.user_id,
        display_name: &current.login.display_name,
        at: state.now(),
    };
    let moved = db::decide_group(&state.pool, &body.actual_category, &decision)
        .await
        .map_err(refused)?;
    Ok(Json(Moved {
        moved: u32::try_from(moved).unwrap_or(u32::MAX),
    }))
}

/// `PUT /api/v1/transactions/{id}/category`: gives one undecided row its category, as above.
#[utoipa::path(
    put,
    path = "/api/v1/transactions/{id}/category",
    operation_id = "setTransactionCategory",
    params(("id" = i64, Path, description = "The transaction.")),
    request_body = Decide,
    responses(
        (status = 200, description = "The transaction, with its category.", body = Transaction),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such transaction, or it isn't an undecided row.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: there's no such category.", body = ErrorEnvelope),
    ),
)]
pub async fn set_category(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Id(id): Id,
    Body(body): Body<Decide>,
) -> Result<Json<Transaction>, ApiError> {
    let decision = db::Decision {
        category_id: body.category_id,
        user_id: current.login.user_id,
        display_name: &current.login.display_name,
        at: state.now(),
    };
    db::decide_row(&state.pool, id, &decision)
        .await
        .map_err(refused)?;
    let detail = db::transaction(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::internal(&"a transaction vanished while it was categorized"))?;
    Ok(Json(detail.into()))
}
