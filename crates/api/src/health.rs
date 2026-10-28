use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::ErrorEnvelope;
use crate::{ApiError, AppState};

#[derive(Serialize, ToSchema)]
pub struct Health {
    /// Always `ok`: any other state is an error response.
    #[schema(value_type = String, example = "ok")]
    status: &'static str,
}

/// `GET /api/v1/health`, no auth, for the Docker healthcheck: 200 when the database answers.
#[utoipa::path(
    get,
    path = "/api/v1/health",
    operation_id = "health",
    responses(
        (status = 200, description = "The server and its database answer.", body = Health),
        (status = 503, description = "The database isn't answering.", body = ErrorEnvelope),
    ),
)]
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
