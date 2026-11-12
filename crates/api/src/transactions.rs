//! Transactions (docs/spec/m1-ledger-import.md#transactions): read here; manual entry
//! (`crate::manual`) writes the rows typed in by hand. The list pages on `(date, id)`, newest first, searched and filtered
//! by its query; the tags are listed for its tag filter.

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use domain::transactions::{CategoryFilter, Cursor, Search, Source, Status, SyncFlag};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{Body, ErrorEnvelope, Id};
use crate::session::Current;
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

/// What sync wants the household to look at (docs/spec/m2-bank-data.md#sync-health-in-the-ui),
/// each with its action: `possible_duplicate` (**Merge** / **Keep both**),
/// `possible_manual_match` (**Yes** / **No**), `not_seen_at_bank` (**Delete** / **Keep**) and
/// `amount_changed_split` (fix the split).
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransactionSyncFlag {
    PossibleDuplicate,
    PossibleManualMatch,
    NotSeenAtBank,
    AmountChangedSplit,
}

impl From<SyncFlag> for TransactionSyncFlag {
    fn from(flag: SyncFlag) -> Self {
        match flag {
            SyncFlag::PossibleDuplicate => TransactionSyncFlag::PossibleDuplicate,
            SyncFlag::PossibleManualMatch => TransactionSyncFlag::PossibleManualMatch,
            SyncFlag::NotSeenAtBank => TransactionSyncFlag::NotSeenAtBank,
            SyncFlag::AmountChangedSplit => TransactionSyncFlag::AmountChangedSplit,
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
    /// How many different categories its split lines are in; 0 unless it's split.
    split_categories: u32,
    /// The other account of the transfer it's a leg of; null for any other transaction.
    #[schema(required = true)]
    transfer_account: Option<String>,
    /// That other account's id; null for any other transaction.
    #[schema(required = true)]
    transfer_account_id: Option<i64>,
    /// Its tags' names, in name order.
    tags: Vec<String>,
    /// Entered by hand and not yet adopted by a bank row: it shows the **manual** badge, and
    /// `PUT` and `DELETE /api/v1/transactions/{id}` can edit or delete it.
    manual: bool,
    /// What sync wants the household to look at; null when nothing. Listed under **Needs a look**.
    #[schema(required = true)]
    sync_flag: Option<TransactionSyncFlag>,
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
            split_categories: u32::try_from(row.split_categories).unwrap_or_default(),
            transfer_account: row.transfer_account,
            transfer_account_id: row.transfer_account_id,
            tags: row.tags,
            manual: row.manual,
            sync_flag: row.sync_flag.map(Into::into),
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

/// A page, and the search and filters that pick its rows. Every filter given narrows the list; the
/// Transactions screen's address carries the same parameters *(agent)*.
#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// The `next_cursor` of the page before; leave it out for the newest rows. Pass the same
    /// search and filters with it.
    after: Option<String>,
    /// How many rows, 1 to 200; 50 if left out.
    limit: Option<u32>,
    /// Text in the payee, raw description or memo (ignoring case), or an amount in the household
    /// currency, either way round: `12.50` matches −12.50 and 12.50.
    q: Option<String>,
    /// An account's id.
    #[param(value_type = Option<i64>)]
    account: Option<String>,
    /// A category's id (split rows count in each of their lines' categories), or `uncategorized`.
    category: Option<String>,
    /// On or after this day, `YYYY-MM-DD`.
    from: Option<String>,
    /// On or before this day, `YYYY-MM-DD`.
    to: Option<String>,
    /// A tag's id.
    #[param(value_type = Option<i64>)]
    tag: Option<String>,
    /// `true` for only the rows still waiting in **From Actual, undecided**.
    #[param(value_type = Option<bool>)]
    undecided: Option<String>,
    /// With `undecided=true`: only the rows that came in under this Actual category; an empty
    /// value is the rows whose export kept none *(agent)*.
    actual_category: Option<String>,
    /// `true` for only the rows with a sync flag: **Needs a look**.
    #[param(value_type = Option<bool>)]
    needs_a_look: Option<String>,
}

/// A filter parameter that can't be read.
fn invalid_filter() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_filter",
        "Those filters can't be read.",
    )
}

/// `text`, if given, as an id.
fn read_id(text: Option<&str>) -> Result<Option<i64>, ApiError> {
    text.map(|text| {
        text.parse::<i64>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or_else(invalid_filter)
    })
    .transpose()
}

/// `text`, if given, as a day.
fn read_day(text: Option<&str>) -> Result<Option<domain::Date>, ApiError> {
    text.map(|text| text.parse().map_err(|_| invalid_filter()))
        .transpose()
}

impl PageQuery {
    /// The filter its search and filter parameters ask for.
    async fn filter(&self, state: &AppState) -> Result<db::Filter, ApiError> {
        let category = match self.category.as_deref() {
            None => None,
            Some("uncategorized") => Some(CategoryFilter::Uncategorized),
            Some(text) => read_id(Some(text))?.map(CategoryFilter::Category),
        };
        if self.actual_category.is_some() && self.undecided.as_deref() != Some("true") {
            return Err(invalid_filter());
        }
        let undecided = match self.undecided.as_deref() {
            None | Some("false") => false,
            Some("true") => true,
            Some(_) => return Err(invalid_filter()),
        };
        let needs_a_look = match self.needs_a_look.as_deref() {
            None | Some("false") => false,
            Some("true") => true,
            Some(_) => return Err(invalid_filter()),
        };
        let mut filter = db::Filter {
            search: None,
            account_id: read_id(self.account.as_deref())?,
            category,
            from: read_day(self.from.as_deref())?,
            to: read_day(self.to.as_deref())?,
            tag_id: read_id(self.tag.as_deref())?,
            undecided,
            actual_category: self.actual_category.clone(),
            needs_a_look,
        };
        if let Some(text) = self.q.as_deref().filter(|text| !text.trim().is_empty()) {
            // Behind the setup gate, so the household exists unless the database lost it.
            let household = db::household(&state.pool)
                .await?
                .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
            filter.search = Search::new(text, household.currency);
        }
        Ok(filter)
    }
}

/// `GET /api/v1/transactions`: a page of the transactions the search and filters pick, newest
/// first by date then id.
#[utoipa::path(
    get,
    path = "/api/v1/transactions",
    operation_id = "getTransactions",
    params(PageQuery),
    responses(
        (status = 200, description = "A page of transactions.", body = TransactionPage),
        (status = 400, description = "`invalid_page`: `after` isn't a cursor this API wrote, or `limit` is out of range. `invalid_filter`: `account`, `category`, `from`, `to`, `tag`, `undecided`, `actual_category` or `needs_a_look` can't be read.", body = ErrorEnvelope),
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
        .as_deref()
        .map(str::parse::<Cursor>)
        .transpose()
        .map_err(|_| invalid())?;
    let limit = query.limit.unwrap_or(PAGE);
    if !(1..=MOST).contains(&limit) {
        return Err(invalid());
    }
    let filter = query.filter(&state).await?;
    let page = db::transactions(&state.pool, &filter, after, limit).await?;
    Ok(Json(TransactionPage {
        transactions: page.transactions.into_iter().map(Into::into).collect(),
        next_cursor: page.next.map(|cursor| cursor.to_string()),
    }))
}

/// A tag, as the tag filter offers it.
#[derive(Serialize, ToSchema)]
pub struct Tag {
    id: i64,
    /// Without the `#`.
    name: String,
}

/// Every tag the household has.
#[derive(Serialize, ToSchema)]
pub struct TagList {
    /// By name, ignoring case.
    tags: Vec<Tag>,
}

/// `GET /api/v1/tags`: every tag, for the Transactions tag filter.
#[utoipa::path(
    get,
    path = "/api/v1/tags",
    operation_id = "getTags",
    responses(
        (status = 200, description = "Every tag, by name.", body = TagList),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn tags(State(state): State<AppState>) -> Result<Json<TagList>, ApiError> {
    let tags = db::tags(&state.pool).await?;
    Ok(Json(TagList {
        tags: tags
            .into_iter()
            .map(|tag| Tag {
                id: tag.id,
                name: tag.name,
            })
            .collect(),
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
    /// One of the rows waiting in **From Actual, undecided**: it can be given a category with
    /// `PUT /api/v1/transactions/{id}/category`.
    undecided: bool,
    /// Excluded from budgets: `PUT /api/v1/transactions/{id}/excluded` sets it.
    excluded: bool,
    /// The row a `possible_duplicate` or `possible_manual_match` flag points at; null otherwise.
    #[schema(required = true)]
    flagged_against: Option<TransactionRow>,
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
            undecided: detail.undecided,
            excluded: detail.excluded,
            flagged_against: detail.flagged_against.map(Into::into),
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

/// Whether a transaction counts in budgets.
#[derive(Deserialize, ToSchema)]
pub struct Excluded {
    /// True: left out of budgets. False: counted again.
    excluded: bool,
}

/// `PUT /api/v1/transactions/{id}/excluded`: excludes a transaction from budgets, or counts it
/// again. Offered in place of deleting a row from the bank
/// (docs/spec/m2-bank-data.md#deleting-a-synced-row).
#[utoipa::path(
    put,
    path = "/api/v1/transactions/{id}/excluded",
    operation_id = "setTransactionExcluded",
    params(("id" = i64, Path, description = "The transaction.")),
    request_body = Excluded,
    responses(
        (status = 200, description = "The transaction, changed.", body = Transaction),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such transaction.", body = ErrorEnvelope),
    ),
)]
pub async fn set_excluded(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Id(id): Id,
    Body(body): Body<Excluded>,
) -> Result<Json<Transaction>, ApiError> {
    let found = db::set_excluded(
        &state.pool,
        id,
        body.excluded,
        current.login.user_id,
        state.now(),
    )
    .await?;
    if !found {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There's no such transaction.",
        ));
    }
    get(State(state), Id(id)).await
}
