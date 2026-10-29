//! The HTTP API: JSON under `/api/v1/…` (docs/spec/foundations.md#http-api).

mod auth;
mod error;
mod health;
mod household;
mod middleware;
mod setup;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::routing::{any, get, post};
use axum::{Json, Router};
use domain::SetupCode;
use utoipa::OpenApi;

pub use error::ApiError;

#[derive(OpenApi)]
#[openapi(
    info(title = "Saltcellar", description = "The Saltcellar HTTP API."),
    paths(
        health::get,
        setup::status,
        setup::currencies,
        setup::check_code,
        setup::finish,
        household::get
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
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    Db(#[from] db::Error),
    #[error("can't make the setup code: no random bytes from the OS: {0}")]
    Random(getrandom::Error),
}

impl AppState {
    /// The state for a migrated database. While setup isn't finished, this start gets a new
    /// setup code: see [`AppState::setup_code`].
    ///
    /// # Errors
    /// If the database fails, or the OS has no random bytes for the code.
    pub async fn load(pool: db::SqlitePool) -> Result<AppState, LoadError> {
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
        })
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
/// Until setup is finished, everything but health and the setup routes answers `setup_required`.
/// Every path under `/api` is the API's, so an unknown one gets the JSON 404, never the frontend.
pub fn router(state: AppState) -> Router {
    let open = Router::new()
        .route("/health", get(health::get))
        .route("/setup", get(setup::status).post(setup::finish))
        .route("/setup/currencies", get(setup::currencies))
        .route("/setup/code", post(setup::check_code));
    let gated = Router::new()
        .route("/household", get(household::get))
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
