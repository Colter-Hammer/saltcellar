use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::ErrorEnvelope;
use crate::{ApiError, AppState};

#[derive(Serialize, ToSchema)]
pub struct Household {
    /// ISO 4217: every amount is in minor units of it.
    #[schema(example = "USD")]
    currency: String,
    /// IANA: it decides what "today" is.
    #[schema(example = "America/Chicago")]
    timezone: String,
}

/// `GET /api/v1/household`: the household's currency and timezone.
#[utoipa::path(
    get,
    path = "/api/v1/household",
    operation_id = "getHousehold",
    responses(
        (status = 200, description = "The household.", body = Household),
        (status = 403, description = "`setup_required`: setup isn't finished.", body = ErrorEnvelope),
    ),
)]
pub async fn get(State(state): State<AppState>) -> Result<Json<Household>, ApiError> {
    // Behind the setup gate, so the household exists unless the database lost it.
    let household = db::household(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
    Ok(Json(Household {
        currency: household.currency.code().to_owned(),
        timezone: household.timezone.name().to_owned(),
    }))
}
