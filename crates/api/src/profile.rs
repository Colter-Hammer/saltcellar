//! Settings › Profile: a login's own display name, locale and week start, its password, and
//! logging out everywhere (docs/spec/m1-ledger-import.md#login-and-profile).

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use domain::login::{DisplayName, Password};
use domain::profile::{Locale, WeekStart};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth;
use crate::error::{Body, ErrorEnvelope};
use crate::session::{Current, check_throttle};
use crate::{ApiError, AppState};

#[derive(Serialize, ToSchema)]
pub struct Profile {
    username: String,
    display_name: String,
    /// BCP 47, like `de-DE`; null = the browser's locale.
    display_locale: Option<String>,
    /// 0 = Sunday … 6 = Saturday; null = from the locale.
    #[schema(minimum = 0, maximum = 6)]
    week_start: Option<u8>,
}

impl From<db::Profile> for Profile {
    fn from(profile: db::Profile) -> Self {
        Profile {
            username: profile.username,
            display_name: profile.display_name,
            display_locale: profile.display_locale,
            week_start: profile.week_start,
        }
    }
}

/// `GET /api/v1/profile`: the logged-in login's profile.
#[utoipa::path(
    get,
    path = "/api/v1/profile",
    operation_id = "getProfile",
    responses(
        (status = 200, description = "The profile.", body = Profile),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
    ),
)]
pub async fn get(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
) -> Result<Json<Profile>, ApiError> {
    let profile = db::profile(&state.pool, current.login.user_id).await?;
    Ok(Json(profile.into()))
}

#[derive(Deserialize, ToSchema)]
pub struct ChangeProfile {
    display_name: String,
    /// BCP 47, like `de-DE`; null = the browser's locale.
    display_locale: Option<String>,
    /// 0 = Sunday … 6 = Saturday; null = from the locale.
    week_start: Option<i64>,
}

/// `PUT /api/v1/profile`: saves the display name, locale and week start.
#[utoipa::path(
    put,
    path = "/api/v1/profile",
    operation_id = "changeProfile",
    request_body = ChangeProfile,
    responses(
        (status = 200, description = "The saved profile.", body = Profile),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`, naming each field that's wrong.", body = ErrorEnvelope),
    ),
)]
pub async fn change(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Body(body): Body<ChangeProfile>,
) -> Result<Json<Profile>, ApiError> {
    let mut fields = BTreeMap::new();
    let mut field = |name: &str, error: String| fields.insert(name.to_owned(), error);
    let display_name = DisplayName::new(&body.display_name)
        .map_err(|error| field("display_name", error.to_string()))
        .ok();
    let locale = body
        .display_locale
        .map(|tag| tag.parse::<Locale>())
        .transpose()
        .map_err(|error| field("display_locale", error.to_string()));
    let week_start = body
        .week_start
        .map(WeekStart::new)
        .transpose()
        .map_err(|error| field("week_start", error.to_string()));
    let (Some(display_name), Ok(locale), Ok(week_start)) = (display_name, locale, week_start)
    else {
        return Err(ApiError::validation(
            "Some details need another look.",
            fields,
        ));
    };
    let change = db::ProfileChange {
        display_name: &display_name,
        display_locale: locale.as_ref(),
        week_start,
    };
    db::update_profile(&state.pool, current.login.user_id, &change).await?;
    get(State(state), Extension(current)).await
}

#[derive(Deserialize, ToSchema)]
pub struct ChangePassword {
    current_password: String,
    /// At least 12 characters.
    password: String,
    /// The new password typed again.
    password_again: String,
}

/// `POST /api/v1/profile/password`: changes the password, given the current one. Wrong current
/// passwords count towards the login throttle, keyed on the username *(agent)*. Every session
/// stays logged in; Log out everywhere ends them.
#[utoipa::path(
    post,
    path = "/api/v1/profile/password",
    operation_id = "changePassword",
    request_body = ChangePassword,
    responses(
        (status = 204, description = "The password is changed."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`, naming each field that's wrong, the current password among them.", body = ErrorEnvelope),
        (status = 429, description = "`too_many_attempts`: too many wrong passwords for this login; wait.", body = ErrorEnvelope),
    ),
)]
pub async fn change_password(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Body(body): Body<ChangePassword>,
) -> Result<StatusCode, ApiError> {
    let user_id = current.login.user_id;
    let key = &current.login.username;
    let now = state.now();
    check_throttle(&state, key, now).await?;

    let old_hash = db::password_hash(&state.pool, user_id).await?;
    let typed = body.current_password;
    let verified =
        tokio::task::spawn_blocking(move || auth::verify_password(old_hash.as_deref(), &typed))
            .await
            .map_err(|error| ApiError::internal(&error))?;
    if !verified {
        db::record_failure(&state.pool, key, now).await?;
        return Err(ApiError::validation(
            "Some details need another look.",
            [(
                "current_password".to_owned(),
                "That isn't your current password.".to_owned(),
            )]
            .into(),
        ));
    }
    let password = crate::reset::new_password(&body.password, &body.password_again)?;
    let new_hash = hash(password).await?;
    db::set_password(&state.pool, user_id, &new_hash).await?;
    tracing::info!(user_id, "password changed");
    Ok(StatusCode::NO_CONTENT)
}

/// The argon2id hash of `password`, made off the async threads.
pub async fn hash(password: Password) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || auth::hash_password(&password))
        .await
        .map_err(|error| ApiError::internal(&error))?
        .map_err(|error| ApiError::internal(&error))
}

/// `DELETE /api/v1/sessions`: logs out everywhere, deleting every session this login has, this
/// one included.
#[utoipa::path(
    delete,
    path = "/api/v1/sessions",
    operation_id = "logOutEverywhere",
    responses(
        (
            status = 204,
            description = "Every session of this login has ended.",
            headers(("Set-Cookie" = String, description = "Clears the session cookie.")),
        ),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
    ),
)]
pub async fn log_out_everywhere(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    db::end_all_sessions(&state.pool, current.login.user_id).await?;
    tracing::info!(user_id = current.login.user_id, "logged out everywhere");
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, auth::cleared_cookie(&headers))],
    )
        .into_response())
}
