//! Settings › Bank connection & sync, the connection part
//! (docs/spec/m2-bank-data.md#the-connection): claiming a setup token, replacing it and
//! disconnecting.
//!
//! The access URL is a secret: it exists in memory only between the claim and its sealing, and
//! no response, log line or error here carries it.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use simplefin::{ClaimError, ClaimUrl};
use utoipa::ToSchema;

use crate::error::{Body, ErrorEnvelope};
use crate::{ApiError, AppState};

/// Where the connection stands. Anything but `ok` pauses syncing until a new setup token is
/// pasted.
#[derive(Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Ok,
    /// SimpleFIN turned the access URL down.
    TokenRevoked,
    /// The SimpleFIN subscription is unpaid.
    Unpaid,
    /// `BUDGET_SECRET_KEY` is missing, or isn't the key the connection was made with.
    KeyMismatch,
}

impl From<db::ConnectionStatus> for ConnectionStatus {
    fn from(status: db::ConnectionStatus) -> Self {
        match status {
            db::ConnectionStatus::Ok => ConnectionStatus::Ok,
            db::ConnectionStatus::TokenRevoked => ConnectionStatus::TokenRevoked,
            db::ConnectionStatus::Unpaid => ConnectionStatus::Unpaid,
            db::ConnectionStatus::KeyMismatch => ConnectionStatus::KeyMismatch,
        }
    }
}

/// The household's one bank connection. Its secret is never part of this.
#[derive(Serialize, ToSchema)]
pub struct Connection {
    /// What to call it, like `SimpleFIN Bridge`.
    label: String,
    status: ConnectionStatus,
    /// True for every status but `ok`: syncing is paused.
    paused: bool,
}

/// The bank connection screen's state.
#[derive(Serialize, ToSchema)]
pub struct BankConnection {
    /// Whether `BUDGET_SECRET_KEY` is set. Without it no setup token can be claimed.
    secret_key_set: bool,
    /// Null until a setup token is claimed, and again after Disconnect.
    connection: Option<Connection>,
}

async fn current(state: &AppState) -> Result<BankConnection, ApiError> {
    let connection = db::connection(&state.pool).await?;
    Ok(BankConnection {
        secret_key_set: state.bank.secret_key.is_some(),
        connection: connection.map(|connection| Connection {
            label: connection.label,
            paused: connection.status != db::ConnectionStatus::Ok,
            status: connection.status.into(),
        }),
    })
}

/// `GET /api/v1/bank-connection`: whether the secret key is set and what the connection is.
#[utoipa::path(
    get,
    path = "/api/v1/bank-connection",
    operation_id = "getBankConnection",
    responses(
        (status = 200, description = "The connection.", body = BankConnection),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn get(State(state): State<AppState>) -> Result<Json<BankConnection>, ApiError> {
    Ok(Json(current(&state).await?))
}

/// A pasted setup token.
#[derive(Deserialize, ToSchema)]
pub struct SetupToken {
    /// The token from SimpleFIN Bridge: base64 of a claim URL, or the claim URL itself.
    setup_token: String,
}

/// `PUT /api/v1/bank-connection`: claims a setup token, stores the access URL it answers sealed
/// with `BUDGET_SECRET_KEY`, and makes it the household's connection: a new one, or the existing
/// one with its secret replaced and its pause cleared, keeping its accounts. A token that isn't
/// one is refused before any request is made; so is every claim without the secret key.
#[utoipa::path(
    put,
    path = "/api/v1/bank-connection",
    operation_id = "claimSetupToken",
    request_body = SetupToken,
    responses(
        (status = 200, description = "Claimed; the connection now.", body = BankConnection),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 409, description = "`secret_key_missing`: `BUDGET_SECRET_KEY` isn't set.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed` with `setup_token` if it isn't a setup token; `token_refused` if SimpleFIN won't take it (already used, or not one).", body = ErrorEnvelope),
        (status = 502, description = "`bridge_unavailable`: SimpleFIN couldn't be reached or answered something unexpected.", body = ErrorEnvelope),
    ),
)]
pub async fn claim(
    State(state): State<AppState>,
    Body(body): Body<SetupToken>,
) -> Result<Json<BankConnection>, ApiError> {
    let Some(key) = state.bank.secret_key.clone() else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "secret_key_missing",
            "Bank sync needs BUDGET_SECRET_KEY, and it isn't set. Set it (openssl rand -base64 32) and restart the app, then paste the setup token.",
        ));
    };
    let policy = state.bank.claim_policy.clone();
    let url = ClaimUrl::parse(&body.setup_token, &policy).map_err(|error| {
        ApiError::validation(
            "That isn't a setup token.",
            BTreeMap::from([("setup_token".to_owned(), error.to_string())]),
        )
    })?;
    let claimed = tokio::task::spawn_blocking(move || simplefin::claim(&url, &policy))
        .await
        .map_err(|error| ApiError::internal(&error))?
        .map_err(claim_refused)?;
    let sealed = simplefin::seal(&key, &claimed).map_err(|error| ApiError::internal(&error))?;
    db::save_connection(&state.pool, &sealed).await?;
    tracing::info!("bank connection claimed");
    Ok(Json(current(&state).await?))
}

fn claim_refused(error: ClaimError) -> ApiError {
    match error {
        ClaimError::Refused(403) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "token_refused",
            "SimpleFIN didn't accept that setup token. A token works once; make a new one at SimpleFIN Bridge.",
        ),
        ClaimError::Refused(_) | ClaimError::BadAnswer => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "bridge_unavailable",
            "SimpleFIN answered with something this app can't use. Try again later.",
        ),
        ClaimError::Unreachable => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "bridge_unavailable",
            "SimpleFIN couldn't be reached. Check the server's connection and try again.",
        ),
    }
}

/// `DELETE /api/v1/bank-connection`: disconnects. The secret is deleted and syncing stops; the
/// accounts and their rows stay, and become manual accounts. Disconnecting when there is no
/// connection does nothing.
#[utoipa::path(
    delete,
    path = "/api/v1/bank-connection",
    operation_id = "disconnectBank",
    responses(
        (status = 204, description = "Disconnected."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn disconnect(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    if db::disconnect(&state.pool).await? {
        tracing::info!("bank connection removed");
    }
    Ok(StatusCode::NO_CONTENT)
}
