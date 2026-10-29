//! Logging in and out, and the session every other route needs
//! (docs/spec/m1-ledger-import.md#login-and-profile, docs/spec/foundations.md#auth).

use axum::Json;
use axum::extract::{Extension, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use domain::login::Username;
use domain::throttle;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::{self, NewSession};
use crate::error::{Body, ErrorEnvelope};
use crate::{ApiError, AppState};

/// The session a request came with, for the routes behind [`require_login`].
#[derive(Clone)]
pub struct Current {
    token_hash: String,
    login: db::SessionLogin,
}

/// Every route behind this needs a live session: without one it answers `login_required`. Each
/// use slides the session's expiry, and the cookie's with it.
pub async fn require_login(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let Some(token) = auth::token(request.headers()).map(str::to_owned) else {
        return Err(login_required());
    };
    let token_hash = auth::token_hash(&token);
    let Some(login) = db::use_session(&state.pool, &token_hash, state.now()).await? else {
        return Err(login_required());
    };
    let cookie = auth::cookie(&token, request.headers());
    request
        .extensions_mut()
        .insert(Current { token_hash, login });
    let mut response = next.run(request).await;
    // Logging out sets its own cookie, which wins.
    if !response.headers().contains_key(header::SET_COOKIE) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

fn login_required() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "login_required",
        "Log in to continue.",
    )
}

/// `Err(too_many_attempts)` while `key` is paused by the login throttle at `now`.
pub async fn check_throttle(state: &AppState, key: &str, now: Timestamp) -> Result<(), ApiError> {
    let failures = db::recent_failures(&state.pool, key).await?;
    throttle::check(&failures, now).map_err(|paused| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            paused.to_string(),
        )
    })
}

#[derive(Serialize, ToSchema)]
pub struct Session {
    username: String,
    display_name: String,
}

/// `GET /api/v1/session`: who is logged in.
#[utoipa::path(
    get,
    path = "/api/v1/session",
    operation_id = "getSession",
    responses(
        (status = 200, description = "The login this session belongs to.", body = Session),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
    ),
)]
pub async fn get(Extension(current): Extension<Current>) -> Json<Session> {
    Json(Session {
        username: current.login.username,
        display_name: current.login.display_name,
    })
}

#[derive(Deserialize, ToSchema)]
pub struct LogIn {
    /// As typed: case and surrounding spaces don't matter.
    username: String,
    password: String,
}

/// `POST /api/v1/session`: logs in.
#[utoipa::path(
    post,
    path = "/api/v1/session",
    operation_id = "logIn",
    request_body = LogIn,
    responses(
        (
            status = 204,
            description = "Logged in.",
            headers(("Set-Cookie" = String, description = "The session cookie.")),
        ),
        (status = 401, description = "`login_failed`: the username or the password is wrong; it doesn't say which.", body = ErrorEnvelope),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
        (status = 429, description = "`too_many_attempts`: too many failures for this username; wait.", body = ErrorEnvelope),
    ),
)]
pub async fn log_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    Body(body): Body<LogIn>,
) -> Result<Response, ApiError> {
    let username = Username::new(&body.username).ok();
    // Throttled by the username as typed, so unknown ones are too; but in the same form a login's
    // own is kept in, so `Sam` and `sam` share a count *(agent)*.
    let key = username
        .as_ref()
        .map_or_else(|| body.username.trim().to_lowercase(), ToString::to_string);
    let now = state.now();
    check_throttle(&state, &key, now).await?;

    let login = match &username {
        Some(username) => db::password_login(&state.pool, username).await?,
        None => None,
    };
    let hash = login.as_ref().map(|login| login.password_hash.clone());
    let password = body.password;
    let verified =
        tokio::task::spawn_blocking(move || auth::verify_password(hash.as_deref(), &password))
            .await
            .map_err(|error| ApiError::internal(&error))?;
    let Some(login) = login.filter(|_| verified) else {
        db::record_failure(&state.pool, &key, now).await?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "login_failed",
            "Wrong username or password.",
        ));
    };

    let session = NewSession::generate().map_err(|error| ApiError::internal(&error))?;
    db::log_in(&state.pool, login.user_id, &session.token_hash, &key, now).await?;
    tracing::info!(user_id = login.user_id, "logged in");
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session.cookie(&headers))],
    )
        .into_response())
}

/// `DELETE /api/v1/session`: logs out, deleting this session.
#[utoipa::path(
    delete,
    path = "/api/v1/session",
    operation_id = "logOut",
    responses(
        (
            status = 204,
            description = "Logged out.",
            headers(("Set-Cookie" = String, description = "Clears the session cookie.")),
        ),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
    ),
)]
pub async fn log_out(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    db::end_session(&state.pool, &current.token_hash).await?;
    tracing::info!(user_id = current.login.user_id, "logged out");
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, auth::cleared_cookie(&headers))],
    )
        .into_response())
}
