//! Settings › Imports (docs/spec/m1-ledger-import.md#settings--imports): the import batches, and
//! undoing one.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{Body, ErrorEnvelope, Id};
use crate::{ApiError, AppState};

/// One import batch.
#[derive(Serialize, ToSchema)]
pub struct ImportBatch {
    id: i64,
    /// When it was imported, as an RFC 3339 time in UTC.
    imported_at: String,
    /// `csv`, `ofx` or `actual`.
    format: String,
    file_name: String,
    /// The display name of who imported it; absent if that login is gone.
    #[schema(required = false)]
    imported_by: Option<String>,
    /// Rows it added.
    added: i64,
    /// Rows it matched to ones already there.
    matched: i64,
    /// `active`, or `undone`.
    state: String,
}

/// The batches, latest first.
#[derive(Serialize, ToSchema)]
pub struct ImportBatches {
    batches: Vec<ImportBatch>,
}

/// `GET /api/v1/imports`: every import batch, active or undone, latest first.
#[utoipa::path(
    get,
    path = "/api/v1/imports",
    operation_id = "listImportBatches",
    responses(
        (status = 200, description = "The batches.", body = ImportBatches),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<ImportBatches>, ApiError> {
    let batches = db::import_batches(&state.pool).await?;
    Ok(Json(ImportBatches {
        batches: batches
            .into_iter()
            .map(|batch| ImportBatch {
                id: batch.id,
                imported_at: batch.imported_at.to_string(),
                format: batch.format,
                file_name: batch.file_name,
                imported_by: batch.imported_by,
                added: batch.added,
                matched: batch.matched,
                state: if batch.undone_at.is_some() {
                    "undone"
                } else {
                    "active"
                }
                .to_owned(),
            })
            .collect(),
    }))
}

/// Whether to undo even if rows were edited since.
#[derive(Deserialize, ToSchema)]
pub struct UndoRequest {
    /// `true` once the person has been asked about the edited rows. Absent means `false`.
    #[serde(default)]
    confirm: bool,
}

/// What an undo removed.
#[derive(Serialize, ToSchema)]
pub struct Undone {
    rows_deleted: u64,
    accounts_deleted: u64,
}

fn refused(error: db::UndoError) -> ApiError {
    match error {
        db::UndoError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There's no such import.",
        ),
        db::UndoError::AlreadyUndone => ApiError::new(
            StatusCode::CONFLICT,
            "already_undone",
            "That import was already undone.",
        ),
        db::UndoError::Edited(count) => ApiError::new(
            StatusCode::CONFLICT,
            "rows_edited",
            format!(
                "{count} of its rows were edited since the import. Undoing it discards those edits."
            ),
        ),
        db::UndoError::Db(error) => error.into(),
    }
}

/// `POST /api/v1/imports/{id}/undo`: undoes an active batch in one database transaction: deletes
/// the rows it inserted with their split lines, tags and transfers, restores the fields it filled
/// on matched rows, deletes the accounts it created that have no rows left and the payees and tags
/// left unused, and marks it undone. Categories stay. If any inserted row was edited since, it
/// answers `409 rows_edited` and changes nothing until sent again with `confirm: true`.
#[utoipa::path(
    post,
    path = "/api/v1/imports/{id}/undo",
    operation_id = "undoImport",
    params(("id" = i64, Path, description = "The import batch.")),
    request_body = UndoRequest,
    responses(
        (status = 200, description = "The batch is undone.", body = Undone),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such import.", body = ErrorEnvelope),
        (status = 409, description = "`rows_edited`: rows were edited since and `confirm` wasn't set; or `already_undone`.", body = ErrorEnvelope),
    ),
)]
pub async fn undo(
    State(state): State<AppState>,
    Id(id): Id,
    Body(body): Body<UndoRequest>,
) -> Result<Json<Undone>, ApiError> {
    let undone = db::undo_import(&state.pool, id, body.confirm, state.now())
        .await
        .map_err(refused)?;
    Ok(Json(Undone {
        rows_deleted: undone.rows_deleted,
        accounts_deleted: undone.accounts_deleted,
    }))
}
