//! Claiming a fresh install: the setup code, the gate and the wizard's routes
//! (docs/spec/m1-ledger-import.md#setup-wizard).

use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use domain::login::{DisplayName, Password, PasswordError, Username};
use domain::{CURRENCIES, Currency, SetupCode, TimeZone, throttle};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::{self, NewSession};
use crate::error::{Body, ErrorEnvelope};
use crate::{ApiError, AppState};

/// The throttle key for wrong setup codes: the login throttle's, keyed on `setup` *(agent)*.
const THROTTLE_KEY: &str = "setup";

/// Whether setup is finished, and the code that claims the install until it is.
pub struct Setup {
    code: Option<SetupCode>,
    done: AtomicBool,
}

impl Setup {
    pub fn finished() -> Setup {
        Setup {
            code: None,
            done: AtomicBool::new(true),
        }
    }

    pub fn pending(code: SetupCode) -> Setup {
        Setup {
            code: Some(code),
            done: AtomicBool::new(false),
        }
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// The code that claims the install, while setup isn't finished.
    pub fn code(&self) -> Option<&SetupCode> {
        self.code.as_ref().filter(|_| !self.is_done())
    }

    fn finish(&self) {
        self.done.store(true, Ordering::Release);
    }
}

/// While setup isn't finished, every route behind this answers `setup_required`.
pub async fn gate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if state.setup.is_done() {
        next.run(request).await
    } else {
        setup_required().into_response()
    }
}

/// The `/api/v1` fallback, which is behind the gate too: an unknown path is `setup_required`
/// until setup is finished, `not_found` after.
pub async fn not_found(State(state): State<AppState>) -> ApiError {
    if state.setup.is_done() {
        crate::error::not_found().await
    } else {
        setup_required()
    }
}

fn setup_required() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "setup_required",
        "Finish setting up the app first.",
    )
}

#[derive(Serialize, ToSchema)]
pub struct SetupStatus {
    /// Whether setup is finished. Until it is, the app shows only the setup wizard.
    done: bool,
}

/// `GET /api/v1/setup`: whether setup is finished.
#[utoipa::path(
    get,
    path = "/api/v1/setup",
    operation_id = "getSetup",
    responses((status = 200, description = "Whether setup is finished.", body = SetupStatus)),
)]
pub async fn status(State(state): State<AppState>) -> Json<SetupStatus> {
    Json(SetupStatus {
        done: state.setup.is_done(),
    })
}

/// `GET /api/v1/setup/currencies`: the currencies the household can choose.
#[utoipa::path(
    get,
    path = "/api/v1/setup/currencies",
    operation_id = "getCurrencies",
    responses((
        status = 200,
        description = "Every ISO 4217 code in circulation, sorted.",
        body = [String],
    )),
)]
pub async fn currencies() -> Json<&'static [&'static str]> {
    Json(CURRENCIES)
}

#[derive(Deserialize, ToSchema)]
pub struct CheckSetupCode {
    /// As typed: case, spaces and dashes don't matter.
    code: String,
}

/// `POST /api/v1/setup/code`: whether this is the setup code, for the wizard's first step.
#[utoipa::path(
    post,
    path = "/api/v1/setup/code",
    operation_id = "checkSetupCode",
    request_body = CheckSetupCode,
    responses(
        (status = 204, description = "It's the setup code."),
        (status = 403, description = "`setup_code_wrong`: it isn't.", body = ErrorEnvelope),
        (status = 409, description = "`setup_done`: setup is already finished.", body = ErrorEnvelope),
        (status = 429, description = "`too_many_attempts`: too many wrong codes; wait.", body = ErrorEnvelope),
    ),
)]
pub async fn check_code(
    State(state): State<AppState>,
    Body(body): Body<CheckSetupCode>,
) -> Result<StatusCode, ApiError> {
    verify_code(&state, &body.code).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct FinishSetup {
    /// The setup code again, as typed.
    code: String,
    /// ISO 4217, one of `GET /api/v1/setup/currencies`.
    currency: String,
    /// An IANA timezone name.
    timezone: String,
    display_name: String,
    username: String,
    /// At least 12 characters.
    password: String,
    /// The password typed again.
    password_again: String,
}

/// `POST /api/v1/setup`: finishes setup. Writes the household and the first login in one go, and
/// logs that login in.
#[utoipa::path(
    post,
    path = "/api/v1/setup",
    operation_id = "finishSetup",
    request_body = FinishSetup,
    responses(
        (
            status = 204,
            description = "Setup is finished and the new login is logged in.",
            headers(("Set-Cookie" = String, description = "The session cookie.")),
        ),
        (status = 403, description = "`setup_code_wrong`: the code isn't the setup code.", body = ErrorEnvelope),
        (status = 409, description = "`setup_done`: setup is already finished.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`, naming each field that's wrong.", body = ErrorEnvelope),
        (status = 429, description = "`too_many_attempts`: too many wrong codes; wait.", body = ErrorEnvelope),
    ),
)]
pub async fn finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Body(body): Body<FinishSetup>,
) -> Result<Response, ApiError> {
    verify_code(&state, &body.code).await?;

    let mut fields = BTreeMap::new();
    let currency = field(&mut fields, "currency", body.currency.parse::<Currency>());
    let timezone = field(&mut fields, "timezone", body.timezone.parse::<TimeZone>());
    let display_name = field(
        &mut fields,
        "display_name",
        DisplayName::new(&body.display_name),
    );
    let username = field(&mut fields, "username", Username::new(&body.username));
    let password = match Password::new(&body.password, &body.password_again) {
        Ok(password) => Some(password),
        Err(error @ PasswordError::Mismatch) => {
            fields.insert("password_again".to_owned(), error.to_string());
            None
        }
        Err(error) => {
            fields.insert("password".to_owned(), error.to_string());
            None
        }
    };
    let (Some(currency), Some(timezone), Some(display_name), Some(username), Some(password)) =
        (currency, timezone, display_name, username, password)
    else {
        return Err(ApiError::validation(
            "Some details need another look.",
            fields,
        ));
    };

    let password_hash = tokio::task::spawn_blocking(move || auth::hash_password(&password))
        .await
        .map_err(|error| ApiError::internal(&error))?
        .map_err(|error| ApiError::internal(&error))?;
    let session = NewSession::generate().map_err(|error| ApiError::internal(&error))?;
    let setup = db::NewSetup {
        currency,
        timezone: &timezone,
        display_name: &display_name,
        username: &username,
        password_hash: &password_hash,
        session_token_hash: &session.token_hash,
    };
    match db::finish_setup(&state.pool, &setup, Timestamp::now()).await {
        Ok(()) => {}
        Err(db::FinishSetupError::AlreadyDone) => {
            state.setup.finish();
            return Err(setup_done());
        }
        Err(db::FinishSetupError::Db(error)) => return Err(error.into()),
    }
    state.setup.finish();
    tracing::info!("setup finished");
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session.cookie(&headers))],
    )
        .into_response())
}

/// `Ok` if `typed` is the setup code. Wrong codes count towards the throttle.
async fn verify_code(state: &AppState, typed: &str) -> Result<(), ApiError> {
    let Some(code) = state.setup.code() else {
        return Err(setup_done());
    };
    let now = Timestamp::now();
    let failures = db::recent_failures(&state.pool, THROTTLE_KEY).await?;
    if let Err(paused) = throttle::check(&failures, now) {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            paused.to_string(),
        ));
    }
    if code.matches(typed) {
        return Ok(());
    }
    db::record_failure(&state.pool, THROTTLE_KEY, now).await?;
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        "setup_code_wrong",
        "That isn't the setup code. Copy it from the app's log: a new one is printed each time the app starts.",
    ))
}

fn setup_done() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "setup_done",
        "Setup is already finished.",
    )
}

/// The value, or `None` with the field's error recorded.
fn field<T, E: Display>(
    fields: &mut BTreeMap<String, String>,
    name: &str,
    parsed: Result<T, E>,
) -> Option<T> {
    parsed
        .map_err(|error| fields.insert(name.to_owned(), error.to_string()))
        .ok()
}
