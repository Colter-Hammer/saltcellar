use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use crate::{ApiError, AppState};

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
}

/// `GET /api/v1/health`, no auth, for the Docker healthcheck: 200 when the database answers.
pub async fn get(State(state): State<AppState>) -> Result<Json<Health>, ApiError> {
    match db::ping(&state.pool).await {
        Ok(()) => Ok(Json(Health { status: "ok" })),
        Err(error) => {
            tracing::error!(%error, "health check: the database didn't answer");
            Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "The database isn't answering.",
            ))
        }
    }
}
