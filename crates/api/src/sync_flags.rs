//! The household's answers to sync flags on rows, and **Recently dropped by bank**
//! (docs/spec/m2-bank-data.md#sync-health-in-the-ui and #rows-the-bank-stops-sending).
//!
//! Every flag's action clears it: **Merge** / **Keep both** a possible duplicate, **Yes** / **No**
//! a possible manual match, **Keep** a row not seen at the bank (its **Delete** is the row's
//! `DELETE`), and fixing the split whose amount the bank changed. Edited pending rows the bank
//! dropped are listed for 30 days, each with **Restore**.

use std::collections::BTreeMap;

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use domain::Money;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{Body, ErrorEnvelope, Id};
use crate::session::Current;
use crate::transactions::Transaction;
use crate::{ApiError, AppState};

/// The routes, behind the login.
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/transactions/{id}/resolve", post(resolve))
        .route("/transactions/{id}/splits", put(set_splits))
        .route("/dropped", get(dropped))
        .route("/dropped/{id}/restore", post(restore))
}

fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "not_found",
        "There's no such transaction.",
    )
}

/// The household's answer to a row's sync flag.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlagAction {
    /// `possible_duplicate`: the same transaction. This row, the newer, stays, with the household's
    /// fields from the row it points at, which is deleted.
    Merge,
    /// `possible_duplicate`: two transactions.
    KeepBoth,
    /// `possible_manual_match`: it is that manual entry. This row, with the bank's amount, stays,
    /// with the manual row's fields; the manual row is deleted.
    AcceptMatch,
    /// `possible_manual_match`: it isn't.
    RejectMatch,
    /// `not_seen_at_bank`: keep it; sync stops asking.
    Keep,
}

/// What to do about a row's sync flag.
#[derive(Deserialize, ToSchema)]
pub struct Resolve {
    action: FlagAction,
}

/// `POST /api/v1/transactions/{id}/resolve`: answers the row's sync flag, which goes.
#[utoipa::path(
    post,
    path = "/api/v1/transactions/{id}/resolve",
    operation_id = "resolveSyncFlag",
    params(("id" = i64, Path, description = "The flagged transaction.")),
    request_body = Resolve,
    responses(
        (status = 200, description = "The transaction, its flag answered.", body = Transaction),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such transaction.", body = ErrorEnvelope),
        (status = 409, description = "`not_flagged`: it doesn't have the flag that action answers (any more).", body = ErrorEnvelope),
    ),
)]
pub async fn resolve(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Id(id): Id,
    Body(body): Body<Resolve>,
) -> Result<Json<Transaction>, ApiError> {
    let resolution = match body.action {
        FlagAction::Merge => db::Resolution::Merge,
        FlagAction::KeepBoth => db::Resolution::KeepBoth,
        FlagAction::AcceptMatch => db::Resolution::AcceptMatch,
        FlagAction::RejectMatch => db::Resolution::RejectMatch,
        FlagAction::Keep => db::Resolution::Keep,
    };
    let kept = db::resolve_sync_flag(
        &state.pool,
        id,
        resolution,
        current.login.user_id,
        state.now(),
    )
    .await
    .map_err(|error| match error {
        db::ResolveError::NotFound => not_found(),
        db::ResolveError::NotFlagged => ApiError::new(
            StatusCode::CONFLICT,
            "not_flagged",
            "This transaction doesn't need that any more.",
        ),
        db::ResolveError::Db(error) => error.into(),
    })?;
    crate::transactions::get(State(state), Id(kept)).await
}

/// A split line as the household fixes it.
#[derive(Deserialize, ToSchema)]
pub struct SplitLineEntry {
    category_id: i64,
    /// In minor units, the same way round as the transaction's amount.
    amount_minor: i64,
    #[serde(default)]
    memo: Option<String>,
}

/// A split transaction's new lines.
#[derive(Deserialize, ToSchema)]
pub struct SplitEntry {
    /// They add up to the transaction's amount. One line leaves it unsplit, in that category.
    lines: Vec<SplitLineEntry>,
}

/// `PUT /api/v1/transactions/{id}/splits`: replaces a split transaction's lines, as fixing an
/// `amount_changed_split` flag does; the flag goes. Review takes this over in milestone 3.
#[utoipa::path(
    put,
    path = "/api/v1/transactions/{id}/splits",
    operation_id = "setTransactionSplits",
    params(("id" = i64, Path, description = "The split transaction.")),
    request_body = SplitEntry,
    responses(
        (status = 200, description = "The transaction, its split fixed.", body = Transaction),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such transaction.", body = ErrorEnvelope),
        (status = 409, description = "`not_split`: it isn't split.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: `lines` don't add up to the amount, or name a category there isn't.", body = ErrorEnvelope),
    ),
)]
pub async fn set_splits(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Id(id): Id,
    Body(body): Body<SplitEntry>,
) -> Result<Json<Transaction>, ApiError> {
    let lines: Vec<db::NewSplitLine> = body
        .lines
        .into_iter()
        .map(|line| db::NewSplitLine {
            category_id: line.category_id,
            amount: Money::from_minor(line.amount_minor),
            memo: line
                .memo
                .map(|memo| memo.trim().to_owned())
                .filter(|memo| !memo.is_empty()),
        })
        .collect();
    let invalid = |message: &str| {
        ApiError::validation(
            "Check the split.",
            BTreeMap::from([("lines".to_owned(), message.to_owned())]),
        )
    };
    db::set_splits(&state.pool, id, &lines, current.login.user_id, state.now())
        .await
        .map_err(|error| match error {
            db::SplitError::NotFound => not_found(),
            db::SplitError::NotSplit => ApiError::new(
                StatusCode::CONFLICT,
                "not_split",
                "This transaction isn't split.",
            ),
            db::SplitError::DontAddUp => invalid("The lines must add up to the amount."),
            db::SplitError::NoSuchCategory => invalid("Choose a category for every line."),
            db::SplitError::Db(error) => error.into(),
        })?;
    crate::transactions::get(State(state), Id(id)).await
}

/// A row the bank dropped, as **Recently dropped by bank** lists it.
#[derive(Serialize, ToSchema)]
pub struct DroppedRow {
    /// The dropped row's id, for **Restore**.
    id: i64,
    account_id: i64,
    account: String,
    /// `YYYY-MM-DD`.
    date: String,
    /// In minor units: negative is money out, positive money in.
    amount_minor: i64,
    #[schema(required = true)]
    payee: Option<String>,
    raw_description: String,
    /// Null when it was split or uncategorized.
    #[schema(required = true)]
    category: Option<String>,
    split: bool,
    #[schema(required = true)]
    memo: Option<String>,
    /// RFC 3339, UTC. **Restore** works for 30 days after.
    dropped_at: String,
}

/// Every row listed under Recently dropped by bank.
#[derive(Serialize, ToSchema)]
pub struct DroppedList {
    /// The latest dropped first.
    rows: Vec<DroppedRow>,
}

/// `GET /api/v1/dropped`: **Recently dropped by bank**, the pending rows the household had
/// edited that the bank stopped sending, from the last 30 days.
#[utoipa::path(
    get,
    path = "/api/v1/dropped",
    operation_id = "getDropped",
    responses(
        (status = 200, description = "The rows, the latest dropped first.", body = DroppedList),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn dropped(State(state): State<AppState>) -> Result<Json<DroppedList>, ApiError> {
    let rows = db::dropped_rows(&state.pool, state.now()).await?;
    Ok(Json(DroppedList {
        rows: rows
            .into_iter()
            .map(|row| DroppedRow {
                id: row.id,
                account_id: row.account_id,
                account: row.account,
                date: row.date.to_string(),
                amount_minor: row.amount.minor(),
                payee: row.payee,
                raw_description: row.raw_description,
                category: row.category,
                split: row.split,
                memo: row.memo,
                dropped_at: row.dropped_at.to_string(),
            })
            .collect(),
    }))
}

/// `POST /api/v1/dropped/{id}/restore`: brings a dropped row back with the household's edits.
/// Sync won't drop it again; the bank sending it again or posting it still matches it.
#[utoipa::path(
    post,
    path = "/api/v1/dropped/{id}/restore",
    operation_id = "restoreDropped",
    params(("id" = i64, Path, description = "The dropped row.")),
    responses(
        (status = 200, description = "The transaction, back.", body = Transaction),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: no such row is listed: restored already, or dropped more than 30 days ago.", body = ErrorEnvelope),
        (status = 409, description = "`back_from_bank`: the bank has sent it again; it's in the list.", body = ErrorEnvelope),
    ),
)]
pub async fn restore(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Id(id): Id,
) -> Result<Json<Transaction>, ApiError> {
    let restored = db::restore_dropped(&state.pool, id, current.login.user_id, state.now())
        .await
        .map_err(|error| match error {
            db::RestoreError::NotFound => ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "That row can't be restored any more.",
            ),
            db::RestoreError::BackFromBank => ApiError::new(
                StatusCode::CONFLICT,
                "back_from_bank",
                "The bank sent it again: it's in your transactions.",
            ),
            db::RestoreError::Db(error) => error.into(),
        })?;
    crate::transactions::get(State(state), Id(restored)).await
}
