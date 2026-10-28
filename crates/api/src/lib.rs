//! The HTTP API: JSON under `/api/v1/…` (docs/spec/foundations.md#http-api).

mod error;
mod health;
mod middleware;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::get;

pub use error::ApiError;

/// What every handler can reach.
#[derive(Clone)]
pub struct AppState {
    pub pool: db::SqlitePool,
}

/// The whole API, with the shared rules applied to every route:
/// one log line per request, and mutating requests must be JSON.
pub fn router(state: AppState) -> Router {
    let v1 = Router::new()
        .route("/health", get(health::get))
        .fallback(error::not_found)
        .method_not_allowed_fallback(error::method_not_allowed);
    Router::new()
        .nest("/api/v1", v1)
        .layer(axum::middleware::from_fn(middleware::require_json))
        .layer(axum::middleware::from_fn(middleware::log_request))
        .with_state(state)
}
