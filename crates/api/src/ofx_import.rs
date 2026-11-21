//! OFX, QFX and QBO import (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import). Like the CSV import,
//! each step sends the file's text again and the server keeps nothing between them: **read** says
//! which account the file is for (pre-selected by its account id's last 4 digits), **preview**
//! counts what Import would do, **commit** writes it as one import batch (`format = 'ofx'`). There
//! is no mapping: the file says what each field is, and each transaction's FITID is its
//! `import_ref`.

use std::collections::BTreeSet;

use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Extension, State};
use axum::http::StatusCode;
use axum::routing::post;
use domain::csv_import::Read;
use domain::ofx;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::csv_import::{CsvImportDone, CsvPreview, MAX_CSV_BYTES, check_account, preview_of};
use crate::error::{Body as JsonBody, ErrorEnvelope};
use crate::session::Current;
use crate::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/imports/ofx/read", post(read))
        .route("/imports/ofx/preview", post(preview))
        .route("/imports/ofx/commit", post(commit))
        .layer(DefaultBodyLimit::max(MAX_CSV_BYTES + 64 * 1024))
}

/// An OFX, QFX or QBO file, sent for reading.
#[derive(Deserialize, ToSchema)]
pub struct OfxReadRequest {
    /// The file's text, as is.
    content: String,
}

/// What a file holds, and the account it's likely for.
#[derive(Serialize, ToSchema)]
pub struct OfxRead {
    /// The last 4 digits of the bank's id for the account; null when the file has none.
    #[schema(required = true)]
    last_four: Option<String>,
    /// The one account whose name ends in those digits, to pre-select; null for none or several.
    #[schema(required = true)]
    account_id: Option<i64>,
    /// Transactions that read.
    transactions: usize,
    /// The first and last posting days, `YYYY-MM-DD`; null with no transactions.
    #[schema(required = true)]
    first_date: Option<String>,
    #[schema(required = true)]
    last_date: Option<String>,
}

/// A file and the account to import it into.
#[derive(Deserialize, ToSchema)]
pub struct OfxRequest {
    account_id: i64,
    content: String,
}

/// A file to import, with the household's decisions.
#[derive(Deserialize, ToSchema)]
pub struct OfxCommitRequest {
    account_id: i64,
    /// The file's name, for Settings › Imports.
    file_name: String,
    content: String,
    /// The places (`row`) of the rows to check that should be added; the rest are skipped.
    keep: Vec<usize>,
}

/// `POST /api/v1/imports/ofx/read`: reads a file's statement and finds its account.
#[utoipa::path(
    post,
    path = "/api/v1/imports/ofx/read",
    operation_id = "readOfxFile",
    request_body = OfxReadRequest,
    responses(
        (status = 200, description = "The statement's account digits, the account to pre-select and what it holds.", body = OfxRead),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 422, description = "`not_ofx`: no `<OFX>` element; `empty_file`: no statement; `several_accounts`: statements for more than one account.", body = ErrorEnvelope),
    ),
)]
pub async fn read(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<OfxReadRequest>,
) -> Result<Json<OfxRead>, ApiError> {
    let (account, read) = statement(&state, &body.content).await?;
    let last_four = ofx::last_four(&account);
    let accounts = db::import_accounts(&state.pool).await?;
    let account_id = last_four.as_deref().and_then(|last| {
        ofx::preselect(last, accounts.iter().map(|(id, name)| (*id, name.as_str())))
    });
    let dates = || read.rows.iter().map(|row| row.date);
    Ok(Json(OfxRead {
        last_four,
        account_id,
        transactions: read.rows.len(),
        first_date: dates().min().map(|date| date.to_string()),
        last_date: dates().max().map(|date| date.to_string()),
    }))
}

/// `POST /api/v1/imports/ofx/preview`: what Import would do with the file; nothing is written.
#[utoipa::path(
    post,
    path = "/api/v1/imports/ofx/preview",
    operation_id = "previewOfxImport",
    request_body = OfxRequest,
    responses(
        (status = 200, description = "How many rows are new, already there or to check.", body = CsvPreview),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such account.", body = ErrorEnvelope),
        (status = 422, description = "`not_ofx`, `empty_file` or `several_accounts`, as for read.", body = ErrorEnvelope),
    ),
)]
pub async fn preview(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<OfxRequest>,
) -> Result<Json<CsvPreview>, ApiError> {
    check_account(&state, body.account_id).await?;
    let (_, read) = statement(&state, &body.content).await?;
    Ok(Json(preview_of(&state, body.account_id, read).await?))
}

/// `POST /api/v1/imports/ofx/commit`: imports the file as one import batch.
#[utoipa::path(
    post,
    path = "/api/v1/imports/ofx/commit",
    operation_id = "commitOfxImport",
    request_body = OfxCommitRequest,
    responses(
        (status = 201, description = "Imported, as one import batch.", body = CsvImportDone),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such account.", body = ErrorEnvelope),
        (status = 422, description = "`not_ofx`, `empty_file`, `several_accounts`, or `nothing_readable`: no transaction reads.", body = ErrorEnvelope),
    ),
)]
pub async fn commit(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    JsonBody(body): JsonBody<OfxCommitRequest>,
) -> Result<(StatusCode, Json<CsvImportDone>), ApiError> {
    check_account(&state, body.account_id).await?;
    let (_, read) = statement(&state, &body.content).await?;
    if read.rows.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "nothing_readable",
            "No transaction in this file reads.",
        ));
    }
    let file_name = match body.file_name.trim() {
        "" => "file.ofx",
        name => name,
    };
    let keep: BTreeSet<usize> = body.keep.into_iter().collect();
    let done = db::commit_ofx_import(
        &state.pool,
        db::OfxCommit {
            account_id: body.account_id,
            file_name,
            unreadable: read.unreadable.len() as u64,
            rows: read.rows,
            keep: &keep,
            imported_by: current.login.user_id,
            today: state.today().await?,
        },
    )
    .await?;
    tracing::info!(batch = done.batch_id, "imported an OFX file");
    Ok((StatusCode::CREATED, Json(done.into())))
}

/// The file's one statement: its account id and its transactions.
async fn statement(state: &AppState, content: &str) -> Result<(String, Read), ApiError> {
    let household = db::household(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
    let unprocessable =
        |code, message| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, code, message);
    let mut statements = ofx::parse(content, household.currency.decimal_places())
        .map_err(|_| unprocessable("not_ofx", "This isn't an OFX, QFX or QBO file."))?;
    // A file for several accounts is refused rather than split *(agent)*: banks download one
    // account at a time, and a batch is one account's.
    match statements.len() {
        0 => Err(unprocessable(
            "empty_file",
            "There's no statement in this file.",
        )),
        1 => {
            let statement = statements.remove(0);
            Ok((statement.account_id, statement.read))
        }
        _ => Err(unprocessable(
            "several_accounts",
            "This file holds more than one account. Download one account at a time.",
        )),
    }
}
