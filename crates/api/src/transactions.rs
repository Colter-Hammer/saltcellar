//! Transactions (docs/spec/m1-ledger-import.md#transactions): read-only in milestone 1, where the
//! Actual importer writes them. The list pages on `(date, id)`, newest first.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use domain::transactions::{Cursor, Source, Status};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ErrorEnvelope, Id};
use crate::{ApiError, AppState};

/// How many rows a page has unless asked for fewer or more *(agent)*.
const PAGE: u32 = 50;
/// The most a page has *(agent)*.
const MOST: u32 = 200;

/// Whether the bank has settled it.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TransactionStatus {
    Pending,
    Posted,
}

impl From<Status> for TransactionStatus {
    fn from(status: Status) -> Self {
        match status {
            Status::Pending => TransactionStatus::Pending,
            Status::Posted => TransactionStatus::Posted,
        }
    }
}

/// Where it came from: the bank (through the sync provider), a file, or entered by hand.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TransactionSource {
    Provider,
    Import,
    Manual,
}

impl From<Source> for TransactionSource {
    fn from(source: Source) -> Self {
        match source {
            Source::Provider => TransactionSource::Provider,
            Source::Import => TransactionSource::Import,
            Source::Manual => TransactionSource::Manual,
        }
    }
}

/// A transaction as the list shows it.
#[derive(Serialize, ToSchema)]
pub struct TransactionRow {
    id: i64,
    /// `YYYY-MM-DD`.
    date: String,
    /// In minor units: negative is money out, positive money in.
    amount_minor: i64,
    status: TransactionStatus,
    /// Null when it has none.
    #[schema(required = true)]
    payee: Option<String>,
    /// Exactly what the bank sent; never edited.
    raw_description: String,
    account_id: i64,
    account: String,
    /// Null when it's split, a transfer leg, or uncategorized.
    #[schema(required = true)]
    category_id: Option<i64>,
    #[schema(required = true)]
    category: Option<String>,
    /// Split across categories: its lines are in its detail.
    split: bool,
    /// Its tags' names, in name order.
    tags: Vec<String>,
}

impl From<db::TransactionRow> for TransactionRow {
    fn from(row: db::TransactionRow) -> Self {
        TransactionRow {
            id: row.id,
            date: row.date.to_string(),
            amount_minor: row.amount.minor(),
            status: row.status.into(),
            payee: row.payee,
            raw_description: row.raw_description,
            account_id: row.account_id,
            account: row.account,
            category_id: row.category_id,
            category: row.category,
            split: row.split,
            tags: row.tags,
        }
    }
}

/// A page of transactions, newest first.
#[derive(Serialize, ToSchema)]
pub struct TransactionPage {
    transactions: Vec<TransactionRow>,
    /// Pass it back as `after` for the next page; null on the last.
    #[schema(required = true)]
    next_cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// The `next_cursor` of the page before; leave it out for the newest rows.
    after: Option<String>,
    /// How many rows, 1 to 200; 50 if left out.
    limit: Option<u32>,
}

/// `GET /api/v1/transactions`: a page of transactions, newest first by date then id.
#[utoipa::path(
    get,
    path = "/api/v1/transactions",
    operation_id = "getTransactions",
    params(PageQuery),
    responses(
        (status = 200, description = "A page of transactions.", body = TransactionPage),
        (status = 400, description = "`invalid_page`: `after` isn't a cursor this API wrote, or `limit` is out of range.", body = ErrorEnvelope),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn list(
    State(state): State<AppState>,
    query: Result<Query<PageQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<TransactionPage>, ApiError> {
    let invalid = || {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_page",
            "That page of transactions can't be found.",
        )
    };
    let Query(query) = query.map_err(|_| invalid())?;
    let after = query
        .after
        .map(|after| after.parse::<Cursor>())
        .transpose()
        .map_err(|_| invalid())?;
    let limit = query.limit.unwrap_or(PAGE);
    if !(1..=MOST).contains(&limit) {
        return Err(invalid());
    }
    let page = db::transactions(&state.pool, after, limit).await?;
    Ok(Json(TransactionPage {
        transactions: page.transactions.into_iter().map(Into::into).collect(),
        next_cursor: page.next.map(|cursor| cursor.to_string()),
    }))
}

/// One of a split transaction's lines.
#[derive(Serialize, ToSchema)]
pub struct SplitLine {
    category_id: i64,
    category: String,
    /// In minor units.
    amount_minor: i64,
    #[schema(required = true)]
    memo: Option<String>,
}

/// The import a transaction came in with.
#[derive(Serialize, ToSchema)]
pub struct ImportBatchRef {
    id: i64,
    file_name: String,
    /// RFC 3339, UTC.
    imported_at: String,
}

/// A transaction in full.
#[derive(Serialize, ToSchema)]
pub struct Transaction {
    #[serde(flatten)]
    row: TransactionRow,
    #[schema(required = true)]
    memo: Option<String>,
    /// Empty unless it's split.
    splits: Vec<SplitLine>,
    source: TransactionSource,
    /// How it got its category, in words; null when nothing gave it one.
    #[schema(required = true)]
    categorization_reason: Option<String>,
    /// The import it came in with; null if it came in with none.
    #[schema(required = true)]
    import_batch: Option<ImportBatchRef>,
}

impl From<db::TransactionDetail> for Transaction {
    fn from(detail: db::TransactionDetail) -> Self {
        Transaction {
            row: detail.row.into(),
            memo: detail.memo,
            splits: detail
                .splits
                .into_iter()
                .map(|line| SplitLine {
                    category_id: line.category_id,
                    category: line.category,
                    amount_minor: line.amount.minor(),
                    memo: line.memo,
                })
                .collect(),
            source: detail.source.into(),
            categorization_reason: detail.categorization_reason,
            import_batch: detail.batch.map(|batch| ImportBatchRef {
                id: batch.id,
                file_name: batch.file_name,
                imported_at: batch.imported_at.to_string(),
            }),
        }
    }
}

/// `GET /api/v1/transactions/{id}`: one transaction in full.
#[utoipa::path(
    get,
    path = "/api/v1/transactions/{id}",
    operation_id = "getTransaction",
    params(("id" = i64, Path, description = "The transaction.")),
    responses(
        (status = 200, description = "The transaction.", body = Transaction),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such transaction.", body = ErrorEnvelope),
    ),
)]
pub async fn get(State(state): State<AppState>, Id(id): Id) -> Result<Json<Transaction>, ApiError> {
    let detail = db::transaction(&state.pool, id).await?.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There's no such transaction.",
        )
    })?;
    Ok(Json(detail.into()))
}
