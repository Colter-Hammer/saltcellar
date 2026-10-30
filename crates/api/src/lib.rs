//! The HTTP API: JSON under `/api/v1/…` (docs/spec/foundations.md#http-api).

mod accounts;
mod auth;
mod categories;
mod error;
mod health;
mod household;
mod import;
mod middleware;
mod profile;
mod reset;
mod session;
mod setup;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;

use axum::routing::{any, delete, get, patch, post, put};
use axum::{Json, Router};
use domain::SetupCode;
use jiff::Timestamp;
use utoipa::OpenApi;

pub use error::ApiError;
pub use reset::{IssueResetError, ResetLink, issue_password_reset};

#[derive(OpenApi)]
#[openapi(
    info(title = "Saltcellar", description = "The Saltcellar HTTP API."),
    paths(
        health::get,
        setup::status,
        setup::currencies,
        setup::check_code,
        setup::finish,
        session::get,
        session::log_in,
        session::log_out,
        reset::check,
        reset::use_reset,
        profile::get,
        profile::change,
        profile::change_password,
        profile::log_out_everywhere,
        household::get,
        accounts::list,
        accounts::get,
        categories::list,
        categories::use_starter,
        categories::order,
        categories::add_group,
        categories::change_group,
        categories::delete_group,
        categories::add_category,
        categories::change_category,
        categories::delete_category,
        import::upload,
        import::get,
        import::commit
    ),
    components(schemas(error::ErrorEnvelope))
)]
struct Doc;

/// The OpenAPI document for the whole API, generated from the routes and types below.
/// `just gen-api` writes it to `web/src/api/openapi.json`, where the frontend's types come from.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    Doc::openapi()
}

/// What every handler can reach.
#[derive(Clone)]
pub struct AppState {
    pub pool: db::SqlitePool,
    setup: Arc<setup::Setup>,
    uploads: Arc<import::Uploads>,
    clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    Db(#[from] db::Error),
    #[error("can't make the setup code: no random bytes from the OS: {0}")]
    Random(getrandom::Error),
}

impl AppState {
    /// The state for a migrated database, with the data dir that holds it, where uploads wait too.
    /// While setup isn't finished, this start gets a new setup code: see [`AppState::setup_code`].
    ///
    /// # Errors
    /// If the database fails, or the OS has no random bytes for the code.
    pub async fn load(pool: db::SqlitePool, data_dir: &Path) -> Result<AppState, LoadError> {
        let setup = if db::household(&pool).await?.is_some() {
            setup::Setup::finished()
        } else {
            let mut bytes = [0u8; SetupCode::LEN];
            getrandom::fill(&mut bytes).map_err(LoadError::Random)?;
            setup::Setup::pending(SetupCode::from_random(bytes))
        };
        Ok(AppState {
            pool,
            setup: Arc::new(setup),
            uploads: Arc::new(import::Uploads::new(data_dir)),
            clock: Arc::new(Timestamp::now),
        })
    }

    /// The same state, telling the time by `clock`, so tests can move it on.
    #[cfg(test)]
    pub(crate) fn with_clock(self, clock: impl Fn() -> Timestamp + Send + Sync + 'static) -> Self {
        AppState {
            clock: Arc::new(clock),
            ..self
        }
    }

    /// The time now, by the state's clock.
    fn now(&self) -> Timestamp {
        (self.clock)()
    }

    /// The code that claims the install, until setup is finished. The caller prints it to the log,
    /// the one secret ever logged on purpose.
    #[must_use]
    pub fn setup_code(&self) -> Option<&SetupCode> {
        self.setup.code()
    }
}

/// The whole API, with the shared rules applied to every route:
/// one log line per request, and mutating requests must be JSON.
/// Until setup is finished, everything but health and the setup routes answers `setup_required`;
/// after, everything but those, logging in and the reset link answers `login_required` without a
/// live session.
/// Every path under `/api` is the API's, so an unknown one gets the JSON 404, never the frontend.
pub fn router(state: AppState) -> Router {
    let open = Router::new()
        .route("/health", get(health::get))
        .route("/setup", get(setup::status).post(setup::finish))
        .route("/setup/currencies", get(setup::currencies))
        .route("/setup/code", post(setup::check_code));
    let logged_in = Router::new()
        .route("/session", get(session::get).delete(session::log_out))
        .route("/sessions", delete(profile::log_out_everywhere))
        .route("/profile", get(profile::get).put(profile::change))
        .route("/profile/password", post(profile::change_password))
        .route("/household", get(household::get))
        .route("/accounts", get(accounts::list))
        .route("/accounts/{id}", get(accounts::get))
        .route(
            "/categories",
            get(categories::list).post(categories::add_category),
        )
        .route("/categories/starter", post(categories::use_starter))
        .route("/categories/order", put(categories::order))
        .route(
            "/categories/{id}",
            patch(categories::change_category).delete(categories::delete_category),
        )
        .route("/category-groups", post(categories::add_group))
        .route("/imports/actual", post(import::upload))
        .route("/imports/actual/{id}", get(import::get))
        .route("/imports/actual/{id}/commit", post(import::commit))
        .route(
            "/category-groups/{id}",
            patch(categories::change_group).delete(categories::delete_group),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            session::require_login,
        ));
    let gated = Router::new()
        .route("/session", post(session::log_in))
        .route("/password-reset", post(reset::use_reset))
        .route("/password-reset/check", post(reset::check))
        .merge(logged_in)
        .method_not_allowed_fallback(error::method_not_allowed)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            setup::gate,
        ));
    let v1 = open
        .merge(gated)
        .method_not_allowed_fallback(error::method_not_allowed);
    Router::new()
        .nest("/api/v1", v1)
        .route("/api/v1/", any(setup::not_found))
        .route("/api/v1/{*rest}", any(setup::not_found))
        .route("/api/openapi.json", get(Json(openapi())))
        .route("/api", any(error::not_found))
        .route("/api/", any(error::not_found))
        .route("/api/{*rest}", any(error::not_found))
        .layer(axum::middleware::from_fn(middleware::require_json))
        .layer(axum::middleware::from_fn(middleware::log_request))
        .with_state(state)
}
