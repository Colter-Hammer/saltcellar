//! Import from Actual (docs/spec/m1-ledger-import.md#import-from-actual). Step 1 uploads the
//! export, reads it and sums it up, writing nothing to the database: the unpacked export waits in
//! the data dir. Steps 2 and 3 read its accounts and categories, choosing in the browser. Step 4
//! commits the choices as one import batch.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use axum::Json;
use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use utoipa::ToSchema;

use domain::actual_overlap::Overlap;
use domain::import::{Plan, Target};

use crate::accounts::{AccountRole, AccountType};
use crate::auth;
use crate::error::{Body as JsonBody, ErrorEnvelope};
use crate::session::Current;
use crate::{ApiError, AppState};

/// The importer's routes, under `/api/v1`.
pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/imports/actual", post(upload))
        .route("/imports/actual/{id}", get(self::get))
        .route("/imports/actual/{id}/preview", post(preview))
        .route("/imports/actual/{id}/commit", post(commit))
}

/// The most an upload may be: 100 MB *(agent)*, in MiB so a "100 MB" file always fits.
pub const MAX_UPLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// How long an unpacked export waits for the import to commit before it's deleted *(agent)*.
const KEEP_FOR: Duration = Duration::from_hours(24);

/// Where uploaded exports wait: one folder per upload, named by its id, under the data dir.
pub struct Uploads {
    dir: PathBuf,
}

impl Uploads {
    pub fn new(data_dir: &Path) -> Uploads {
        Uploads {
            dir: data_dir.join("imports"),
        }
    }

    /// Deletes every upload older than a day. One that can't be deleted is left for next time.
    fn sweep(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age > KEEP_FOR));
            if stale && let Err(error) = std::fs::remove_dir_all(&path) {
                tracing::warn!(%error, path = %path.display(), "can't delete an old upload");
            }
        }
    }
}

/// An upload's folder, deleted again unless the upload is kept.
struct Staged {
    dir: PathBuf,
    keep: bool,
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// An uploaded Actual export and what's in it.
#[derive(Serialize, ToSchema)]
pub struct ActualUpload {
    /// Names this upload in the importer's next steps.
    id: String,
    /// The budget's name in Actual.
    #[schema(example = "Our budget")]
    budget_name: String,
    /// Live accounts, closed ones included.
    accounts: u64,
    /// Live transactions in live accounts; a split counts once.
    transactions: u64,
    /// Live categories.
    categories: u64,
    /// The earliest transaction's date, `YYYY-MM-DD`; null when there are none.
    #[schema(required = true)]
    first_date: Option<String>,
    /// The latest transaction's date, `YYYY-MM-DD`; null when there are none.
    #[schema(required = true)]
    last_date: Option<String>,
}

/// `POST /api/v1/imports/actual`: reads an Actual export.
#[utoipa::path(
    post,
    path = "/api/v1/imports/actual",
    operation_id = "uploadActualExport",
    request_body(
        content = inline(String),
        content_type = "application/zip",
        description = "The `.zip` Actual's Settings › Export data saves, as is. At most 100 MB.",
    ),
    responses(
        (status = 201, description = "The export is read; nothing is imported yet.", body = ActualUpload),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
        (status = 409, description = "`currency_not_supported`: the household currency doesn't have 2 decimal places, as Actual's amounts do.", body = ErrorEnvelope),
        (status = 413, description = "`file_too_large`: over 100 MB, or unpacks to more than the app reads.", body = ErrorEnvelope),
        (status = 415, description = "`zip_required`: the body isn't sent as `application/zip`.", body = ErrorEnvelope),
        (status = 422, description = "`not_an_actual_export`, or `export_unreadable`: an Actual export with data the app can't read.", body = ErrorEnvelope),
    ),
)]
pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<ActualUpload>), ApiError> {
    if !is_zip(&headers) {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "zip_required",
            "Send the export as the .zip file itself, with Content-Type: application/zip.",
        ));
    }
    check_household(&state).await?;
    let too_large = || {
        ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "file_too_large",
            "This file is over 100 MB, more than an Actual export should be.",
        )
    };
    let length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok()?.parse::<u64>().ok());
    if length.is_some_and(|length| length > MAX_UPLOAD_BYTES) {
        return Err(too_large());
    }

    state.uploads.sweep();
    let id = auth::random_token().map_err(|error| ApiError::internal(&error))?;
    let mut staged = Staged {
        dir: state.uploads.dir.join(&id),
        keep: false,
    };
    tokio::fs::create_dir_all(&staged.dir)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let zip = staged.dir.join("upload.zip");
    if !receive(body, &zip).await? {
        return Err(too_large());
    }

    let dir = staged.dir.clone();
    let unpacked = tokio::task::spawn_blocking(move || {
        let unpacked = actual::unpack(&zip, &dir);
        let _ = std::fs::remove_file(&zip);
        unpacked
    })
    .await
    .map_err(|error| ApiError::internal(&error))?;
    unpacked.map_err(refusal)?;
    let mut export = actual::Export::open(&staged.dir).await.map_err(refusal)?;
    let summary = export.summary().await;
    export.close().await;
    let summary = summary.map_err(refusal)?;

    staged.keep = true;
    tracing::info!(upload = %id, "read an Actual export");
    let (first_date, last_date) = summary
        .dates
        .map(|(first, last)| (first.to_string(), last.to_string()))
        .unzip();
    Ok((
        StatusCode::CREATED,
        Json(ActualUpload {
            id,
            budget_name: summary.budget_name,
            accounts: summary.accounts,
            transactions: summary.transactions,
            categories: summary.categories,
            first_date,
            last_date,
        }),
    ))
}

/// What's in an uploaded export for the importer's Accounts and Categories steps, with the
/// defaults each choice starts from. Choosing happens in the browser; nothing is written here.
#[derive(Serialize, ToSchema)]
pub struct ActualImport {
    id: String,
    #[schema(example = "Our budget")]
    budget_name: String,
    /// Every live Actual account: open before closed, on budget before off budget, then Actual's
    /// order.
    accounts: Vec<ActualAccount>,
    /// Every live Actual category that any row uses, in Actual's order, but for Starting Balances,
    /// whose rows become opening balances.
    categories: Vec<ActualCategory>,
    /// The app's own accounts, for **Same as**; closed ones included.
    app_accounts: Vec<AppAccount>,
    /// The bank accounts waiting in the new accounts prompt, which **Same as** can track too.
    waiting_accounts: Vec<WaitingAccount>,
}

/// A bank account the connection reported that waits in the new accounts prompt.
#[derive(Serialize, ToSchema)]
pub struct WaitingAccount {
    /// The provider's id for it.
    #[schema(example = "ACT-demo-checking")]
    external_account_id: String,
    /// The bank's name for it, which the account tracking it takes.
    name: String,
    #[schema(required = true)]
    institution: Option<String>,
}

/// A live Actual account, its figures, and what **Create new** starts from.
#[derive(Serialize, ToSchema)]
pub struct ActualAccount {
    /// Actual's id for it.
    id: String,
    name: String,
    closed: bool,
    off_budget: bool,
    /// Its rows: a split counts once, a starting balance counts.
    rows: u64,
    /// The balance Actual shows: every row, starting balances included, in minor units.
    balance_minor: i64,
    /// The best guess from its name.
    #[serde(rename = "type")]
    kind: AccountType,
    /// Tracking if it's off budget in Actual, else budgeted.
    role: AccountRole,
    /// The app account **Same as** starts on: the one with the same bank-sync id, else, with no
    /// account or waiting bank account of that id, one of the same name ignoring case; null when
    /// none.
    #[schema(required = true)]
    same_as: Option<i64>,
    /// The waiting bank account with the same bank-sync id, which **Same as** starts on when no
    /// app account has it; null when none.
    #[schema(required = true)]
    same_as_waiting: Option<String>,
}

/// A live Actual category and the rows using it.
#[derive(Serialize, ToSchema)]
pub struct ActualCategory {
    /// Actual's id for it.
    id: String,
    name: String,
    /// Its Actual group's name.
    group: String,
    /// Whether its Actual group is an income group.
    income: bool,
    /// How many rows in each Actual account use it, split lines included; only accounts with
    /// some are listed. Skipping an account takes its rows away.
    rows: Vec<AccountRows>,
}

#[derive(Serialize, ToSchema)]
pub struct AccountRows {
    /// Actual's id for the account.
    account_id: String,
    rows: u64,
}

/// One of the app's accounts.
#[derive(Serialize, ToSchema)]
pub struct AppAccount {
    id: i64,
    name: String,
}

/// `GET /api/v1/imports/actual/{id}`: an uploaded export's accounts and categories.
#[utoipa::path(
    get,
    path = "/api/v1/imports/actual/{id}",
    operation_id = "getActualImport",
    params(("id" = String, Path, description = "The upload, as its upload answered.")),
    responses(
        (status = 200, description = "The export's accounts and categories; nothing is imported yet.", body = ActualImport),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`upload_not_found`: no such upload, or it's over a day old and deleted.", body = ErrorEnvelope),
        (status = 422, description = "`export_unreadable`: the export has data the app can't read.", body = ErrorEnvelope),
    ),
)]
pub async fn get(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ActualImport>, ApiError> {
    let (budget_name, read) = read_upload(&state, &id).await?;

    let app_accounts = db::accounts(&state.pool).await?;
    let waiting = waiting(&state).await?;
    let accounts = read
        .accounts
        .iter()
        .map(|account| {
            let role = domain::accounts::Role::from_actual(account.off_budget);
            let bank_id = account.bank_id.as_ref();
            let by_id = app_accounts
                .iter()
                .find(|app| bank_id.is_some() && app.external_account_id.as_ref() == bank_id)
                .map(|app| app.id);
            let same_as_waiting = waiting
                .iter()
                .find(|feed| by_id.is_none() && bank_id == Some(&feed.external_id))
                .map(|feed| feed.external_id.clone());
            // Name matching covers accounts whose bank id matches nothing *(agent)*.
            let by_name = || {
                app_accounts
                    .iter()
                    .find(|app| {
                        app.name.trim().to_lowercase() == account.name.trim().to_lowercase()
                    })
                    .map(|app| app.id)
            };
            let same_as = by_id.or_else(|| same_as_waiting.is_none().then(by_name).flatten());
            ActualAccount {
                id: account.id.clone(),
                name: account.name.clone(),
                closed: account.closed,
                off_budget: account.off_budget,
                rows: account.rows,
                balance_minor: account.balance.minor(),
                kind: domain::accounts::AccountType::guess(&account.name, role).into(),
                role: role.into(),
                same_as,
                same_as_waiting,
            }
        })
        .collect();
    let categories = read
        .categories
        .into_iter()
        .filter(|category| !category.rows.is_empty() && !category.is_starting_balances())
        .map(|category| ActualCategory {
            id: category.id,
            name: category.name,
            group: category.group,
            income: category.income,
            rows: category
                .rows
                .into_iter()
                .map(|(account_id, rows)| AccountRows { account_id, rows })
                .collect(),
        })
        .collect();
    Ok(Json(ActualImport {
        id,
        budget_name,
        accounts,
        categories,
        app_accounts: app_accounts
            .into_iter()
            .map(|account| AppAccount {
                id: account.id,
                name: account.name,
            })
            .collect(),
        waiting_accounts: waiting
            .into_iter()
            .map(|feed| WaitingAccount {
                external_account_id: feed.external_id,
                name: feed.name,
                institution: feed.institution,
            })
            .collect(),
    }))
}

/// The bank accounts waiting in the new accounts prompt that could be tracked: those in the
/// household's currency.
async fn waiting(state: &AppState) -> Result<Vec<db::ConnectionAccount>, ApiError> {
    let household = db::household(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
    let currency = household.currency.code();
    Ok(db::connection_accounts(&state.pool)
        .await?
        .into_iter()
        .filter(|feed| feed.decision == db::FeedDecision::Pending && feed.currency == currency)
        .collect())
}

/// The upload `id`'s folder, if it's still there.
fn upload_dir(state: &AppState, id: &str) -> Result<PathBuf, ApiError> {
    let gone = || {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "upload_not_found",
            "This upload is gone: uploads wait a day. Upload the export again.",
        )
    };
    // Only an id this server made names a folder: 64 hex digits, nothing that could leave it.
    if id.len() != 64 || !id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(gone());
    }
    let dir = state.uploads.dir.join(id);
    if !dir.join(actual::DB_FILE).is_file() {
        return Err(gone());
    }
    Ok(dir)
}

/// The upload `id`'s budget name and live rows.
async fn read_upload(
    state: &AppState,
    id: &str,
) -> Result<(String, domain::actual::Export), ApiError> {
    let dir = upload_dir(state, id)?;
    let mut export = actual::Export::open(&dir).await.map_err(refusal)?;
    let read = export.read().await;
    let budget_name = export.budget_name().to_owned();
    export.close().await;
    Ok((budget_name, read.map_err(refusal)?))
}

/// The importer's choices, to commit.
#[derive(Deserialize, ToSchema)]
pub struct ActualCommitRequest {
    /// The uploaded file's name, as Settings › Imports will list it.
    #[schema(example = "Example-Household-2026-10-30.zip")]
    file_name: String,
    /// What happens to each live Actual account: every one needs a choice.
    accounts: Vec<ActualAccountChoice>,
    /// Where each Actual category's rows go; one left out goes to Decide later.
    categories: Vec<ActualCategoryChoice>,
    /// The possible duplicates the preview listed that the household says are new, by Actual's
    /// transaction id: they're added. Any other is taken as the row it may be.
    #[serde(default)]
    new_rows: Vec<String>,
}

/// Create a new account, use one the app has (Same as), track a bank account waiting in the new
/// accounts prompt (Same as one of those), or leave the account out.
#[derive(Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AccountAction {
    Create,
    Same,
    Track,
    Skip,
}

/// What happens to one Actual account. Create new reads `name`, `type`, `role` and
/// `balances_only`; Same as reads `same_as`; Track reads `track`, `role` and `balances_only`.
#[derive(Deserialize, ToSchema)]
pub struct ActualAccountChoice {
    /// Actual's id for the account.
    actual_id: String,
    action: AccountAction,
    name: String,
    #[serde(rename = "type")]
    kind: AccountType,
    role: AccountRole,
    /// Brings the balance and none of the rows.
    balances_only: bool,
    /// The app account it's the same as.
    #[schema(required = true)]
    same_as: Option<i64>,
    /// The waiting bank account it's the same as, by its provider id, to track.
    #[serde(default)]
    track: Option<String>,
}

/// Where an Actual category's rows go.
#[derive(Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum GoesTo {
    /// The app category in `category_id`.
    Category,
    /// Transfer (not a category).
    Transfer,
    /// Decide later.
    Later,
}

#[derive(Deserialize, ToSchema)]
pub struct ActualCategoryChoice {
    /// Actual's id for the category.
    actual_id: String,
    goes_to: GoesTo,
    /// The app category, when it goes to one.
    #[schema(required = true)]
    category_id: Option<i64>,
}

/// The committed import, as the Done step sums it up.
#[derive(Serialize, ToSchema)]
pub struct ActualImportDone {
    /// The import batch, which Settings › Imports can undo.
    batch_id: i64,
    /// How many accounts the import created.
    accounts_created: u64,
    /// Each account the import created or added rows to, with its rows, in the export's order.
    accounts: Vec<ImportedAccount>,
    /// Rows that were there already, synced or from an earlier import: they took Actual's
    /// category, notes and payee instead of coming in again.
    matched: u64,
    /// Rows that took the category theirs goes to, split rows among them.
    mapped: u64,
    /// Rows on Decide later, and splits with a line that has no category to go to: they wait in
    /// From Actual, undecided.
    undecided: u64,
    /// Rows marked transfer: those whose category goes to Transfer, and transfer legs.
    transfer: u64,
    /// Rows with no category in Actual.
    uncategorized: u64,
    /// Transfers linked between two imported accounts; their legs are among the rows marked
    /// transfer.
    linked_transfers: u64,
}

#[derive(Serialize, ToSchema)]
pub struct ImportedAccount {
    id: i64,
    name: String,
    /// Whether the import created it (or tracked it), rather than adding to one there already.
    created: bool,
    /// Rows added.
    rows: u64,
    /// Rows that were there already.
    matched: u64,
}

/// What committing would do, before it does: how many rows come in and how many are there
/// already, and the possible duplicates for the household to decide on.
#[derive(Serialize, ToSchema)]
pub struct ActualPreview {
    /// Rows that would be added.
    new: u64,
    /// Rows there already, synced or from an earlier import, which take Actual's category, notes
    /// and payee instead.
    already: u64,
    /// Rows that could be a row an account has, or another as good: added only when the
    /// household says they're new (`new_rows`), otherwise taken as that row.
    check: Vec<ActualCheckRow>,
}

/// A possible duplicate: an Actual row and the stored row it most likely is.
#[derive(Serialize, ToSchema)]
pub struct ActualCheckRow {
    /// Actual's transaction id, as `new_rows` names it.
    actual_id: String,
    /// The app account it goes into.
    account: String,
    /// `YYYY-MM-DD`.
    date: String,
    description: String,
    amount_minor: i64,
    /// The stored row it most likely is.
    #[schema(required = true)]
    could_be: Option<crate::csv_import::CsvStoredRow>,
}

/// `POST /api/v1/imports/actual/{id}/preview`: what importing the upload with these choices
/// would do; nothing is written.
#[utoipa::path(
    post,
    path = "/api/v1/imports/actual/{id}/preview",
    operation_id = "previewActualImport",
    params(("id" = String, Path, description = "The upload, as its upload answered.")),
    request_body = ActualCommitRequest,
    responses(
        (status = 200, description = "What the import would do.", body = ActualPreview),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`upload_not_found`: no such upload, or it's over a day old and deleted.", body = ErrorEnvelope),
        (status = 422, description = "`invalid_choices`, or `export_unreadable`, as the commit answers.", body = ErrorEnvelope),
    ),
)]
pub async fn preview(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    JsonBody(body): JsonBody<ActualCommitRequest>,
) -> Result<Json<ActualPreview>, ApiError> {
    let (_, export) = read_upload(&state, &id).await?;
    let plan = plan_of(&state, &export, &body).await?;
    let overlaps = db::actual_overlaps(&state.pool, &plan).await?;
    let names: HashMap<i64, String> = db::accounts(&state.pool)
        .await?
        .into_iter()
        .map(|account| (account.id, account.name))
        .collect();
    let mut preview = ActualPreview {
        new: 0,
        already: 0,
        check: Vec::new(),
    };
    let mut checks = Vec::new();
    for (planned, overlaps) in plan.accounts.iter().zip(&overlaps) {
        for (row, overlap) in planned.rows.iter().zip(overlaps) {
            match overlap {
                Overlap::New => preview.new += 1,
                Overlap::Check(stored) => checks.push((planned, row, *stored)),
                _ => preview.already += 1,
            }
        }
    }
    let ids: Vec<i64> = checks.iter().map(|(_, _, stored)| *stored).collect();
    let stored: HashMap<i64, domain::csv_import::Stored> = db::csv_matched_rows(&state.pool, &ids)
        .await?
        .into_iter()
        .map(|row| (row.id, row))
        .collect();
    preview.check = checks
        .into_iter()
        .map(|(planned, row, id)| ActualCheckRow {
            actual_id: row.import_ref.clone(),
            account: match &planned.target {
                Target::Existing(account) => names.get(account).cloned().unwrap_or_default(),
                _ => planned.actual_name.clone(),
            },
            date: row.date.to_string(),
            description: row.raw_description.clone(),
            amount_minor: row.amount.minor(),
            could_be: stored.get(&id).map(crate::csv_import::CsvStoredRow::of),
        })
        .collect();
    Ok(Json(preview))
}

/// The choices applied to the export, checked against what the app has.
async fn plan_of(
    state: &AppState,
    export: &domain::actual::Export,
    body: &ActualCommitRequest,
) -> Result<Plan, ApiError> {
    let choices = choices(body).map_err(invalid)?;
    let app_accounts: Vec<(i64, String)> = db::accounts(&state.pool)
        .await?
        .into_iter()
        .map(|account| (account.id, account.name))
        .collect();
    let app_categories = db::category_groups(&state.pool)
        .await?
        .into_iter()
        .flat_map(|group| group.categories)
        .map(|category| category.id)
        .collect();
    let waiting: Vec<String> = waiting(state)
        .await?
        .into_iter()
        .map(|feed| feed.external_id)
        .collect();
    Plan::new(
        export,
        &choices,
        domain::import::Existing {
            accounts: &app_accounts,
            categories: &app_categories,
            waiting: &waiting,
        },
    )
    .map_err(|error| invalid(error.to_string()))
}

/// `POST /api/v1/imports/actual/{id}/commit`: imports the upload with the importer's choices.
#[utoipa::path(
    post,
    path = "/api/v1/imports/actual/{id}/commit",
    operation_id = "commitActualImport",
    params(("id" = String, Path, description = "The upload, as its upload answered.")),
    request_body = ActualCommitRequest,
    responses(
        (status = 201, description = "Imported, as one import batch.", body = ActualImportDone),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`upload_not_found`: no such upload, or it's over a day old and deleted.", body = ErrorEnvelope),
        (status = 422, description = "`invalid_choices`: an account without a choice or with one that can't go ahead (a bank account to track that isn't waiting any more among them), or a category going to one that isn't there; `export_unreadable`: the export has data the app can't read, or a balance that can't be worked out.", body = ErrorEnvelope),
    ),
)]
pub async fn commit(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    axum::extract::Path(id): axum::extract::Path<String>,
    JsonBody(body): JsonBody<ActualCommitRequest>,
) -> Result<(StatusCode, Json<ActualImportDone>), ApiError> {
    let (_, export) = read_upload(&state, &id).await?;
    let plan = plan_of(&state, &export, &body).await?;
    let new_rows: HashSet<String> = body.new_rows.iter().cloned().collect();
    let committed = db::commit_actual_import(
        &state.pool,
        db::ActualCommit {
            plan: &plan,
            file_name: body.file_name.trim(),
            imported_by: current.login.user_id,
            today: state.today().await?,
            new_rows: &new_rows,
        },
    )
    .await
    .map_err(|error| match error {
        db::ImportError::NotWaiting(external_id) => invalid(format!(
            "The bank account {external_id} isn't waiting in the new accounts prompt any more."
        )),
        db::ImportError::Balance(account) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "export_unreadable",
            format!("This Actual export has data the app can't read: the balance of {account}."),
        ),
        db::ImportError::Db(error) => error.into(),
    })?;

    if let Err(error) = tokio::fs::remove_dir_all(state.uploads.dir.join(&id)).await {
        tracing::warn!(%error, upload = %id, "can't delete a committed upload");
    }
    let counts = plan.counts();
    tracing::info!(batch = committed.batch_id, upload = %id, "imported from Actual");
    Ok((
        StatusCode::CREATED,
        Json(ActualImportDone {
            batch_id: committed.batch_id,
            accounts_created: committed.accounts.iter().filter(|a| a.created).count() as u64,
            matched: committed.accounts.iter().map(|a| a.matched).sum(),
            accounts: committed
                .accounts
                .into_iter()
                .map(|account| ImportedAccount {
                    id: account.id,
                    name: account.name,
                    created: account.created,
                    rows: account.rows,
                    matched: account.matched,
                })
                .collect(),
            mapped: counts.mapped,
            undecided: counts.undecided,
            transfer: counts.transfer,
            uncategorized: counts.uncategorized,
            linked_transfers: committed.linked_transfers,
        }),
    ))
}

/// The request's choices, by Actual id.
fn choices(body: &ActualCommitRequest) -> Result<domain::import::Choices, String> {
    use domain::import::{AccountChoice, GoesTo as Goes};
    let mut choices = domain::import::Choices::default();
    for choice in &body.accounts {
        let account =
            match choice.action {
                AccountAction::Create => AccountChoice::Create {
                    name: choice.name.clone(),
                    kind: choice.kind.into(),
                    role: choice.role.into(),
                    balances_only: choice.balances_only,
                },
                AccountAction::Same => AccountChoice::SameAs(choice.same_as.ok_or_else(|| {
                    format!("Choose the account {} is the same as.", choice.name)
                })?),
                AccountAction::Track => AccountChoice::Track {
                    external_id: choice.track.clone().ok_or_else(|| {
                        format!("Choose the bank account {} is the same as.", choice.name)
                    })?,
                    role: choice.role.into(),
                    balances_only: choice.balances_only,
                },
                AccountAction::Skip => AccountChoice::Skip,
            };
        choices.accounts.insert(choice.actual_id.clone(), account);
    }
    for choice in &body.categories {
        let goes_to = match (choice.goes_to, choice.category_id) {
            (GoesTo::Category, Some(id)) => Goes::Category(id),
            (GoesTo::Category, None) => {
                return Err("Choose the category each Actual category goes to.".to_owned());
            }
            (GoesTo::Transfer, _) => Goes::Transfer,
            (GoesTo::Later, _) => Goes::Later,
        };
        choices.categories.insert(choice.actual_id.clone(), goes_to);
    }
    Ok(choices)
}

fn invalid(message: String) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_choices", message)
}

fn is_zip(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/zip"))
}

/// The refusal that doesn't depend on the file: a currency Actual's cents don't fit. An earlier
/// import still in place is no reason any more: a re-import matches its rows by id.
async fn check_household(state: &AppState) -> Result<(), ApiError> {
    // Behind the setup gate, so the household exists unless the database lost it.
    let household = db::household(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
    let places = household.currency.decimal_places();
    if places != 2 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "currency_not_supported",
            format!(
                "Actual keeps amounts in cents, so its exports only fit a household currency with 2 decimal places, and {} has {places}.",
                household.currency
            ),
        ));
    }
    Ok(())
}

/// Writes the request body to `path`; `false` once it passes [`MAX_UPLOAD_BYTES`].
async fn receive(mut body: Body, path: &Path) -> Result<bool, ApiError> {
    let internal = |error: std::io::Error| ApiError::internal(&error);
    let mut file = tokio::fs::File::create(path).await.map_err(internal)?;
    let mut total: u64 = 0;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "upload_failed",
                "The upload broke off. Try again.",
            )
        })?;
        if let Ok(data) = frame.into_data() {
            total += data.len() as u64;
            if total > MAX_UPLOAD_BYTES {
                return Ok(false);
            }
            file.write_all(&data).await.map_err(internal)?;
        }
    }
    file.flush().await.map_err(internal)?;
    Ok(true)
}

/// A file the importer can't read, as the answer that says why.
fn refusal(error: actual::ReadError) -> ApiError {
    let message = error.to_string();
    match error {
        actual::ReadError::NotAnExport => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "not_an_actual_export",
            message,
        ),
        actual::ReadError::TooLarge => {
            ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large", message)
        }
        actual::ReadError::Unreadable(_) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "export_unreadable",
            message,
        ),
        actual::ReadError::Io(error) => ApiError::internal(&error),
    }
}
