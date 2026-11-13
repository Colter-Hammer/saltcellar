//! Drift and balance adjustments (docs/spec/m2-bank-data.md#balances): an account's bank
//! balance against the app's, the drift flag with its suspect rows, and **Accept $X as balance
//! adjustment** with its **Undo**.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::accounts::Account;
use crate::error::{Body, ErrorEnvelope, Id};
use crate::session::Current;
use crate::{ApiError, AppState};

/// The newest snapshot, the bank's balance against the app's.
#[derive(Serialize, ToSchema)]
pub struct BankComparison {
    /// `YYYY-MM-DD`: the snapshot's date.
    as_of: String,
    /// The bank's balance, in minor units.
    bank_minor: i64,
    /// The app's balance drift is measured against, in minor units: opening balance, posted rows
    /// and balance adjustments (pending rows too where the bank's balance is known to count them).
    app_minor: i64,
    /// Bank less app, in minor units.
    drift_minor: i64,
}

impl From<db::LatestSnapshot> for BankComparison {
    fn from(latest: db::LatestSnapshot) -> Self {
        BankComparison {
            as_of: latest.as_of.to_string(),
            bank_minor: latest.bank.minor(),
            app_minor: latest.app.minor(),
            drift_minor: latest.drift.minor(),
        }
    }
}

/// What a sync run did to a row.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum RowChange {
    Added,
    Changed,
    Dropped,
}

/// A row the run that stepped the drift added, changed or dropped.
#[derive(Serialize, ToSchema)]
pub struct SuspectRow {
    /// Null once the row is gone (dropped, or deleted since).
    #[schema(required = true)]
    transaction_id: Option<i64>,
    change: RowChange,
    /// `YYYY-MM-DD`.
    date: String,
    amount_minor: i64,
    /// The bank's text for it.
    description: String,
}

/// Drift that held at the same non-zero value for two snapshots in a row.
#[derive(Serialize, ToSchema)]
pub struct DriftFlag {
    /// Bank less app, in minor units: what **Accept** records.
    drift_minor: i64,
    /// `YYYY-MM-DD`: when drift last stepped to this value.
    since: String,
    /// What the run that stepped it added, changed or dropped, newest first.
    suspect_rows: Vec<SuspectRow>,
}

impl From<db::DriftFlag> for DriftFlag {
    fn from(flag: db::DriftFlag) -> Self {
        DriftFlag {
            drift_minor: flag.drift.minor(),
            since: flag.since.to_string(),
            suspect_rows: flag
                .suspect_rows
                .into_iter()
                .map(|row| SuspectRow {
                    transaction_id: row.transaction_id,
                    change: match row.change {
                        db::RowChange::Added => RowChange::Added,
                        db::RowChange::Changed => RowChange::Changed,
                        db::RowChange::Dropped => RowChange::Dropped,
                    },
                    date: row.date.to_string(),
                    amount_minor: row.amount.minor(),
                    description: row.description,
                })
                .collect(),
        }
    }
}

/// A balance adjustment: drift the household accepted. Not a transaction.
#[derive(Serialize, ToSchema)]
pub struct BalanceAdjustment {
    id: i64,
    /// `YYYY-MM-DD`: the snapshot it settled.
    date: String,
    amount_minor: i64,
    #[schema(required = true)]
    note: Option<String>,
    /// The display name of who accepted it.
    #[schema(required = true)]
    accepted_by: Option<String>,
    /// RFC 3339.
    accepted_at: String,
    /// RFC 3339; null while it counts. An undone one is kept, for a later drift report.
    #[schema(required = true)]
    undone_at: Option<String>,
}

impl From<db::Adjustment> for BalanceAdjustment {
    fn from(adjustment: db::Adjustment) -> Self {
        BalanceAdjustment {
            id: adjustment.id,
            date: adjustment.date.to_string(),
            amount_minor: adjustment.amount.minor(),
            note: adjustment.note,
            accepted_by: adjustment.accepted_by,
            accepted_at: adjustment.accepted_at.to_string(),
            undone_at: adjustment.undone_at.map(|at| at.to_string()),
        }
    }
}

/// An account's balance against the bank's: the newest comparison, the drift flag and the
/// adjustments.
#[derive(Serialize, ToSchema, Default)]
pub struct AccountDrift {
    /// Its newest balance was sent with the wrong sign and flipped.
    sign_corrected: bool,
    /// Its newest snapshot, the bank's balance against the app's; null before one, and for a
    /// balances-only account.
    #[schema(required = true)]
    bank_comparison: Option<BankComparison>,
    /// Drift that held at the same value for two snapshots; null when there's none.
    #[schema(required = true)]
    flag: Option<DriftFlag>,
    /// Its balance adjustments, newest first, undone ones included.
    adjustments: Vec<BalanceAdjustment>,
}

/// Account `id`'s drift, as its response carries it.
pub(crate) async fn drift(state: &AppState, id: i64) -> Result<AccountDrift, ApiError> {
    let drift = db::account_drift(&state.pool, id).await?;
    Ok(AccountDrift {
        sign_corrected: drift.sign_corrected,
        bank_comparison: drift.latest.map(Into::into),
        flag: drift.flag.map(Into::into),
        adjustments: drift.adjustments.into_iter().map(Into::into).collect(),
    })
}

/// The longest note an adjustment takes *(agent)*.
const NOTE_MAX: usize = 500;

/// Accepting an account's flagged drift.
#[derive(Deserialize, ToSchema)]
pub struct AdjustmentAccept {
    /// The flagged drift being accepted, in minor units: refused if the flag has moved since.
    amount_minor: i64,
    /// Why, in the household's words; empty or left out for none.
    note: Option<String>,
}

/// `POST /api/v1/accounts/{id}/balance-adjustments`: **Accept $X as balance adjustment**.
/// Records the flagged drift as a balance adjustment dated the newest snapshot's day; the app's
/// balance includes it from then on, so the flag clears.
#[utoipa::path(
    post,
    path = "/api/v1/accounts/{id}/balance-adjustments",
    operation_id = "acceptBalanceAdjustment",
    params(("id" = i64, Path, description = "The account.")),
    request_body = AdjustmentAccept,
    responses(
        (status = 201, description = "Accepted; the account now.", body = Account),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such account.", body = ErrorEnvelope),
        (status = 409, description = "`no_drift`: the account's drift isn't flagged, or not at that amount.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed` with `note` if it is over 500 characters.", body = ErrorEnvelope),
    ),
)]
pub async fn accept(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Id(id): Id,
    Body(body): Body<AdjustmentAccept>,
) -> Result<(StatusCode, Json<Account>), ApiError> {
    let note = body
        .note
        .as_deref()
        .map(str::trim)
        .filter(|note| !note.is_empty());
    if note.is_some_and(|note| note.chars().count() > NOTE_MAX) {
        return Err(ApiError::validation(
            "Check the note.",
            BTreeMap::from([(
                "note".to_owned(),
                format!("Keep it to {NOTE_MAX} characters."),
            )]),
        ));
    }
    db::accept_balance_adjustment(
        &state.pool,
        id,
        domain::Money::from_minor(body.amount_minor),
        note,
        current.login.user_id,
        state.now(),
    )
    .await
    .map_err(refused)?;
    Ok((
        StatusCode::CREATED,
        Json(crate::accounts::detail(&state, id).await?),
    ))
}

/// `POST /api/v1/balance-adjustments/{id}/undo`: **Undo** an accepted adjustment. It is kept,
/// marked undone, and the drift it covered comes back.
#[utoipa::path(
    post,
    path = "/api/v1/balance-adjustments/{id}/undo",
    operation_id = "undoBalanceAdjustment",
    params(("id" = i64, Path, description = "The adjustment.")),
    responses(
        (status = 200, description = "Undone; its account now.", body = Account),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such adjustment.", body = ErrorEnvelope),
        (status = 409, description = "`already_undone`: it is undone already.", body = ErrorEnvelope),
    ),
)]
pub async fn undo(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Id(id): Id,
) -> Result<Json<Account>, ApiError> {
    let account = db::undo_balance_adjustment(&state.pool, id, current.login.user_id, state.now())
        .await
        .map_err(refused)?;
    Ok(Json(crate::accounts::detail(&state, account).await?))
}

fn refused(error: db::AdjustmentError) -> ApiError {
    match error {
        db::AdjustmentError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There's no such account or adjustment.",
        ),
        db::AdjustmentError::NoDrift => ApiError::new(
            StatusCode::CONFLICT,
            "no_drift",
            "The balance isn't off by that amount any more. Reload and look again.",
        ),
        db::AdjustmentError::AlreadyUndone => ApiError::new(
            StatusCode::CONFLICT,
            "already_undone",
            "That adjustment is undone already.",
        ),
        db::AdjustmentError::Db(error) => error.into(),
    }
}
