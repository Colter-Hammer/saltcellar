//! The new accounts prompt (docs/spec/m2-bank-data.md#accounts): the accounts the bank
//! connection reports that wait to be tracked or ignored, the ignored ones, and the ones refused
//! for their currency.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use domain::accounts::{AccountType, Role};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::accounts::AccountRole;
use crate::error::{Body, ErrorEnvelope};
use crate::{ApiError, AppState};

/// An account the connection reports that isn't tracked.
#[derive(Serialize, ToSchema)]
pub struct FeedAccount {
    /// The provider's account id.
    external_account_id: String,
    /// As the bank names it.
    name: String,
    /// Its bank, as the feed names it; null when it doesn't.
    #[schema(required = true)]
    institution: Option<String>,
    /// What tracking it suggests for balances only: true when its name says investment or loan
    /// *(agent)*.
    balances_only_suggested: bool,
    /// The tracked account it is likely the same as: one with the same name and bank whose own
    /// provider id has stopped arriving. Null when there's none.
    #[schema(required = true)]
    same_as_account_id: Option<i64>,
}

/// An account in another currency than the household's: recorded, never tracked.
#[derive(Serialize, ToSchema)]
pub struct RefusedAccount {
    external_account_id: String,
    name: String,
    #[schema(required = true)]
    institution: Option<String>,
    /// ISO 4217, as the feed reports it.
    currency: String,
    /// Why it can't be tracked, to show as is.
    message: String,
}

/// The new accounts prompt's state.
#[derive(Serialize, ToSchema)]
pub struct ConnectionAccounts {
    /// Waiting for a decision, in the order first seen.
    pending: Vec<FeedAccount>,
    /// Ignored; each can be revived.
    ignored: Vec<FeedAccount>,
    /// In another currency than the household's.
    refused: Vec<RefusedAccount>,
}

/// `GET /api/v1/connection-accounts`: the connection's accounts that aren't tracked.
#[utoipa::path(
    get,
    path = "/api/v1/connection-accounts",
    operation_id = "getConnectionAccounts",
    responses(
        (status = 200, description = "Waiting, ignored and refused; all empty without a connection.", body = ConnectionAccounts),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<ConnectionAccounts>, ApiError> {
    let household = household_currency(&state).await?;
    let mut prompt = ConnectionAccounts {
        pending: Vec::new(),
        ignored: Vec::new(),
        refused: Vec::new(),
    };
    for account in db::connection_accounts(&state.pool).await? {
        if account.decision == db::FeedDecision::Tracked {
            continue;
        }
        if account.currency != household {
            prompt.refused.push(RefusedAccount {
                message: foreign(&account.name, &account.currency, household),
                external_account_id: account.external_id,
                name: account.name,
                institution: account.institution,
                currency: account.currency,
            });
            continue;
        }
        let kind = AccountType::guess(&account.name, Role::Tracking);
        let shown = FeedAccount {
            balances_only_suggested: matches!(kind, AccountType::Investment | AccountType::Loan),
            external_account_id: account.external_id,
            name: account.name,
            institution: account.institution,
            same_as_account_id: account.same_as,
        };
        match account.decision {
            db::FeedDecision::Ignored => prompt.ignored.push(shown),
            _ => prompt.pending.push(shown),
        }
    }
    Ok(Json(prompt))
}

/// Why an account in `currency` can't be tracked by a household in `household`.
fn foreign(name: &str, currency: &str, household: &str) -> String {
    format!(
        "{name} reports its money in {currency}, and this household keeps {household}, so it can't be tracked."
    )
}

async fn household_currency(state: &AppState) -> Result<&'static str, ApiError> {
    let household = db::household(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
    Ok(household.currency.code())
}

/// How to track a feed account.
#[derive(Deserialize, ToSchema)]
pub struct TrackFeedAccount {
    role: AccountRole,
    /// Only its balance is kept; transactions the feed sends for it are ignored.
    balances_only: bool,
    /// The account it is the same as, which moves onto it with its history and opening balance.
    /// Left out or null: a new account is made, named as the bank names it, of the type its name
    /// suggests.
    #[serde(default)]
    same_as_account_id: Option<i64>,
}

/// What tracking made or linked.
#[derive(Serialize, ToSchema)]
pub struct Tracked {
    /// The account that now gets the feed.
    account_id: i64,
}

/// `POST /api/v1/connection-accounts/{external_account_id}/track`: tracks a waiting or ignored
/// feed account, then back-fills it: its last 90 days of rows arrive before this answers. If the
/// back-fill fails it is tried again on the next sync, and the account is tracked all the same.
#[utoipa::path(
    post,
    path = "/api/v1/connection-accounts/{external_account_id}/track",
    operation_id = "trackConnectionAccount",
    params(("external_account_id" = String, Path, description = "The provider's account id.")),
    request_body = TrackFeedAccount,
    responses(
        (status = 200, description = "Tracked.", body = Tracked),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: the connection hasn't reported it, or the account to link doesn't exist.", body = ErrorEnvelope),
        (status = 409, description = "`already_tracked`: it is tracked already.", body = ErrorEnvelope),
        (status = 422, description = "`foreign_currency`: its currency isn't the household's.", body = ErrorEnvelope),
    ),
)]
pub async fn track(
    State(state): State<AppState>,
    Path(external_id): Path<String>,
    Body(body): Body<TrackFeedAccount>,
) -> Result<Json<Tracked>, ApiError> {
    let household = household_currency(&state).await?;
    let track = db::Track {
        role: body.role.into(),
        balances_only: body.balances_only,
        same_as: body.same_as_account_id,
    };
    let account_id = db::track_connection_account(&state.pool, &external_id, &track, household)
        .await
        .map_err(|error| refused(error, household))?;
    if let Err(error) = crate::sync::backfill(&state).await {
        tracing::warn!(code = error.code(), "the back-fill after tracking failed");
    }
    Ok(Json(Tracked { account_id }))
}

/// `POST /api/v1/connection-accounts/{external_account_id}/ignore`: ignores a waiting feed
/// account. It is listed under Ignored, and can be tracked later.
#[utoipa::path(
    post,
    path = "/api/v1/connection-accounts/{external_account_id}/ignore",
    operation_id = "ignoreConnectionAccount",
    params(("external_account_id" = String, Path, description = "The provider's account id.")),
    responses(
        (status = 204, description = "Ignored."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: the connection hasn't reported it.", body = ErrorEnvelope),
        (status = 409, description = "`already_tracked`: it is tracked.", body = ErrorEnvelope),
    ),
)]
pub async fn ignore(
    State(state): State<AppState>,
    Path(external_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let household = household_currency(&state).await?;
    db::ignore_connection_account(&state.pool, &external_id)
        .await
        .map_err(|error| refused(error, household))?;
    Ok(StatusCode::NO_CONTENT)
}

fn refused(error: db::TrackError, household: &str) -> ApiError {
    match error {
        db::TrackError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "The bank connection hasn't reported that account.",
        ),
        db::TrackError::NoSuchAccount => ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There's no such account to link it to.",
        ),
        db::TrackError::AlreadyTracked => ApiError::new(
            StatusCode::CONFLICT,
            "already_tracked",
            "That account is tracked already. Reload to see it.",
        ),
        db::TrackError::ForeignCurrency { currency } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "foreign_currency",
            format!(
                "That account reports its money in {currency}, and this household keeps {household}, so it can't be tracked."
            ),
        ),
        db::TrackError::Db(error) => error.into(),
    }
}
