//! Import from Actual, step 1: the export is uploaded, read and summed up
//! (docs/spec/m1-ledger-import.md#step-1-file). Nothing is written to the database: the unpacked
//! export waits in the data dir for the later steps.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use http_body_util::BodyExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use utoipa::ToSchema;

use crate::auth;
use crate::error::ErrorEnvelope;
use crate::{ApiError, AppState};

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
        (status = 409, description = "`currency_not_supported`: the household currency doesn't have 2 decimal places, as Actual's amounts do; `earlier_import`: an earlier import from Actual hasn't been undone.", body = ErrorEnvelope),
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

fn is_zip(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/zip"))
}

/// The refusals that don't depend on the file: a currency Actual's cents don't fit, and an earlier
/// import still in place.
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
    if db::actual_import_in_place(&state.pool).await? {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "earlier_import",
            "Undo the earlier import from Actual first.",
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
