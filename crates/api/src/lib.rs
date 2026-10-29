//! The HTTP API: JSON under `/api/v1/…` (docs/spec/foundations.md#http-api).

mod error;
mod health;
mod middleware;
#[cfg(test)]
mod tests;

use axum::routing::{any, get};
use axum::{Json, Router};
use utoipa::OpenApi;

pub use error::ApiError;

#[derive(OpenApi)]
#[openapi(
    info(title = "Saltcellar", description = "The Saltcellar HTTP API."),
    paths(health::get),
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
}

/// The whole API, with the shared rules applied to every route:
/// one log line per request, and mutating requests must be JSON.
/// Every path under `/api` is the API's, so an unknown one gets the JSON 404, never the frontend.
pub fn router(state: AppState) -> Router {
    let v1 = Router::new()
        .route("/health", get(health::get))
        .fallback(error::not_found)
        .method_not_allowed_fallback(error::method_not_allowed);
    Router::new()
        .nest("/api/v1", v1)
        .route("/api/openapi.json", get(Json(openapi())))
        .route("/api", any(error::not_found))
        .route("/api/", any(error::not_found))
        .route("/api/{*rest}", any(error::not_found))
        .layer(axum::middleware::from_fn(middleware::require_json))
        .layer(axum::middleware::from_fn(middleware::log_request))
        .with_state(state)
}
