//! CSV import (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import). Each step sends the file's text
//! again, with the account and mapping, so the server keeps nothing between them: **read** shows
//! the first records and the mapping to use (the account's saved one for this header, else a
//! guess), **preview** counts what Import would do and writes nothing, **commit** writes it as one
//! import batch.

use std::collections::{BTreeSet, HashMap};

use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Extension, State};
use axum::http::StatusCode;
use axum::routing::post;
use domain::csv_import::{self as csv, AmountColumns, DateFormat, Entry, Outcome};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{Body as JsonBody, ErrorEnvelope};
use crate::session::Current;
use crate::{ApiError, AppState};

/// The most a file's text may be: 10 MiB *(agent)*, a few years of any account's rows.
pub const MAX_CSV_BYTES: usize = 10 * 1024 * 1024;

/// Records the mapping screen shows.
const SHOWN_RECORDS: usize = 30;
/// New rows the preview shows as a sample, and problems it lists.
const SHOWN_ROWS: usize = 10;
const SHOWN_PROBLEMS: usize = 20;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/imports/csv/read", post(read))
        .route("/imports/csv/preview", post(preview))
        .route("/imports/csv/commit", post(commit))
        .layer(DefaultBodyLimit::max(MAX_CSV_BYTES + 64 * 1024))
}

/// How the file writes its dates.
#[derive(Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum CsvDateFormat {
    /// `2026-03-04`.
    Ymd,
    /// `03/04/2026`.
    Mdy,
    /// `04/03/2026`.
    Dmy,
}

/// Where the amount is.
#[derive(Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CsvAmount {
    /// One column, signed.
    Signed { column: usize },
    /// Money out in one column, money in in another, both without a sign.
    DebitCredit { debit: usize, credit: usize },
}

/// Which column holds what. Columns count from 0.
#[derive(Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct CsvMapping {
    date: usize,
    /// The bank's posting date, if the file has one.
    #[schema(required = true)]
    post_date: Option<usize>,
    description: usize,
    amount: CsvAmount,
    /// The file writes money out as positive.
    flip_sign: bool,
    date_format: CsvDateFormat,
    /// The bank's own category, if the file has one.
    #[schema(required = true)]
    category: Option<usize>,
    #[schema(required = true)]
    memo: Option<usize>,
    /// Records before the first row, the header among them.
    skip_rows: usize,
}

impl From<CsvMapping> for csv::Mapping {
    fn from(mapping: CsvMapping) -> Self {
        csv::Mapping {
            date: mapping.date,
            post_date: mapping.post_date,
            description: mapping.description,
            amount: match mapping.amount {
                CsvAmount::Signed { column } => AmountColumns::Signed(column),
                CsvAmount::DebitCredit { debit, credit } => {
                    AmountColumns::DebitCredit { debit, credit }
                }
            },
            flip_sign: mapping.flip_sign,
            date_format: match mapping.date_format {
                CsvDateFormat::Ymd => DateFormat::YearMonthDay,
                CsvDateFormat::Mdy => DateFormat::MonthDayYear,
                CsvDateFormat::Dmy => DateFormat::DayMonthYear,
            },
            category: mapping.category,
            memo: mapping.memo,
            skip_rows: mapping.skip_rows,
        }
    }
}

impl From<csv::Mapping> for CsvMapping {
    fn from(mapping: csv::Mapping) -> Self {
        CsvMapping {
            date: mapping.date,
            post_date: mapping.post_date,
            description: mapping.description,
            amount: match mapping.amount {
                AmountColumns::Signed(column) => CsvAmount::Signed { column },
                AmountColumns::DebitCredit { debit, credit } => {
                    CsvAmount::DebitCredit { debit, credit }
                }
            },
            flip_sign: mapping.flip_sign,
            date_format: match mapping.date_format {
                DateFormat::YearMonthDay => CsvDateFormat::Ymd,
                DateFormat::MonthDayYear => CsvDateFormat::Mdy,
                DateFormat::DayMonthYear => CsvDateFormat::Dmy,
            },
            category: mapping.category,
            memo: mapping.memo,
            skip_rows: mapping.skip_rows,
        }
    }
}

/// A file, sent for reading.
#[derive(Deserialize, ToSchema)]
pub struct CsvReadRequest {
    account_id: i64,
    /// The file's text, as is.
    content: String,
}

/// A file's first records and the mapping to start from.
#[derive(Serialize, ToSchema)]
pub struct CsvRead {
    /// The first records, cell by cell.
    records: Vec<Vec<String>>,
    /// How many records the file has.
    record_count: usize,
    /// The columns of its widest record.
    columns: usize,
    /// The mapping saved for this account and this header; null for a header not seen before.
    #[schema(required = true)]
    saved: Option<CsvMapping>,
    /// What the header suggests, for the mapping screen to start from.
    guess: CsvMapping,
}

/// A file and the mapping to read it with.
#[derive(Deserialize, ToSchema)]
pub struct CsvRequest {
    account_id: i64,
    content: String,
    mapping: CsvMapping,
}

/// `POST /api/v1/imports/csv/read`: reads a file's records and finds its mapping.
#[utoipa::path(
    post,
    path = "/api/v1/imports/csv/read",
    operation_id = "readCsvFile",
    request_body = CsvReadRequest,
    responses(
        (status = 200, description = "The file's first records, the mapping saved for its header (if any) and a guess.", body = CsvRead),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such account.", body = ErrorEnvelope),
        (status = 422, description = "`empty_file`: the file has no records.", body = ErrorEnvelope),
    ),
)]
pub async fn read(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<CsvReadRequest>,
) -> Result<Json<CsvRead>, ApiError> {
    check_account(&state, body.account_id).await?;
    let records = csv::parse(&body.content);
    if records
        .iter()
        .all(|cells| cells.iter().all(|cell| cell.trim().is_empty()))
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "empty_file",
            "There's nothing in this file.",
        ));
    }
    // The latest saved mapping whose header is this file's, read at the row it was saved with.
    let mut saved = None;
    for mapping in db::import_mappings(&state.pool, body.account_id).await? {
        let Ok(config) = serde_json::from_str::<CsvMapping>(&mapping.config) else {
            continue;
        };
        if csv::header_signature(&records, config.skip_rows) == mapping.header_signature {
            saved = Some(config);
            break;
        }
    }
    Ok(Json(CsvRead {
        columns: csv::width(&records),
        record_count: records.len(),
        guess: csv::guess_mapping(&records).into(),
        saved,
        records: records.into_iter().take(SHOWN_RECORDS).collect(),
    }))
}

/// A row as the preview shows it.
#[derive(Serialize, ToSchema)]
pub struct CsvPreviewRow {
    /// Its record's place in the file, from 1.
    row: usize,
    /// `YYYY-MM-DD`.
    date: String,
    description: String,
    /// In minor units, signed from the account's point of view.
    amount_minor: i64,
    #[schema(required = true)]
    category: Option<String>,
}

/// A stored row a file row might be.
#[derive(Serialize, ToSchema)]
pub struct CsvStoredRow {
    date: String,
    description: String,
    amount_minor: i64,
}

impl CsvStoredRow {
    pub(crate) fn of(stored: &csv::Stored) -> CsvStoredRow {
        CsvStoredRow {
            date: stored.date.to_string(),
            description: stored.raw_description.clone(),
            amount_minor: stored.amount.minor(),
        }
    }
}

/// A row to check: it could be a row the account has, or another as good.
#[derive(Serialize, ToSchema)]
pub struct CsvCheckRow {
    #[serde(flatten)]
    row: CsvPreviewRow,
    /// The stored row it most likely is.
    #[schema(required = true)]
    could_be: Option<CsvStoredRow>,
}

/// A record the mapping can't read.
#[derive(Serialize, ToSchema)]
pub struct CsvProblem {
    row: usize,
    /// Why, in words.
    why: String,
}

/// What an import would do.
#[derive(Serialize, ToSchema)]
pub struct CsvPreview {
    /// Rows that would be added.
    new: u64,
    /// Rows the account has already.
    already: u64,
    /// Rows to decide on.
    to_check: u64,
    /// Records skipped because their date or amount doesn't read.
    unreadable: u64,
    /// The first new rows, as they'd be written.
    sample: Vec<CsvPreviewRow>,
    /// Every row to check.
    check: Vec<CsvCheckRow>,
    /// The first unreadable records.
    problems: Vec<CsvProblem>,
}

/// `POST /api/v1/imports/csv/preview`: what Import would do with the file; nothing is written.
#[utoipa::path(
    post,
    path = "/api/v1/imports/csv/preview",
    operation_id = "previewCsvImport",
    request_body = CsvRequest,
    responses(
        (status = 200, description = "How many rows are new, already there or to check.", body = CsvPreview),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such account.", body = ErrorEnvelope),
        (status = 422, description = "`invalid_mapping`: the mapping points at a column the file doesn't have; `empty_file`.", body = ErrorEnvelope),
    ),
)]
pub async fn preview(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<CsvRequest>,
) -> Result<Json<CsvPreview>, ApiError> {
    let loaded = load(&state, &body).await?;
    Ok(Json(
        preview_of(&state, body.account_id, loaded.read).await?,
    ))
}

/// What importing `read` into an account would do: shared with the OFX import.
pub(crate) async fn preview_of(
    state: &AppState,
    account_id: i64,
    read: csv::Read,
) -> Result<CsvPreview, ApiError> {
    let entries = db::plan_csv_import(&state.pool, account_id, read.rows).await?;
    let counts = csv::counts(&entries);

    let sample = entries
        .iter()
        .filter(|entry| entry.outcome == Outcome::New)
        .take(SHOWN_ROWS)
        .map(|entry| preview_row(&entry.row))
        .collect();
    let ids: Vec<i64> = entries
        .iter()
        .filter_map(|entry| match entry.outcome {
            Outcome::Check(id) => Some(id),
            _ => None,
        })
        .collect();
    let stored: HashMap<i64, csv::Stored> = db::csv_matched_rows(&state.pool, &ids)
        .await?
        .into_iter()
        .map(|row| (row.id, row))
        .collect();
    let check = entries
        .iter()
        .filter_map(|Entry { row, outcome }| match outcome {
            Outcome::Check(id) => Some(CsvCheckRow {
                row: preview_row(row),
                could_be: stored.get(id).map(CsvStoredRow::of),
            }),
            _ => None,
        })
        .collect();
    Ok(CsvPreview {
        new: counts.new,
        already: counts.already,
        to_check: counts.check,
        unreadable: read.unreadable.len() as u64,
        sample,
        check,
        problems: read
            .unreadable
            .iter()
            .take(SHOWN_PROBLEMS)
            .map(|problem| CsvProblem {
                row: problem.row,
                why: problem.why.to_owned(),
            })
            .collect(),
    })
}

fn preview_row(row: &csv::FileRow) -> CsvPreviewRow {
    CsvPreviewRow {
        row: row.row,
        date: row.date.to_string(),
        description: row.description.clone(),
        amount_minor: row.amount.minor(),
        category: row.category.clone(),
    }
}

/// A file to import, with the household's decisions.
#[derive(Deserialize, ToSchema)]
pub struct CsvCommitRequest {
    account_id: i64,
    /// The file's name, for Settings › Imports.
    file_name: String,
    content: String,
    mapping: CsvMapping,
    /// The places (`row`) of the rows to check that should be added; the rest are skipped.
    keep: Vec<usize>,
}

/// What was imported.
#[derive(Serialize, ToSchema)]
pub struct CsvImportDone {
    /// The import batch, which Settings › Imports can undo.
    batch_id: i64,
    added: u64,
    /// Rows the account had already.
    matched: u64,
    /// Unreadable records and rows to check that weren't kept.
    skipped: u64,
}

/// `POST /api/v1/imports/csv/commit`: imports the file as one import batch and saves the mapping
/// for this account and header.
#[utoipa::path(
    post,
    path = "/api/v1/imports/csv/commit",
    operation_id = "commitCsvImport",
    request_body = CsvCommitRequest,
    responses(
        (status = 201, description = "Imported, as one import batch.", body = CsvImportDone),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such account.", body = ErrorEnvelope),
        (status = 422, description = "`invalid_mapping`, `empty_file`, or `nothing_readable`: no record reads with this mapping.", body = ErrorEnvelope),
    ),
)]
pub async fn commit(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    JsonBody(body): JsonBody<CsvCommitRequest>,
) -> Result<(StatusCode, Json<CsvImportDone>), ApiError> {
    let request = CsvRequest {
        account_id: body.account_id,
        content: body.content,
        mapping: body.mapping,
    };
    let loaded = load(&state, &request).await?;
    if loaded.read.rows.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "nothing_readable",
            "No row of this file reads with this mapping.",
        ));
    }
    let config =
        serde_json::to_string(&body.mapping).map_err(|error| ApiError::internal(&error))?;
    let file_name = match body.file_name.trim() {
        "" => "file.csv",
        name => name,
    };
    let keep: BTreeSet<usize> = body.keep.into_iter().collect();
    let done = db::commit_csv_import(
        &state.pool,
        db::CsvCommit {
            account_id: body.account_id,
            file_name,
            header_signature: &loaded.signature,
            config: &config,
            unreadable: loaded.read.unreadable.len() as u64,
            rows: loaded.read.rows,
            keep: &keep,
            imported_by: current.login.user_id,
            today: state.today().await?,
        },
    )
    .await?;
    tracing::info!(batch = done.batch_id, "imported a CSV file");
    Ok((StatusCode::CREATED, Json(done.into())))
}

impl From<db::CsvCommitted> for CsvImportDone {
    fn from(done: db::CsvCommitted) -> Self {
        CsvImportDone {
            batch_id: done.batch_id,
            added: done.added,
            matched: done.matched,
            skipped: done.skipped,
        }
    }
}

/// A file read through a mapping.
struct Loaded {
    read: csv::Read,
    /// The header the mapping is saved under.
    signature: String,
}

async fn load(state: &AppState, request: &CsvRequest) -> Result<Loaded, ApiError> {
    check_account(state, request.account_id).await?;
    let records = csv::parse(&request.content);
    let mapping: csv::Mapping = request.mapping.into();
    let width = csv::width(&records);
    let columns = [
        Some(mapping.date),
        mapping.post_date,
        Some(mapping.description),
        mapping.category,
        mapping.memo,
    ]
    .into_iter()
    .flatten()
    .chain(match mapping.amount {
        AmountColumns::Signed(column) => vec![column],
        AmountColumns::DebitCredit { debit, credit } => vec![debit, credit],
    });
    if width == 0 {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "empty_file",
            "There's nothing in this file.",
        ));
    }
    if columns.into_iter().any(|column| column >= width) || mapping.skip_rows > records.len() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_mapping",
            "The mapping points at a column or a row this file doesn't have.",
        ));
    }
    let household = db::household(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
    Ok(Loaded {
        read: csv::read_rows(&records, &mapping, household.currency.decimal_places()),
        signature: csv::header_signature(&records, mapping.skip_rows),
    })
}

pub(crate) async fn check_account(state: &AppState, id: i64) -> Result<(), ApiError> {
    if db::import_account_exists(&state.pool, id).await? {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There's no such account.",
        ))
    }
}
