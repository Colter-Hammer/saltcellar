//! Password reset links: issued by the `reset-password` CLI command, used on the set-new-password
//! page (docs/spec/m1-ledger-import.md#login-and-profile). The token is only ever in the link's
//! fragment and in request bodies, so no request line, and no log, holds it.

use std::fmt;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use domain::login::{Password, PasswordError, Username};
use jiff::Timestamp;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::{self, NewSession};
use crate::error::{Body, ErrorEnvelope};
use crate::{ApiError, AppState};

/// The frontend's set-new-password page.
pub const PAGE: &str = "/reset-password";

/// A new reset link's token. Its `Debug` hides it, so it can't reach a log by accident.
pub struct ResetLink(String);

impl ResetLink {
    /// The link's path: the page, with the token in the fragment, which browsers never send.
    #[must_use]
    pub fn path(&self) -> String {
        format!("{PAGE}#{}", self.0)
    }
}

impl fmt::Debug for ResetLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResetLink(..)")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IssueResetError {
    #[error("there's no login named {0:?}")]
    UnknownLogin(String),
    #[error("can't make the link: no random bytes from the OS: {0}")]
    Random(getrandom::Error),
    #[error(transparent)]
    Db(#[from] db::Error),
}

/// Issues a one-time reset link for the login named `username` (as typed), valid for 24 hours from
/// `now`. Any link the login had before stops working.
///
/// # Errors
/// [`IssueResetError::UnknownLogin`] if there's no such login (or it was removed); otherwise only
/// if the OS has no random bytes or the database fails.
pub async fn issue_password_reset(
    pool: &db::SqlitePool,
    username: &str,
    now: Timestamp,
) -> Result<ResetLink, IssueResetError> {
    let unknown = || IssueResetError::UnknownLogin(username.trim().to_owned());
    let name = Username::new(username).map_err(|_| unknown())?;
    let token = auth::random_token().map_err(IssueResetError::Random)?;
    if !db::issue_password_reset(pool, &name, &auth::token_hash(&token), None, now).await? {
        return Err(unknown());
    }
    Ok(ResetLink(token))
}

fn link_dead() -> ApiError {
    ApiError::new(
        StatusCode::GONE,
        "reset_link_dead",
        "This reset link has expired or was already used. Ask for a new one.",
    )
}

#[derive(Deserialize, ToSchema)]
pub struct CheckReset {
    /// The token from the link's fragment.
    token: String,
}

/// `POST /api/v1/password-reset/check`: whether a reset link still works, for the page to say so
/// before anyone types a password.
#[utoipa::path(
    post,
    path = "/api/v1/password-reset/check",
    operation_id = "checkPasswordReset",
    request_body = CheckReset,
    responses(
        (status = 204, description = "The link works."),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
        (status = 410, description = "`reset_link_dead`: it expired, was used, or a newer one replaced it.", body = ErrorEnvelope),
    ),
)]
pub async fn check(
    State(state): State<AppState>,
    Body(body): Body<CheckReset>,
) -> Result<StatusCode, ApiError> {
    let hash = auth::token_hash(&body.token);
    match db::password_reset(&state.pool, &hash, state.now()).await? {
        Some(_) => Ok(StatusCode::NO_CONTENT),
        None => Err(link_dead()),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct UseReset {
    /// The token from the link's fragment.
    token: String,
    /// At least 12 characters.
    password: String,
    /// The password typed again.
    password_again: String,
}

/// `POST /api/v1/password-reset`: sets a new password with a reset link, which then stops working,
/// and logs in. The login's other sessions end *(agent)*.
#[utoipa::path(
    post,
    path = "/api/v1/password-reset",
    operation_id = "usePasswordReset",
    request_body = UseReset,
    responses(
        (
            status = 204,
            description = "The password is set and this browser is logged in.",
            headers(("Set-Cookie" = String, description = "The session cookie.")),
        ),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
        (status = 410, description = "`reset_link_dead`: it expired, was used, or a newer one replaced it.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`, naming each field that's wrong.", body = ErrorEnvelope),
    ),
)]
pub async fn use_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Body(body): Body<UseReset>,
) -> Result<Response, ApiError> {
    let token_hash = auth::token_hash(&body.token);
    // A dead link says so before the password does: a new password wouldn't help.
    if db::password_reset(&state.pool, &token_hash, state.now())
        .await?
        .is_none()
    {
        return Err(link_dead());
    }
    let password = new_password(&body.password, &body.password_again)?;
    let password_hash = crate::profile::hash(password).await?;
    let session = NewSession::generate().map_err(|error| ApiError::internal(&error))?;
    let reset = db::UseReset {
        token_hash: &token_hash,
        password_hash: &password_hash,
        session_token_hash: &session.token_hash,
    };
    let Some(user_id) = db::use_password_reset(&state.pool, &reset, state.now()).await? else {
        return Err(link_dead());
    };
    tracing::info!(user_id, "password reset");
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session.cookie(&headers))],
    )
        .into_response())
}

/// A new password, typed twice, or the 422 naming the field that's wrong.
pub fn new_password(typed: &str, again: &str) -> Result<Password, ApiError> {
    Password::new(typed, again).map_err(|error| {
        let field = match error {
            PasswordError::Mismatch => "password_again",
            PasswordError::TooShort | PasswordError::TooLong => "password",
        };
        ApiError::validation(
            "Some details need another look.",
            [(field.to_owned(), error.to_string())].into(),
        )
    })
}
